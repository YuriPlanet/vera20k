//! Runtime-adapter regressions. Native arithmetic vectors live in
//! render::foot_depth; these tests exercise which live state reaches it.

use super::{
    depth_cell, shp_z_adjust_in_runtime, unit_bridge_split_in_runtime, unit_z_adjust_in_runtime,
};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::Simulation;

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
    let terrain = crate::map::resolved_terrain::test_grid(8, 8, |x, y| {
        crate::map::resolved_terrain::test_smudge_cell(x, y)
    });
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
    let owner = runtime.simulation.intern("OWNER");
    let type_ref = runtime.simulation.intern(name);
    GameEntity::new_at_frame_zero_for_test(
        1,
        2,
        2,
        0,
        0,
        owner,
        Health::new(100),
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
        .resolved_terrain_mut_for_test()
        .as_mut()
        .unwrap()
        .cell_mut_for_test(coord.0, coord.1)
        .unwrap()
        .level = level;
}

#[test]
fn native_on_bridge_controls_the_cliff_gate_and_shp_surface() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    set_live_level(&mut runtime, (3, 3), 4);
    tank.set_on_bridge(false);
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &tank, true, None),
        19
    );
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &tank), 17.0);

    tank.set_on_bridge(true);
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &tank, true, None),
        -1
    );
    // Exactly on the native bridge surface: GetHeight()==0 still reaches Foot,
    // even when the derived occupancy marker is absent.
    tank.position
        .set_exact_z_leptons_for_test(Some(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS));
    let foot = unit_z_adjust_in_runtime(Some(&runtime), &tank, true, None);
    assert_eq!(
        shp_z_adjust_in_runtime(Some(&runtime), &tank),
        (foot - 2) as f32
    );
    assert_eq!(foot, -61);
}

/// Aircraft draw their voxels through Foot's vt+0x2EC (0x4DAFC0) like the
/// Foot classes: beside the same cliff as the tank above, whose default
/// ZFudgeCliff=10 gives 19, a JET with ZFudgeCliff=99 takes its own fudge.
/// Its SHP route keeps the height-only term (DrawSHP's aircraft arm).
#[test]
fn an_aircraft_voxel_takes_the_foot_cliff_fudge() {
    let mut runtime = runtime();
    let jet = entity(&mut runtime, "JET", EntityCategory::Aircraft);
    set_live_level(&mut runtime, (3, 3), 4);
    assert_eq!(
        unit_z_adjust_in_runtime(Some(&runtime), &jet, true, None),
        197
    );
    assert_eq!(shp_z_adjust_in_runtime(Some(&runtime), &jet), -2.0);
}

#[test]
fn fixed_slot_alias_and_missing_cells_leave_the_shared_simulation_dummy_unchanged() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    set_live_level(&mut runtime, (0, 2), 5);
    let terrain = runtime.simulation.resolved_terrain().unwrap();
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
        tank.position.set_rx_for_test(rx);
        tank.position.set_ry_for_test(ry);
        let _ = unit_z_adjust_in_runtime(Some(&runtime), &tank, true, None);
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
        .resolved_terrain_mut_for_test()
        .as_mut()
        .unwrap()
        .cell_mut_for_test(2, 2)
        .unwrap()
        .bridge_facts
        .raw_flags = 0x100;
    assert!(
        evaluate(&runtime, &tank),
        "zero ZFudgeBridge must not disable the split"
    );
    tank.set_on_bridge(true);
    assert!(!evaluate(&runtime, &tank));
    tank.set_on_bridge(false);
    for cells in [&[(2, 3)][..], &[(3, 3)][..], &[(2, 3), (3, 3)][..]] {
        for &(x, y) in cells {
            // Synthetic grid base is -1: 5 - (-1) + 1 = native column tile 7.
            runtime
                .simulation
                .resolved_terrain_mut_for_test()
                .as_mut()
                .unwrap()
                .cell_mut_for_test(x, y)
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
                .resolved_terrain_mut_for_test()
                .as_mut()
                .unwrap()
                .cell_mut_for_test(x, y)
                .unwrap()
                .final_tile_index = 0;
        }
    }
    tank.set_category_for_test(EntityCategory::Infantry);
    assert!(!evaluate(&runtime, &tank));
}

#[test]
fn composite_factory_split_requires_navcom_and_slot_zero_weapons_factory() {
    let mut runtime = runtime();
    let mut tank = entity(&mut runtime, "TANK", EntityCategory::Unit);
    let mut factory = entity(&mut runtime, "FACTORY", EntityCategory::Structure);
    factory.set_stable_id_for_test(17);
    runtime.simulation.entities_mut_for_test().insert(factory);
    let evaluate =
        |rt: &SimRuntime, e: &GameEntity| unit_bridge_split_in_runtime(Some(rt), e, true);
    tank.radio_contacts.insert(17);
    assert!(!evaluate(&runtime, &tank));
    tank.navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(3, 3)));
    assert!(
        !super::shp_unit_bridge_fudge_in_runtime(Some(&runtime), &tank),
        "direct SHP has no weapons-factory alternative"
    );
    assert!(
        evaluate(&runtime, &tank),
        "native only tests NavCom presence, not target identity"
    );
    tank.set_on_bridge(true);
    assert!(
        evaluate(&runtime, &tank),
        "factory arm is independent of bridge proximity"
    );
    runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(17)
        .unwrap()
        .set_category_for_test(EntityCategory::Unit);
    assert!(!evaluate(&runtime, &tank));
    runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(17)
        .unwrap()
        .set_category_for_test(EntityCategory::Structure);
    runtime.resources.rules = RuleSet::from_ini(&IniFile::from_str(
        &RULES.replace("WeaponsFactory=yes", "WeaponsFactory=no"),
    ))
    .unwrap();
    assert!(!evaluate(&runtime, &tank));
}
