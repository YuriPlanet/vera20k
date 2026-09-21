//! Runtime-adapter regressions. Native arithmetic vectors live in
//! render::foot_depth; these tests exercise which live state reaches it.

use super::{
    depth_cell, shp_z_adjust_in_runtime, unit_bridge_split_in_runtime, unit_z_adjust_in_runtime,
};
use crate::map::bridge_facts::BridgeCellFacts;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::cloak_disguise::DisguiseRuntime;
use crate::sim::components::{BridgeOccupancy, Health};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

const RULES: &str = "\
[VehicleTypes]
0=TANK
1=CUSTOM
2=HARV
3=UNLOAD
[InfantryTypes]
0=INF
[AircraftTypes]
0=JET
[BuildingTypes]
0=FACTORY
1=REPAIR
[FACTORY]
WeaponsFactory=yes
[REPAIR]
Factory=UnitType
[TANK]
Name=Default cliff coefficient
TooBigToFitUnderBridge=yes
ZFudgeColumn=0
ZFudgeBridge=0
[CUSTOM]
ZFudgeCliff=7
[HARV]
Harvester=yes
UnloadingClass=UNLOAD
TooBigToFitUnderBridge=no
[UNLOAD]
ZFudgeCliff=3
TooBigToFitUnderBridge=yes
[INF]
Name=Infantry default
[JET]
ZFudgeCliff=99
";

fn runtime() -> SimRuntime {
    let mut resources = SimResources::empty();
    resources.rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("fixture rules parse");
    let cells = (0..8)
        .flat_map(|y| (0..8).map(move |x| flat_cell(x, y)))
        .collect();
    let terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
    resources.terrain_template = Some(terrain.clone());
    let mut simulation = Simulation::new();
    simulation.intern_rule_type_ids(&resources.rules);
    simulation.install_resolved_terrain_for_new_map(terrain);
    SimRuntime {
        simulation,
        resources,
    }
}

fn entity(runtime: &mut SimRuntime, name: &str, category: EntityCategory) -> GameEntity {
    let owner = runtime.simulation.interner.intern("OWNER");
    let type_ref = runtime.simulation.interner.intern(name);
    GameEntity::new_at_frame_zero_for_test(
        1,
        2,
        2,
        0,
        0,
        owner,
        Health { current: 100 },
        type_ref,
        category,
        0,
        5,
        category == EntityCategory::Unit,
    )
}

fn set_live_level(runtime: &mut SimRuntime, coord: (u16, u16), level: u8) {
    runtime
        .simulation
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(coord.0, coord.1)
        .unwrap()
        .level = level;
}

#[test]
fn runtime_rules_and_live_cliff_edits_reach_depth_instead_of_the_load_template() {
    let mut runtime = runtime();
    let tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    let custom = entity(&mut runtime, "CUSTOM", EntityCategory::Unit);
    // Stale load resources claim a cliff while the live simulation is flat.
    runtime
        .resources
        .terrain_template
        .as_mut()
        .unwrap()
        .cell_mut(3, 3)
        .unwrap()
        .level = 9;
    runtime.resources.height_map.insert((3, 3), 9);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), -1);

    // This is an adapter sequence, not another exhaustive helper truth table:
    // runtime changes must be visible immediately, without replacing resources.
    set_live_level(&mut runtime, (3, 3), 4);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), 19);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &custom, true), 13);
    set_live_level(&mut runtime, (4, 4), 4);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), 9);
    set_live_level(&mut runtime, (3, 3), 0);
    set_live_level(&mut runtime, (4, 4), 0);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), -1);
    assert_eq!(
        runtime
            .resources
            .terrain_template
            .as_ref()
            .unwrap()
            .cell(3, 3)
            .unwrap()
            .level,
        9
    );
}

#[test]
fn native_on_bridge_controls_the_cliff_gate_and_shp_surface_independently_of_occupancy() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    set_live_level(&mut runtime, (3, 3), 4);
    tank.bridge_occupancy = Some(BridgeOccupancy { deck_level: 4 });
    tank.on_bridge = false;
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), 19);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &tank), 17.0);

    tank.bridge_occupancy = None;
    tank.on_bridge = true;
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), -1);
    // Exactly on the native bridge surface: GetHeight()==0 still reaches Foot,
    // even when the derived occupancy marker is absent.
    tank.position.exact_z_leptons = Some(crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32);
    let foot = unit_z_adjust_in_runtime(Some(&runtime), &tank, true);
    assert_eq!(
        shp_z_adjust_in_runtime(Some(&runtime), &tank),
        (foot - 2) as f32
    );
    assert_eq!(foot, -61);
}

#[test]
fn shp_uses_exact_height_and_category_while_raw_unit_depth_keeps_foot() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    let infantry = entity(&mut runtime, "INF", EntityCategory::Infantry);
    let mut aircraft = entity(&mut runtime, "JET", EntityCategory::Aircraft);
    set_live_level(&mut runtime, (3, 3), 4);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &infantry), 17.0);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &aircraft), -2.0);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &aircraft, true), 0);

    // An exact one-lepton height excludes Foot even though it rounds to no
    // screen lift and there is no air-layer locomotor.
    tank.position.exact_z_leptons = Some(1);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &tank), -2.0);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), 19);

    let mut locomotor =
        LocomotorState::from_object_type(runtime.resources.rules.object("TANK").unwrap(), 0);
    locomotor.kind = LocomotorKind::Jumpjet;
    locomotor.layer = MovementLayer::Air;
    locomotor.altitude = SimFixed::from_num(0);
    tank.locomotor = Some(locomotor);
    tank.position.exact_z_leptons = None;
    assert_eq!(
        shp_z_adjust_in_runtime(Some(&runtime), &tank),
        17.0,
        "air layer alone is not GetHeight"
    );
    tank.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(104);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &tank), -17.0);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), 4);
    aircraft.position.exact_z_leptons = Some(104);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &aircraft), -17.0);
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &aircraft, true),
        -15
    );
    assert_eq!(shp_z_adjust_in_runtime(None, &tank), -17.0);
    assert_eq!(unit_z_adjust_in_runtime(None, &tank, true), -15);
}

#[test]
fn shp_ramp_gate_preserves_coarse_grounded_poses_and_checks_exact_surface_height() {
    use crate::util::lepton::ground_height_leptons;
    use crate::util::native_x87::adjust_for_z_standard;

    let mut runtime = runtime();
    let mut infantry = entity(&mut runtime, "INF", EntityCategory::Infantry);
    infantry.position.sub_x = SimFixed::from_num(128);
    infantry.position.sub_y = SimFixed::from_num(128);
    runtime
        .simulation
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 2)
        .unwrap()
        .slope_type = 1;
    set_live_level(&mut runtime, (3, 3), 4);
    let surface = ground_height_leptons(0, 1, 640, 640).unwrap();
    assert!(surface > 0, "fixture is above its coarse cell-level Z");

    // Ground movement retains only coarse cell-level Z. Its zero semantic
    // altitude must still reach Foot when the subcell ramp surface is higher.
    assert!(infantry.position.exact_z_leptons.is_none());
    assert_eq!(
        shp_z_adjust_in_runtime(Some(&runtime), &infantry),
        (unit_z_adjust_in_runtime(Some(&runtime), &infantry, true) - 2) as f32
    );
    assert_ne!(shp_z_adjust_in_runtime(Some(&runtime), &infantry), -2.0);

    // An exact coordinate instead permits the actual GetHeight subtraction.
    infantry.position.exact_z_leptons = Some(0);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &infantry), -2.0);
    infantry.position.exact_z_leptons = Some(surface);
    assert_eq!(
        shp_z_adjust_in_runtime(Some(&runtime), &infantry),
        (unit_z_adjust_in_runtime(Some(&runtime), &infantry, true) - 2) as f32
    );
    infantry.position.exact_z_leptons = Some(surface + 1);
    let airborne = shp_z_adjust_in_runtime(Some(&runtime), &infantry);
    assert_eq!(airborne, (-adjust_for_z_standard(surface + 1) - 2) as f32);
    assert_ne!(
        airborne,
        (unit_z_adjust_in_runtime(Some(&runtime), &infantry, true) - 2) as f32
    );
}

#[test]
fn actual_type_wins_display_art_except_the_harvester_unloading_body() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    let mut harvester = entity(&mut runtime, "HARV", EntityCategory::Unit);
    let display = runtime.simulation.interner.intern("CUSTOM");
    let unloading = runtime.simulation.interner.intern("UNLOAD");
    set_live_level(&mut runtime, (3, 3), 4);
    tank.display_type_override = Some(display);
    tank.disguise = Some(DisguiseRuntime {
        disguised: true,
        disguise_type: Some(unloading),
        ..Default::default()
    });
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, true), 19);
    assert_eq!(unit_z_adjust_in_runtime(Some(&runtime), &tank, false), 19);
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &tank), 17.0);

    // Unit+6C4 is temporarily replaced for the unloading body. The original
    // type still owns the callers that explicitly do not request that body.
    harvester.display_type_override = Some(unloading);
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &harvester, true),
        5
    );
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &harvester), 3.0);
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &harvester, false),
        19
    );
    harvester.display_type_override = None;
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &harvester, true),
        19
    );
}

#[test]
fn fixed_slot_alias_and_missing_cells_leave_the_shared_simulation_dummy_unchanged() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    set_live_level(&mut runtime, (0, 2), 5);
    let terrain = runtime.simulation.resolved_terrain.as_ref().unwrap();
    let dummy = terrain.shared_cell_dummy();
    dummy.stamp_coord(91, -17);
    dummy.set_level_slope(-7, 0);
    dummy.set_overlay_fields(Some(12), 43);
    let before = dummy.snapshot();
    let overlay_before = dummy.overlay_fields();

    let alias = depth_cell(terrain, None, None, [512, 1]);
    assert!(!alias.is_dummy);
    assert_eq!(alias.coord, [0, 2]);
    assert_eq!(alias.level, 5);
    let missing = depth_cell(terrain, None, None, [500, 500]);
    assert!(missing.is_dummy);
    assert_eq!(missing.coord, [500, 500]);
    assert_eq!(missing.level, -7);

    for (rx, ry) in [(512, 1), (500, 500), (0, 0)] {
        tank.position.rx = rx;
        tank.position.ry = ry;
        let _ = unit_z_adjust_in_runtime(Some(&runtime), &tank, true);
        let _ = shp_z_adjust_in_runtime(Some(&runtime), &tank);
        assert_eq!(
            dummy.snapshot(),
            before,
            "presentation stamped dummy at ({rx},{ry})"
        );
        assert_eq!(dummy.overlay_fields(), overlay_before);
    }
}

#[test]
fn composite_split_uses_live_raw_bridge_terms_and_native_on_bridge() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    let evaluate =
        |rt: &SimRuntime, e: &GameEntity| unit_bridge_split_in_runtime(Some(rt), e, true);
    assert!(!evaluate(&runtime, &tank));
    runtime
        .simulation
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 2)
        .unwrap()
        .bridge_facts
        .raw_flags = 0x100;
    assert!(
        evaluate(&runtime, &tank),
        "zero ZFudgeBridge must not disable the split"
    );
    tank.bridge_occupancy = Some(BridgeOccupancy { deck_level: 4 });
    assert!(
        evaluate(&runtime, &tank),
        "native on_bridge, not derived occupancy"
    );
    tank.on_bridge = true;
    assert!(!evaluate(&runtime, &tank));
    tank.on_bridge = false;
    for cells in [&[(2, 3)][..], &[(3, 3)][..], &[(2, 3), (3, 3)][..]] {
        for &(x, y) in cells {
            // Synthetic grid base is -1: 5 - (-1) + 1 = native column tile 7.
            runtime
                .simulation
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap()
                .final_tile_index = 5;
        }
        assert!(
            !evaluate(&runtime, &tank),
            "raw score 1/2 must reject with ZFudgeColumn=0"
        );
        for &(x, y) in cells {
            runtime
                .simulation
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap()
                .final_tile_index = 0;
        }
    }
    tank.category = EntityCategory::Infantry;
    assert!(!evaluate(&runtime, &tank));
}

#[test]
fn direct_shp_bridge_fudge_uses_unit_no_turret_branch_and_excludes_infantry() {
    let mut runtime = runtime();
    runtime
        .simulation
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 2)
        .unwrap()
        .bridge_facts
        .raw_flags = 0x100;
    let mut unit = entity(&mut runtime, "TANK", EntityCategory::Unit);
    assert!(super::shp_unit_bridge_fudge_in_runtime(
        Some(&runtime),
        &unit
    ));
    unit.category = EntityCategory::Infantry;
    assert!(!super::shp_unit_bridge_fudge_in_runtime(
        Some(&runtime),
        &unit
    ));
    unit.category = EntityCategory::Unit;
    runtime.resources.rules = RuleSet::from_ini(&IniFile::from_str(
        &RULES.replace("[TANK]", "[TANK]\nTurret=yes"),
    ))
    .unwrap();
    assert!(!super::shp_unit_bridge_fudge_in_runtime(
        Some(&runtime),
        &unit
    ));
}

#[test]
fn composite_split_uses_active_unloading_type_and_not_art_override() {
    let mut runtime = runtime();
    runtime
        .simulation
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 2)
        .unwrap()
        .bridge_facts
        .raw_flags = 0x100;
    let mut harvester = entity(&mut runtime, "HARV", EntityCategory::Unit);
    assert!(!unit_bridge_split_in_runtime(
        Some(&runtime),
        &harvester,
        true
    ));
    harvester.display_type_override = Some(runtime.simulation.interner.intern("UNLOAD"));
    assert!(unit_bridge_split_in_runtime(
        Some(&runtime),
        &harvester,
        true
    ));
    assert!(!unit_bridge_split_in_runtime(
        Some(&runtime),
        &harvester,
        false
    ));
    let mut ordinary = entity(&mut runtime, "CUSTOM", EntityCategory::Unit);
    ordinary.display_type_override = harvester.display_type_override;
    assert!(!unit_bridge_split_in_runtime(
        Some(&runtime),
        &ordinary,
        true
    ));
}

#[test]
fn composite_factory_split_requires_navcom_and_slot_zero_weapons_factory() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    let mut factory = entity(&mut runtime, "FACTORY", EntityCategory::Structure);
    factory.stable_id = 17;
    runtime.simulation.entities_mut().insert(factory);
    let evaluate =
        |rt: &SimRuntime, e: &GameEntity| unit_bridge_split_in_runtime(Some(rt), e, true);
    tank.radio_contacts.insert(17);
    assert!(!evaluate(&runtime, &tank));
    tank.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(3, 3));
    assert!(
        !super::shp_unit_bridge_fudge_in_runtime(Some(&runtime), &tank),
        "direct SHP has no weapons-factory alternative"
    );
    assert!(
        evaluate(&runtime, &tank),
        "native only tests NavCom presence, not target identity"
    );
    tank.on_bridge = true;
    assert!(
        evaluate(&runtime, &tank),
        "factory arm is independent of bridge proximity"
    );
    runtime
        .simulation
        .entities_mut()
        .get_mut(17)
        .unwrap()
        .category = EntityCategory::Unit;
    assert!(!evaluate(&runtime, &tank));
    runtime
        .simulation
        .entities_mut()
        .get_mut(17)
        .unwrap()
        .category = EntityCategory::Structure;
    runtime.resources.rules = RuleSet::from_ini(&IniFile::from_str(
        &RULES.replace("WeaponsFactory=yes", "WeaponsFactory=no"),
    ))
    .unwrap();
    assert!(!evaluate(&runtime, &tank));
}

fn flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
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
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        height_in_pixels: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: true,
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
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: SpeedCostProfile::default(),
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
