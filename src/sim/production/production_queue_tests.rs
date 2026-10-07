//! Production queue tests — verifies build queue ordering, credit deduction, prerequisite
//! checks, multi-factory speed bonus, and queue pause/resume behavior.

use super::{
    BuildQueueState, ProductionCategory, build_options_for_owner, cancel_by_type_for_owner,
    credits_for_owner, dispatch_production_changes_for_tests, enqueue_by_type,
    queue_view_for_owner, suspend_production,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use crate::sim::world::{SimSoundEvent, Simulation};

// Re-use test helpers from the main production_tests module.
// P5d: build state is now constructed by ARMING the registry (`arm_build_via`), not by
// inserting `BuildQueueItem`s into the retired `queues_by_owner`.
use super::tests::{
    arm_build_via, basic_infantry_rules, basic_multi_queue_rules, build_catalog_rules,
    naval_production_rules, production_modifier_rules, spawn_structure, water_terrain,
};

fn factory_constructor_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n[InfantryTypes]\n0=E1\n1=E2\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPILE\n1=GAWEAP\n\
         [E1]\nCost=200\nStrength=100\nSpeed=4\nTechLevel=1\nOwner=Americans\n\
         [E2]\nCost=300\nStrength=125\nSpeed=4\nTechLevel=1\nOwner=Americans\n\
         [MTNK]\nCost=700\nStrength=300\nSpeed=6\nTechLevel=1\nOwner=Americans\n\
         [GAPILE]\nOwner=Americans\nFactory=InfantryType\n\
         [GAWEAP]\nOwner=Americans\nFactory=UnitType\n",
    ))
    .expect("factory constructor rules")
}

/// Instruction-backed redirection inputs, through the production retail reader
/// and the existing flat-map/StartProduction fixtures. Native4444B3..444562
/// selects from House+68 and temporarily moves Building+524; this fixture does
/// not implement either decision. The saved original instructions are in
/// refactor-goal/unit-factory-fresh-critic-native-instructions-20261002.json,
/// exit_object_archive_and_busy_redirect. Whole multi-factory native execution
/// is separate from these control/order and restoration regressions.
fn busy_factory_exit_world() -> Option<(Simulation, RuleSet, InternedId)> {
    let (ini, art) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut sim = Simulation::with_seed(0x4444_B300);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 50_000, 10),
    );
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for (id, cell) in [(1, (6, 6)), (2, (10, 20)), (3, (20, 10))] {
        spawn_structure(&mut sim, id, "Americans", "GAWEAP", cell.0, cell.1);
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_projection
        .replace_buildings_for_test(vec![1, 3, 2]);
    sim.production
        .set_primary_factory_for_test(owner, ProductionCategory::Vehicle, 1);
    busy_factory_exit_mission(&mut sim, 1, crate::sim::mission::MissionType::Unload);
    for (id, archive) in [(1, (8, 8)), (2, (12, 24)), (3, (24, 14))] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_archive_target(Some(crate::sim::combat::TargetKind::Cell(
                archive.0, archive.1,
            )));
    }
    Some((sim, rules, owner))
}

fn busy_factory_exit_mission(
    sim: &mut Simulation,
    building: u64,
    mission: crate::sim::mission::MissionType,
) {
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: crate::sim::mission::MissionId::from_known(mission),
            suspended: crate::sim::mission::MissionId::NONE,
            queued: crate::sim::mission::MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
}

fn busy_factory_exit_product(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    attached: bool,
) -> u64 {
    if !attached {
        arm_build_via(
            sim,
            rules,
            "Americans",
            "MTNK",
            ProductionCategory::Vehicle,
            70,
        );
        assert!(
            sim.production
                .factory_shadow
                .test_arm_ready(owner, ProductionCategory::Vehicle)
        );
        return super::factory_lifecycle::active_entity_id(sim, owner, ProductionCategory::Vehicle)
            .unwrap();
    }
    let type_id = sim.intern("MTNK");
    let cost = sim.cost_of(owner, rules.object("MTNK").unwrap(), rules);
    sim.production.factory_shadow.create_building_factory(
        1,
        owner,
        ProductionCategory::Vehicle,
        type_id,
        70,
        cost,
    );
    let product = super::factory_lifecycle::start_active_production(
        sim,
        rules,
        super::factory::FactoryHolder::Building(1),
        type_id,
    )
    .unwrap();
    let factory = sim.production.factory_shadow.test_first_mut().unwrap();
    factory.progress = super::PRODUCTION_STEPS;
    factory.balance = 0;
    factory.suspended = true;
    product
}

/// Invoke the same ExitObject owner used by the House and Building callers.
/// The return boundary deliberately precedes their successful factory release.
fn busy_factory_exit_attempt(
    sim: &mut Simulation,
    rules: &RuleSet,
    _owner: InternedId,
    product: u64,
) -> Option<u64> {
    (super::production_queue::exit_produced_object(sim, rules, 1, product, None)
        == crate::sim::ai_base_building::BuildingExit::Placed)
        .then_some(product)
}

fn assert_busy_factory_exit_receiver(
    sim: &Simulation,
    rules: &RuleSet,
    product: u64,
    receiver: u64,
) {
    let producer = sim.substrate.entities.get(receiver).unwrap();
    let expected = crate::sim::movement::building_exit_coordinate(
        crate::sim::movement::ground_pose::position_world_coord(&producer.position),
        rules.object(sim.resolve(producer.type_ref())).unwrap(),
        || {
            crate::sim::movement::ground_pose::object_get_coords(
                producer,
                sim.resolved_terrain.as_ref(),
            )
        },
    );
    let placed = sim.substrate.entities.get(product).unwrap();
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&placed.position),
        expected,
        "the existing coordinate owner uses the chosen receiver's supplied pose"
    );
    assert_eq!(placed.archive_target(), producer.archive_target());
    assert!(!placed.lifecycle.in_limbo);
    assert_eq!(
        producer.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Unload)
    );
}

#[test]
fn busy_factory_exit_player_uses_house_order_without_moving_primary() {
    let Some((mut sim, rules, owner)) = busy_factory_exit_world() else {
        return;
    };
    assert_eq!(sim.houses[&owner].base_projection.buildings(), [1, 3, 2]);
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .map(|entity| entity.stable_id())
            .collect::<Vec<_>>(),
        [1, 2, 3],
        "House order is deliberately different from storage/object order"
    );
    let product = busy_factory_exit_product(&mut sim, &rules, owner, false);
    let rng = sim.scenario_rng.logical_state();
    assert!(dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_busy_factory_exit_receiver(&sim, &rules, product, 3);
    assert_eq!(sim.scenario_rng.logical_state(), rng);
    assert_eq!(
        sim.production
            .primary_factory(owner, ProductionCategory::Vehicle),
        Some(1)
    );
    assert!(sim.production.factory_shadow.is_empty());
    assert!(!sim.object_placement_scope_active());
}

#[test]
fn busy_factory_exit_skips_attached_different_type_and_non_guard_buildings() {
    let Some((mut sim, rules, owner)) = busy_factory_exit_world() else {
        return;
    };
    spawn_structure(&mut sim, 4, "Americans", "NAWEAP", 20, 20);
    spawn_structure(&mut sim, 5, "Americans", "GAWEAP", 24, 20);
    busy_factory_exit_mission(&mut sim, 3, crate::sim::mission::MissionType::Move);
    // Current NONE + queued Guard must be admitted by the same effective
    // mission getter that native vt+184 executes at4444FF.
    let guard = &mut sim.substrate.entities.get_mut(5).unwrap().mission;
    guard.apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
        current: crate::sim::mission::MissionId::NONE,
        suspended: crate::sim::mission::MissionId::NONE,
        queued: crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
    });
    let mtnk = sim.intern("MTNK");
    sim.production.factory_shadow.create_building_factory(
        2,
        owner,
        ProductionCategory::Vehicle,
        mtnk,
        90,
        700,
    );
    let attached = sim.production.factory_shadow.building_factory(2).cloned();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_projection
        .replace_buildings_for_test(vec![1, 2, 4, 3, 5]);
    let product = busy_factory_exit_product(&mut sim, &rules, owner, false);
    assert!(dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_busy_factory_exit_receiver(&sim, &rules, product, 5);
    assert_eq!(
        sim.production.factory_shadow.building_factory(2),
        attached.as_ref(),
        "any non-null +524 excludes that candidate without changing its factory"
    );
    assert!(sim.production.factory_shadow.building_factory(5).is_none());
    assert_eq!(
        sim.production
            .primary_factory(owner, ProductionCategory::Vehicle),
        Some(1)
    );
}

#[test]
fn busy_factory_exit_restores_building_attachment_after_success() {
    let Some((mut sim, rules, owner)) = busy_factory_exit_world() else {
        return;
    };
    let product = busy_factory_exit_product(&mut sim, &rules, owner, true);
    let registry = sim.production.factory_shadow.clone();
    let credits = sim.houses[&owner].economy.credits;
    let next_id = sim.substrate.next_stable_object_id;
    assert_eq!(
        busy_factory_exit_attempt(&mut sim, &rules, owner, product),
        Some(product)
    );
    assert_busy_factory_exit_receiver(&sim, &rules, product, 3);
    assert_eq!(sim.production.factory_shadow, registry);
    assert!(sim.production.factory_shadow.building_factory(3).is_none());
    assert_eq!(sim.houses[&owner].economy.credits, credits);
    assert_eq!(sim.substrate.next_stable_object_id, next_id);
    assert_eq!(
        sim.production
            .primary_factory(owner, ProductionCategory::Vehicle),
        Some(1)
    );
    assert!(!sim.object_placement_scope_active());
}

#[test]
fn busy_factory_exit_attachment_owner_preserves_identity_and_rejects_occupied_target() {
    let Some((mut sim, rules, owner)) = busy_factory_exit_world() else {
        return;
    };
    let product = busy_factory_exit_product(&mut sim, &rules, owner, true);
    let mtnk = sim.intern("MTNK");
    let cost = sim.cost_of(owner, rules.object("MTNK").unwrap(), &rules);
    sim.production.factory_shadow.create_building_factory(
        3,
        owner,
        ProductionCategory::Vehicle,
        mtnk,
        91,
        cost,
    );
    let registry = sim.production.factory_shadow.clone();
    let source = sim.production.factory_shadow.building_factory(1).cloned();
    let credits = sim.houses[&owner].economy.credits;
    let next_id = sim.substrate.next_stable_object_id;
    for (from, to) in [(1, 3), (1, 1), (4, 2)] {
        assert!(
            !sim.production
                .factory_shadow
                .transfer_building_factory_attachment(from, to)
        );
        assert_eq!(sim.production.factory_shadow, registry);
    }
    assert!(
        sim.production
            .factory_shadow
            .transfer_building_factory_attachment(1, 2)
    );
    assert!(sim.production.factory_shadow.building_factory(1).is_none());
    assert_eq!(
        sim.production.factory_shadow.building_factory(2),
        source.as_ref(),
        "reattachment retains the existing Factory and its held object"
    );
    assert_eq!(source.unwrap().object.unwrap().entity_id, Some(product));
    assert!(
        sim.production
            .factory_shadow
            .transfer_building_factory_attachment(2, 1)
    );
    assert_eq!(sim.production.factory_shadow, registry);
    assert_eq!(sim.houses[&owner].economy.credits, credits);
    assert_eq!(sim.substrate.next_stable_object_id, next_id);
}

#[test]
fn busy_factory_exit_failed_chosen_receiver_returns_once_and_restores_attachment() {
    for attached in [false, true] {
        let Some((mut sim, rules, owner)) = busy_factory_exit_world() else {
            return;
        };
        let product = busy_factory_exit_product(&mut sim, &rules, owner, attached);
        // Explicit existing Reveal early-refusal control. This tests the
        // caller's failure/restoration ordering, not a native production cause
        // for refusal or an invented ScenarioActive flag. Both later receivers
        // would see the same refusal; their distinct archives witness whether
        // the caller incorrectly attempts another eligible candidate.
        sim.substrate
            .entities
            .get_mut(product)
            .unwrap()
            .lifecycle
            .cell_marked = true;
        let registry = sim.production.factory_shadow.clone();
        let credits = sim.houses[&owner].economy.credits;
        let next_id = sim.substrate.next_stable_object_id;
        let rng = (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        assert_eq!(
            busy_factory_exit_attempt(&mut sim, &rules, owner, product),
            None
        );
        let held = sim.substrate.entities.get(product).unwrap();
        assert!(held.lifecycle.in_limbo);
        assert_eq!(
            held.archive_target(),
            sim.substrate.entities.get(3).unwrap().archive_target(),
            "the first eligible receiver copies its archive before failing; no later receiver runs"
        );
        assert_eq!(sim.production.factory_shadow, registry);
        assert!(sim.production.factory_shadow.building_factory(3).is_none());
        assert_eq!(sim.houses[&owner].economy.credits, credits);
        assert_eq!(sim.substrate.next_stable_object_id, next_id);
        assert_eq!(
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng
        );
        assert!(!sim.object_placement_scope_active());
    }
}

#[test]
fn busy_factory_exit_without_an_alternate_refunds_player_product_and_promotes_queue() {
    let Some((mut sim, rules, owner)) = busy_factory_exit_world() else {
        return;
    };
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_projection
        .replace_buildings_for_test(vec![1]);
    let product = busy_factory_exit_product(&mut sim, &rules, owner, false);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        71,
    );
    let credits = sim.houses[&owner].economy.credits;
    let refund = sim.cost_of(owner, rules.object("MTNK").unwrap(), &rules);
    assert!(!dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    // ExitObject4444B3's busy return1 still copies Archive; the direct native
    // rows below preserve that boundary. Human HousePlace4FB57F then sees
    // producer+524zero and abandons this paid product, not the queued tail.
    assert!(!sim.substrate.entities.contains(product));
    assert_eq!(sim.houses[&owner].economy.credits, credits + refund);
    let factory = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .unwrap();
    assert!(!factory.ready);
    assert_eq!(factory.progress, 0);
    assert!(factory.queue.is_empty());
    let successor = factory.object.unwrap().entity_id.unwrap();
    assert!(successor > product);
    assert!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .lifecycle
            .in_limbo
    );
    assert!(sim.production.factory_shadow.building_factory(1).is_none());
}

#[test]
fn busy_factory_exit_original_rows_compare_receiver_archive_restoration_and_rng() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::combat::TargetKind;
    use crate::sim::components::DriveCoord;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};
    use crate::sim::movement::ground_pose;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    let data: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/unit_unlimbo.json",
    ))
    .unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(
        data["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let rows = data["factory_busy_redirect_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(&ini);
    let road = terrain_rules.semantics_for_land_type(1).unwrap();
    let int = |value: &Value| i32::try_from(value.as_i64().unwrap()).unwrap();
    let coord = |value: &Value| DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    };
    let archive = |value: &Value| {
        value.as_array().map(|cell| {
            TargetKind::Cell(
                u16::try_from(cell[0].as_u64().unwrap()).unwrap(),
                u16::try_from(cell[1].as_u64().unwrap()).unwrap(),
            )
        })
    };
    let archive_cell = |value: Option<TargetKind>| match value {
        Some(TargetKind::Cell(rx, ry)) => json!([rx, ry]),
        None => Value::Null,
        other => panic!("unmeasured redirect archive {other:?}"),
    };
    let controls = data["factory_exit_coordinate_rows"].as_array().unwrap();
    assert_eq!(controls.len(), 5);
    let busy_controls: Vec<_> = controls
        .iter()
        .filter(|row| row["input"]["route"] == "busy")
        .collect();
    assert_eq!(busy_controls.len(), 2);
    for (row, control) in rows.iter().map(|row| (row, None)).chain(
        busy_controls
            .iter()
            .map(|control| (&control["whole_exit"], Some(*control))),
    ) {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut fixture_ini = ini.clone();
        if let Some(control) = control {
            if input["exit_coord_control"] == "constructor_missing" {
                fixture_ini = fixture_ini.without_entry_for_test("GAWEAP", "ExitCoord");
            } else {
                let raw = control["setup"]["exit_coord"]["read_passes"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["supplied_section"]["ExitCoord"]
                    .as_str()
                    .unwrap();
                fixture_ini.merge(&IniFile::from_str(&format!("[GAWEAP]\nExitCoord={raw}\n")));
            }
        }
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&fixture_ini, &art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        if let Some(control) = control {
            let exit = rules.object("GAWEAP").unwrap().exit_coord;
            assert_eq!(
                exit.is_none(),
                input["exit_coord_control"] == "constructor_missing"
            );
            let supplied = exit.map_or_else(
                || control["setup"]["exit_coord"]["ctor_exit_coord"].clone(),
                |(x, y, z)| json!([x, y, z]),
            );
            assert_eq!(supplied, control["setup"]["exit_coord"]["final_coord"]);
        }
        let before = &row["before"];
        let after = &row["after"];
        assert_eq!(row["entry"], "0x00443C60");
        assert_eq!(before["counter"], 0, "these callers begin outside scope");
        assert_eq!(
            row["before"]["factory_bytes"],
            row["after"]["factory_bytes"]
        );
        let mut sim = Simulation::with_seed(0);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        // Preserve the packet's real XYZ. The surrounding Road/level4 cells
        // are allocated fixture storage; the native placement cells below
        // supply their actual derived terrain, not a full map-load claim.
        let cells = (0..128)
            .flat_map(|ry| {
                (0..128).map(move |rx| {
                    let mut cell =
                        crate::sim::world::common_raw_test_terrain_cell(rx, ry, 4, false);
                    cell.land_type = 1;
                    cell.yr_cell_land_type = 1;
                    cell.terrain_class = road.terrain_class;
                    cell.base_terrain_class = road.terrain_class;
                    cell.speed_costs = road.speed_costs.clone();
                    cell.base_speed_costs = road.speed_costs.clone();
                    cell
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(128, 128, cells));
        let placement_cells = row["placement_cells_before"].as_object().unwrap();
        assert_eq!(placement_cells.len(), if control.is_some() { 4 } else { 3 });
        for native in placement_cells.values() {
            let rx = native["coord"][0].as_u64().unwrap() as u16;
            let ry = native["coord"][1].as_u64().unwrap() as u16;
            let land = native["land"].as_u64().unwrap() as u8;
            let semantics = terrain_rules.semantics_for_land_type(land).unwrap();
            let target = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(rx, ry)
                .unwrap();
            target.level = native["level"].as_u64().unwrap() as u8;
            target.slope_type = native["slope"].as_u64().unwrap() as u8;
            target.land_type = land;
            target.yr_cell_land_type = land;
            target.zone_type = native["zone_type"].as_u64().unwrap() as u8;
            target.terrain_class = semantics.terrain_class;
            target.base_terrain_class = semantics.terrain_class;
            target.speed_costs = semantics.speed_costs.clone();
            target.base_speed_costs = semantics.speed_costs.clone();
            target.final_tile_index = int(&native["tile"]);
            target.final_sub_tile = native["subtile"].as_u64().unwrap() as u8;
            target.bridge_facts.raw_flags = native["flags"].as_u64().unwrap() as u32;
            assert_eq!(
                target.bridge_facts.raw_flags & 0x100,
                0,
                "{name}: these three measured placement cells have no bridge"
            );
        }
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        });
        sim.session.map_width = 128;
        sim.session.map_height = 128;
        sim.session.binary_frame = before["product"]["frame"].as_u64().unwrap() as u32;
        sim.session.game_mode_nonzero = int(&before["actual_game_mode"]) != 0;
        let owner = sim.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 50_000, 10),
        );
        let mut labels = BTreeMap::new();
        let mut producer_types = vec![("source", input["producer_type"].as_str().unwrap())];
        producer_types.extend(
            input["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|candidate| {
                    (
                        candidate["label"].as_str().unwrap(),
                        candidate["type"].as_str().unwrap(),
                    )
                }),
        );
        for (label, type_name) in &producer_types {
            let native = &before["producers"][label];
            let id = sim
                .construct_object_limbo_at_height(type_name, "Americans", 0, 0, 0, 0, &rules)
                .unwrap();
            assert!(labels.insert(*label, id).is_none());
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            ground_pose::put_location(&mut entity.position, coord(&native["xyz"]));
            entity.set_archive_target(archive(&native["archive_cell"]));
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(int(&native["current"])),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(int(&native["queued"])),
                movement_bypass_latch: 0,
                handler_state: native["status"].as_u64().unwrap() as u32,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(sim.session.binary_frame),
            });
        }
        assert_eq!(
            labels["source"], 1,
            "existing queue fixture source identity"
        );
        let order = input["house_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| labels[label.as_str().unwrap()])
            .collect();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .base_projection
            .replace_buildings_for_test(order);
        sim.production.set_primary_factory_for_test(
            owner,
            ProductionCategory::Vehicle,
            labels["source"],
        );
        let source_attached = before["producers"]["source"]["attachment"] != "0x0";
        assert_eq!(source_attached, input["source_attachment"] == "Building");
        let product = busy_factory_exit_product(&mut sim, &rules, owner, source_attached);
        let type_id = sim.intern("MTNK");
        let cost = sim.cost_of(owner, rules.object("MTNK").unwrap(), &rules);
        for (label, _) in &producer_types {
            if *label != "source" && before["producers"][label]["attachment"] != "0x0" {
                sim.production.factory_shadow.create_building_factory(
                    labels[label],
                    owner,
                    ProductionCategory::Vehicle,
                    type_id,
                    90 + labels[label],
                    cost,
                );
            }
        }
        let actor = sim.substrate.entities.get_mut(product).unwrap();
        ground_pose::put_location(&mut actor.position, coord(&before["product"]["position"]));
        actor.set_archive_target(archive(&before["product_archive_cell"]));
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(int(&before["product"]["mission"])),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(int(&before["product"]["queued"])),
            movement_bypass_latch: 0,
            handler_state: before["product"]["status"].as_u64().unwrap() as u32,
            mission_start_frame: 0,
            ai_counter: before["product"]["mission_visit_count"].as_u64().unwrap() as u32,
            dispatch_timer: MissionDispatchTimer::from_raw(
                int(&before["product"]["dispatch"][0]),
                int(&before["product"]["dispatch"][1]),
            ),
        });
        if before["scenario_active"] == 0 {
            // Native executes Object5F4EC0's actual ScenarioActive refusal.
            // Rust has no ScenarioActive owner: use its represented early
            // Reveal refusal solely to compare caller selection, archive,
            // delayed result, restoration and RNG. Mark/lifecycle parity for
            // this different refusal cause is explicitly excluded.
            assert_eq!(before["product"]["marked"], 0);
            actor.lifecycle.cell_marked = true;
        }
        // Native before is post-constructor. Import that declared boundary;
        // this compares ExitObject, not Factory construction/charge or release.
        sim.main_rng = serde_json::from_value(row["rng_before"]["main"].clone()).unwrap();
        sim.scenario_rng = serde_json::from_value(row["rng_before"]["scenario"].clone()).unwrap();
        sim.mapgen_rng = serde_json::from_value(row["rng_before"]["mapgen"].clone()).unwrap();
        let registry = sim.production.factory_shadow.clone();
        let credits = sim.houses[&owner].economy.credits;
        let next_id = sim.substrate.next_stable_object_id;
        for (label, id) in &labels {
            assert_eq!(
                json!(
                    sim.substrate
                        .entities
                        .get(*id)
                        .unwrap()
                        .mission
                        .effective()
                        .raw()
                ),
                row["effective_before"][label],
                "{name}: supplied effective mission for {label}"
            );
        }
        for (stream, rng) in [
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert!(
                rng.native_state_hex() == row["rng_pair"][stream]["before_hex"].as_str().unwrap(),
                "{name}: complete native {stream} caller RNG"
            );
        }
        let result = busy_factory_exit_attempt(&mut sim, &rules, owner, product);
        // The original returns 0 for a refused Unlimbo, 1 for a held exit,
        // and 2 for delivery. This caller's Option reports delivery only.
        assert!(matches!(row["returned_eax"].as_u64(), Some(0..=2)));
        assert_eq!(
            result,
            (row["returned_eax"] == 2).then_some(product),
            "{name}"
        );
        let placed = sim.substrate.entities.get(product).unwrap();
        let actual_archive = archive_cell(placed.archive_target());
        assert_eq!(
            actual_archive, after["product_archive_cell"],
            "{name}: archive"
        );
        // Distinct original Cell archives witness the recursive receiver even
        // when placement fails. This does not reimplement its eligibility.
        let archive_witnesses: Vec<_> = producer_types
            .iter()
            .filter(|(label, _)| before["producers"][label]["archive_cell"] == actual_archive)
            .map(|(label, _)| *label)
            .collect();
        assert_eq!(
            archive_witnesses.len(),
            1,
            "{name}: unique receiver witness"
        );
        let receiver = archive_witnesses[0];
        let recursive_receiver = (receiver != "source").then_some(receiver);
        assert_eq!(
            json!(recursive_receiver),
            row["selected_receiver"],
            "{name}: first receiver"
        );
        assert_eq!(
            row["recursive_return_eax"].as_array().unwrap().len(),
            usize::from(recursive_receiver.is_some()),
            "{name}: original nested call count"
        );
        if result.is_some() {
            assert_eq!(placed.radio_contacts.slot(0), Some(labels[receiver]));
        }
        let location = ground_pose::position_world_coord(&placed.position);
        assert_eq!(
            json!([location.x, location.y, location.z]),
            after["product"]["position"],
            "{name}: original placement XYZ"
        );
        if let Some(control) = control {
            let cell = &control["actual_cell"]["coord"];
            let (rx, ry) = (
                cell[0].as_u64().unwrap() as u16,
                cell[1].as_u64().unwrap() as u16,
            );
            assert_eq!(json!([placed.position.rx, placed.position.ry]), *cell);
            let plane = &control["actual_cell"]["plane_after"];
            assert_eq!(
                json!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry)),
                plane[2]
            );
            assert_eq!(
                json!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry)),
                plane[3]
            );
        }
        assert_eq!(
            sim.production.factory_shadow, registry,
            "{name}: same Factory values"
        );
        assert_eq!(sim.houses[&owner].economy.credits, credits);
        assert_eq!(sim.substrate.next_stable_object_id, next_id);
        assert_eq!(
            sim.production
                .primary_factory(owner, ProductionCategory::Vehicle),
            Some(labels["source"])
        );
        for (label, id) in &labels {
            let entity = sim.substrate.entities.get(*id).unwrap();
            let native = &after["producers"][label];
            assert_eq!(
                native["attachment"],
                before["producers"][label]["attachment"]
            );
            assert_eq!(
                sim.production
                    .factory_shadow
                    .building_factory(*id)
                    .is_some(),
                native["attachment"] != "0x0",
                "{name}: restored attachment for {label}"
            );
            assert_eq!(
                archive_cell(entity.archive_target()),
                native["archive_cell"]
            );
            assert_eq!(json!(entity.mission.current().raw()), native["current"]);
            assert_eq!(json!(entity.mission.queued().raw()), native["queued"]);
            assert_eq!(
                json!(entity.mission.effective().raw()),
                row["effective_after"][label]
            );
        }
        assert_eq!(after["counter"], before["counter"]);
        assert!(!sim.object_placement_scope_active());
        for (stream, rng) in [
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert!(
                rng.native_state_hex() == row["rng_pair"][stream]["after_hex"].as_str().unwrap(),
                "{name}: complete native {stream} return RNG"
            );
        }
    }
}

/// Original Strip6A8DD3 consumes Factory4C9C60's change flag, then queues
/// PLACE6A8EB8; Event4C710B -> House4FB0E0 reveals the held GI next frame.
/// Joined active-retail controls observe completion267/484 and PLACE268/485.
/// Source: basic-factory-output-prerequisites-research, construction-primary,
/// raw SHA9ff9c219b81c93fe5671b6a153073c09aa4d14c04472baeff0a5680977c1963d.
#[test]
fn human_mobile_completion_retains_identity_until_next_frame_place() {
    let rules = factory_constructor_rules();
    let mut sim = Simulation::with_seed(0xFAC7_0002);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 14, 14);
    let owner = sim.interner.get("Americans").unwrap();
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E2"));
    let held = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();

    let completed_frame = (0..10_000)
        .find(|&frame| {
            sim.session.binary_frame = frame;
            sim.session.tick = u64::from(frame);
            super::publish_production_changes(&mut sim, &rules);
            super::revalidate_and_step_factories(&mut sim, &rules);
            let completed = sim
                .production
                .factory_shadow
                .view(owner, ProductionCategory::Infantry)
                .is_some_and(|factory| factory.ready);
            completed
        })
        .expect("the paid native step ladder completes");
    assert!(sim.pending_command_snapshot().is_empty());
    sim.session.tick += 1;
    sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
    super::publish_production_changes(&mut sim, &rules);
    let entity = sim.substrate.entities.get(held).unwrap();
    assert!(
        entity.lifecycle.in_limbo,
        "Strip emits PLACE before Unlimbo"
    );
    assert!(!entity.in_logic_vector);
    let factory = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    assert_eq!(factory.object.unwrap().entity_id, Some(held));
    assert_eq!(factory.queue.len(), 1, "StartNextQueued follows PLACE");
    let pending = sim.pending_command_snapshot();
    assert_eq!(pending.len(), 1);
    // The next frame's prefix appends PLACE for its own event tail, after
    // already accepted player events and before that frame's Factory sweep.
    assert_eq!(pending[0].execute_tick, u64::from(completed_frame) + 2);
    super::publish_production_changes(&mut sim, &rules);
    assert_eq!(sim.pending_command_snapshot().len(), 1);
}

/// The live app records the Strip-augmented batch; playback regenerates the
/// same completion and must consume its recorded PLACE exactly once. The
/// completion itself is a supplied ready prior here, covered by the paid chain.
#[test]
fn completion_prefix_records_one_place_and_playback_consumes_its_copy() {
    let rules = factory_constructor_rules();
    let make = || {
        let mut sim = Simulation::with_seed(0xFAC7_0003);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        spawn_structure(&mut sim, 1, "Americans", "GAPILE", 14, 14);
        let owner = sim.interner.get("Americans").unwrap();
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
        assert!(
            sim.production
                .factory_shadow
                .test_arm_ready(owner, ProductionCategory::Infantry)
        );
        // This unrelated future event belongs to the scheduler throughout both
        // frames; admitting the prefix must not drain or retime it.
        let future = crate::sim::command::CommandEnvelope::new(
            owner,
            9,
            crate::sim::command::Command::Stop { entity_id: 999 },
        );
        sim.queue_command(future.clone());
        (sim, owner, future)
    };
    let (mut live, owner, future) = make();
    let held = live
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    let rally = crate::sim::command::CommandEnvelope::new(
        owner,
        1,
        crate::sim::command::Command::SetRally {
            rx: 20,
            ry: 20,
            producer_ids: vec![1],
        },
    );
    let mut output = live
        .advance_app_frame(
            std::slice::from_ref(&rally),
            Some(&rules),
            None,
            67,
            crate::sim::world::TickLane::Ordinary,
            None,
        )
        .unwrap();
    let admitted = output.take_admitted_commands();
    assert_eq!(
        admitted,
        [
            rally,
            crate::sim::command::CommandEnvelope::new(
                owner,
                1,
                crate::sim::command::Command::PlaceProducedMobile {
                    category: ProductionCategory::Infantry,
                },
            )
        ]
    );
    assert!(
        !live
            .substrate
            .entities
            .get(held)
            .unwrap()
            .lifecycle
            .in_limbo
    );
    assert_eq!(
        live.substrate.entities.get(held).unwrap().archive_target(),
        Some(crate::sim::combat::TargetKind::Cell(20, 20))
    );
    let next = live
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    assert!(next.queue.is_empty());
    let successor = next.object.unwrap().entity_id.unwrap();
    assert!(
        live.substrate
            .entities
            .get(successor)
            .unwrap()
            .lifecycle
            .in_limbo
    );
    assert_eq!(live.pending_command_snapshot(), [future.clone()]);

    let (mut replay, _, _) = make();
    let due = replay.take_due_replay_commands(admitted.clone());
    let mut replay_output = replay
        .advance_app_frame(
            &due,
            Some(&rules),
            None,
            67,
            crate::sim::world::TickLane::Ordinary,
            None,
        )
        .unwrap();
    assert_eq!(replay_output.take_admitted_commands(), admitted);
    assert_eq!(replay_output.tick.state_hash, output.tick.state_hash);
    assert_eq!(replay.pending_command_snapshot(), [future]);
    assert_eq!(
        replay
            .production
            .factory_shadow
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .object
            .unwrap()
            .entity_id,
        Some(successor)
    );
}

#[test]
fn factory_constructor_start_cancel_and_promotion_own_scenario_words() {
    let seed = 0xFAC7_0001;
    let rules = factory_constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 50_000, 10),
    );
    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAWEAP", 14, 10);

    let mut expected = SimRng::new(seed);
    let mtnk_word = (expected.next_u32() & 0xFFFF) as u16;
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "MTNK"));
    let vehicle = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .and_then(|view| view.object.cloned())
        .expect("vehicle constructed at StartProduction");
    let vehicle_id = vehicle.entity_id.expect("Factory+0x58 identity");
    let vehicle_entity = sim.substrate.entities.get(vehicle_id).unwrap();
    assert!(vehicle_entity.lifecycle.in_limbo);
    assert!(!vehicle_entity.lifecycle.cell_marked);
    assert!(!vehicle_entity.in_logic_vector);
    assert!(!vehicle_entity.in_playfield);
    assert_eq!(vehicle_entity.techno_ctor_random_word, mtnk_word);

    let e1_word = (expected.next_u32() & 0xFFFF) as u16;
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
    let infantry = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .and_then(|view| view.object.cloned())
        .expect("infantry constructed at StartProduction");
    let e1_id = infantry.entity_id.expect("Factory+0x58 identity");
    let e1_entity = sim.substrate.entities.get(e1_id).unwrap();
    assert!(e1_entity.lifecycle.in_limbo);
    assert!(!e1_entity.lifecycle.cell_marked);
    assert!(!e1_entity.in_logic_vector);
    assert!(!e1_entity.in_playfield);
    assert_eq!(e1_entity.techno_ctor_random_word, e1_word);

    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E2"));
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "a queued tail has not reached StartProduction and must not construct"
    );

    let e2_word = (expected.next_u32() & 0xFFFF) as u16;
    assert!(cancel_by_type_for_owner(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        false
    ));
    assert!(sim.substrate.entities.get(e1_id).is_none());
    let promoted = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .and_then(|view| view.object.cloned())
        .expect("E2 promoted through StartNextQueued");
    let e2_id = promoted.entity_id.expect("promoted Factory+0x58 identity");
    assert_eq!(promoted.type_id, sim.interner.get("E2").unwrap());
    assert_eq!(
        sim.substrate
            .entities
            .get(e2_id)
            .unwrap()
            .techno_ctor_random_word,
        e2_word
    );
    assert!(sim.substrate.entities.get(vehicle_id).is_some());
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

/// Original Factory4C9C70 constructs GAPOWR750/sample544=0 in limbo.
/// Abandon4C9FF0 -> scalar459F20 -> Building43BCF0 runs under A8E7AC=1;
/// 43BF11 dirties power alone, with no UnInit/loss/deferred deletion or RNG.
/// Source: building_death_anims constructed-cancel original controls,
/// active gamemd SHA1cdd1180e49024fbda8ad568caac2e86e.
#[test]
fn cancelled_constructor_building_runs_shared_destructor_without_uninit() {
    use crate::sim::power_system::PowerState;
    use serde_json::{Value, json};

    // Retained physical scenario layers, parsed by the production reader.
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_death_anims_limbo_cancel.json",
    ))
    .unwrap();
    let mut ini = IniFile::from_str("[BuildingTypes]\n0=GAPOWR\n[VehicleTypes]\n0=MTNK\n");
    for layer in corpus["death_layers"].as_array().unwrap() {
        if let Some(sections) = layer["physical_input_sections"].as_object() {
            let mut text = String::new();
            for (section, keys) in sections {
                if let Some(keys) = keys.as_object() {
                    text.push_str(&format!("[{section}]\n"));
                    for (key, value) in keys {
                        text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
                    }
                }
            }
            ini.merge(&IniFile::from_str(&text));
        }
    }
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(corpus["controls"].as_array().unwrap().len(), 5);
    for control in corpus["controls"].as_array().unwrap() {
        let cancellation = control["boundaries"].as_array().unwrap().last().unwrap();
        let before = &cancellation["before"];
        let after = &cancellation["after"];
        let mut sim = Simulation::with_seed(control["input"]["seed"].as_u64().unwrap());
        let owner = sim.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(
                owner,
                0,
                None,
                control["input"]["owner_human"].as_bool().unwrap(),
                5000,
                10,
            ),
        );
        arm_build_via(
            &mut sim,
            &rules,
            "Americans",
            "GAPOWR",
            ProductionCategory::Building,
            0,
        );
        let object = sim
            .production
            .factory_shadow
            .view(owner, ProductionCategory::Building)
            .unwrap()
            .object
            .unwrap()
            .entity_id
            .unwrap();
        let building = sim.substrate.entities.get(object).unwrap();
        let ctor = control["boundaries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|boundary| boundary["label"] == "held_ctor_complete_gapowr")
            .unwrap();
        assert_eq!(
            i64::from(building.health.current),
            ctor["state"]["building"]["health"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(building.building_power_health_sample().unwrap()),
            ctor["state"]["building"]["sampled_health"]
                .as_i64()
                .unwrap()
        );
        assert!(building.lifecycle.in_limbo && !building.lifecycle.cell_marked);
        assert!(!building.in_logic_vector);
        if control["input"]["equal_sample_control"] == true {
            // Same isolated health-sampling rung as the native equality control;
            // a held product receives no normal AI turn.
            sim.sample_building_health_for_house_update(object);
        }
        let observer = if control["input"]["targeted_observer_control"] == true {
            // Explicit callback prior, not player acquisition of a limbo target.
            // Only matching Target, active45 timer and constructor Techno state
            // are required by the compared expiry arm; Unlimbo is not compared.
            let observer = sim
                .construct_object_limbo_at_height("MTNK", "Americans", 10, 11, 0, 0, &rules)
                .unwrap();
            sim.assign_target_represented(
                observer,
                Some(crate::sim::combat::TargetKind::Entity(object)),
                Some(&rules),
            )
            .unwrap();
            Some(observer)
        } else {
            None
        };
        assert_eq!(
            i64::from(
                sim.substrate
                    .entities
                    .get(object)
                    .unwrap()
                    .building_power_health_sample()
                    .unwrap()
            ),
            before["building"]["sampled_health"].as_i64().unwrap()
        );
        let mut state = serde_json::to_value(PowerState::default()).unwrap();
        state["power_dirty"] = json!(before["house"]["dirty"][0] == 1);
        state["radar_dirty"] = json!(before["house"]["dirty"][1] == 1);
        sim.power_states
            .insert(owner, serde_json::from_value(state).unwrap());
        // The native reader fixture also has an admitted plant and MTNK. Adopt
        // its declared pre-cancel full cursors; their construction is outside
        // this selected-field cancellation comparison.
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(before["rng"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(before["rng"]["main"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(before["rng"]["mapgen"].as_str().unwrap());
        sim.session.binary_frame = before["frame"].as_u64().unwrap() as u32;

        assert!(cancel_by_type_for_owner(
            &mut sim,
            &rules,
            "Americans",
            "GAPOWR",
            false
        ));
        assert!(sim.substrate.entities.get(object).is_none());
        let state = serde_json::to_value(&sim.power_states[&owner]).unwrap();
        assert_eq!(
            state["power_dirty"],
            after["house"]["dirty"][0] == 1,
            "43BF11 unsampled/equal ctor health"
        );
        assert_eq!(
            state["radar_dirty"],
            after["house"]["dirty"][1] == 1,
            "destructor preserves radar dirty"
        );
        assert_eq!(sim.houses[&owner].tracking.buildings(), 0);
        assert_eq!(sim.houses[&owner].stats.units_lost(), 0);
        assert_eq!(sim.houses[&owner].stats.buildings_lost(), 0);
        assert!(sim.substrate.pending_delete.is_empty());
        assert!(!sim.object_placement_scope_active());
        for (name, rng) in [
            ("scenario", &sim.scenario_rng),
            ("main", &sim.main_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                rng.native_state_hex(),
                after["rng"][name].as_str().unwrap(),
                "cancel full {name} state"
            );
        }
        if let Some(observer) = observer {
            let actor = sim.substrate.entities.get(observer).unwrap();
            assert!(actor.attack_target.is_none(), "43BD67 expiry clears Target");
            assert_eq!(
                [
                    i64::from(actor.passive_scan_timer.start_frame),
                    i64::from(actor.passive_scan_timer.duration)
                ],
                [
                    after["observer"]["passive_scan_timer"][0].as_i64().unwrap(),
                    after["observer"]["passive_scan_timer"][2].as_i64().unwrap()
                ],
                "A8E7AC preserves the active passive scan timer"
            );
        }
        assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::ObjectSoundReleased { owner } if *owner == object)));
    }
}

/// `HouseClass::CanBuild @ 0x004F7870` caps a type's TechLevel at the
/// house's own (`HouseClass+0x1D4`), which the match options set.
#[test]
fn a_house_builds_only_up_to_its_own_tech_level() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans = sim.interner.intern("Americans");
    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    let e1 = sim.interner.intern("E1");
    // An UnbuildableTechLevel option is hidden from the sidebar list.
    for (house_tech_level, listed_enabled) in [(0, None), (1, Some(true))] {
        sim.houses.insert(
            americans,
            crate::sim::house_state::HouseState::new(
                americans,
                0,
                None,
                true,
                50_000,
                house_tech_level,
            ),
        );
        let options = build_options_for_owner(&sim, &rules, "Americans");
        assert_eq!(
            options
                .iter()
                .find(|option| option.type_id == e1)
                .map(|option| option.enabled),
            listed_enabled,
            "house TechLevel {house_tech_level}"
        );
    }
}

#[test]
fn build_catalog_exposes_sidebar_categories_and_required_houses() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    // Pre-intern all rule type IDs so build_options_for_owner can resolve them.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    for (side, name) in [(0, "Americans"), (0, "Alliance")] {
        let id = sim.interner.intern(name);
        sim.houses.insert(
            id,
            crate::sim::house_state::HouseState::new(id, side, None, true, 50_000, 10),
        );
    }
    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAWEAP", 12, 10);
    spawn_structure(&mut sim, 3, "Americans", "GAAIRC", 14, 10);
    spawn_structure(&mut sim, 4, "Americans", "GACNST", 16, 10);
    spawn_structure(&mut sim, 5, "Alliance", "GAPILE", 20, 10);
    spawn_structure(&mut sim, 6, "Alliance", "GAWEAP", 22, 10);
    spawn_structure(&mut sim, 7, "Alliance", "GAAIRC", 24, 10);
    spawn_structure(&mut sim, 8, "Alliance", "GACNST", 26, 10);

    let americans = build_options_for_owner(&sim, &rules, "Americans");
    let alliance = build_options_for_owner(&sim, &rules, "Alliance");

    // Americans should see all items (they satisfy RequiredHouses for GTUR).
    assert_eq!(
        americans
            .iter()
            .map(|opt| opt.queue_category)
            .collect::<Vec<_>>(),
        vec![
            ProductionCategory::Building,
            ProductionCategory::Building,
            ProductionCategory::Defense,
            ProductionCategory::Infantry,
            ProductionCategory::Vehicle,
            ProductionCategory::Vehicle,
            ProductionCategory::Aircraft,
        ]
    );
    assert!(
        americans
            .iter()
            .filter(|opt| {
                matches!(
                    opt.queue_category,
                    ProductionCategory::Infantry
                        | ProductionCategory::Vehicle
                        | ProductionCategory::Aircraft
                )
            })
            .all(|opt| opt.enabled)
    );

    // Alliance should NOT see GTUR — it has RequiredHouses=Americans,
    // so it's hidden (not just greyed out) per RA2 behavior.
    assert!(
        alliance
            .iter()
            .find(|opt| opt.type_id == sim.interner.intern("GTUR"))
            .is_none(),
        "GTUR should be hidden for Alliance (RequiredHouses=Americans)"
    );

    let americans_yard = americans
        .iter()
        .find(|opt| opt.type_id == sim.interner.intern("GACNST"))
        .expect("construction yard should be listed");
    assert!(americans_yard.enabled);
    assert_eq!(americans_yard.reason, None);

    let americans_turret = americans
        .iter()
        .find(|opt| opt.type_id == sim.interner.intern("GTUR"))
        .expect("defense should be listed for Americans");
    assert!(americans_turret.enabled);
    assert_eq!(americans_turret.reason, None);
}

#[test]
fn named_skirmish_owner_uses_country_for_build_permissions() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let owner_id = sim.interner.intern("Commander");
    let country_id = sim.interner.intern("Americans");
    sim.houses.insert(
        owner_id,
        crate::sim::house_state::HouseState::new(
            owner_id,
            0,
            Some(country_id),
            true,
            super::production_types::STARTING_CREDITS,
            10,
        ),
    );
    spawn_structure(&mut sim, 1, "Commander", "GACNST", 10, 10);

    let options = build_options_for_owner(&sim, &rules, "Commander");
    let yard = options
        .iter()
        .find(|opt| opt.type_id == sim.interner.intern("GACNST"))
        .expect("country-matched owner should see Allied construction options");
    assert!(yard.enabled);

    let turret = options
        .iter()
        .find(|opt| opt.type_id == sim.interner.intern("GTUR"))
        .expect("RequiredHouses=Americans should match the house country");
    assert!(turret.enabled);
}

#[test]
#[ignore = "WIP: MCV deploy build-option unlock not yet landed"]
fn deployed_mcv_unlocks_building_options_for_named_skirmish_owner() {
    let mut sim = Simulation::new();
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=AMCV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAPOWR\n\
         [AMCV]\n\
         Strength=450\n\
         Armor=heavy\n\
         Speed=5\n\
         TechLevel=1\n\
         Owner=Americans\n\
         DeploysInto=GACNST\n\
         [GACNST]\n\
         Name=Construction Yard\n\
         Cost=3000\n\
         Strength=1000\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Foundation=4x3\n\
         Factory=BuildingType\n\
         [GAPOWR]\n\
         Name=Power Plant\n\
         Cost=800\n\
         Strength=750\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans\n\
         Foundation=2x2\n\
         Prerequisite=GACNST\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules should parse");
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let owner_id = sim.interner.intern("Commander");
    let country_id = sim.interner.intern("Americans");
    sim.houses.insert(
        owner_id,
        crate::sim::house_state::HouseState::new(
            owner_id,
            0,
            Some(country_id),
            true,
            super::production_types::STARTING_CREDITS,
            10,
        ),
    );
    let mcv = sim
        .spawn_object("AMCV", "Commander", 20, 22, 64, &rules)
        .expect("MCV should spawn");
    assert!(sim.deploy_mcv(mcv, &rules, None));

    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    }

    let options = build_options_for_owner(&sim, &rules, "Commander");
    let power = options
        .iter()
        .find(|opt| opt.type_id == sim.interner.intern("GAPOWR"))
        .expect("deployed yard should unlock first building");
    assert!(power.enabled);
    assert_eq!(power.reason, None);
}

/// `Time_To_Build` reads the owner's power (House `+0x53A4`/`+0x53A8`) and the
/// owner's factory count for the object's class (`0x00500910`).
#[test]
fn build_time_inputs_read_owner_power_and_matching_factories() {
    let mut sim = Simulation::new();
    let rules = production_modifier_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAPILE", 12, 10);
    spawn_structure(&mut sim, 3, "Americans", "GAWEAP", 14, 10);
    spawn_structure(&mut sim, 4, "Soviet", "NAHAND", 20, 20);
    spawn_structure(&mut sim, 5, "Soviet", "GAPOWR", 22, 20);
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        &rules,
        &sim.interner,
        sim.session.binary_frame,
    );

    let americans = sim.interner.intern("Americans");
    let soviet = sim.interner.intern("Soviet");
    let e1 = rules.object("E1").expect("E1");
    let mtnk = rules.object("MTNK").expect("MTNK");
    let inputs = |owner, category, obj| {
        super::factory::time_to_build_inputs(&sim, &rules, owner, category, obj)
    };

    let americans_infantry = inputs(americans, ProductionCategory::Infantry, e1);
    assert_eq!(
        (
            americans_infantry.power_output,
            americans_infantry.power_drain
        ),
        (0, 60)
    );
    assert_eq!(americans_infantry.factory_count, 2);
    assert_eq!(
        inputs(americans, ProductionCategory::Vehicle, mtnk).factory_count,
        1
    );
    let soviet_infantry = inputs(soviet, ProductionCategory::Infantry, e1);
    assert_eq!(
        (soviet_infantry.power_output, soviet_infantry.power_drain),
        (200, 20)
    );
    assert_eq!(soviet_infantry.factory_count, 1);
    assert!(!soviet_infantry.wall);

    // A factory counts from its Unlimbo (`0x00440D13`), so one still playing
    // its build-up animation counts.
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .install_building_up(
            crate::sim::components::BuildingUp::completing_in_ticks(30, 0),
            0,
        );
    assert_eq!(
        super::factory::time_to_build_inputs(
            &sim,
            &rules,
            americans,
            ProductionCategory::Infantry,
            e1
        )
        .factory_count,
        2
    );
}

/// A `Wall=yes` building takes `WallBuildSpeedCoefficient=` (Time_To_Build's
/// branch on BuildingType `+0x1571`, `0x006F4929..0x006F493D`); every building
/// factory counts.
#[test]
fn wall_build_time_inputs_carry_the_wall_coefficient() {
    let mut sim = Simulation::new();
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[General]\n\
             BuildSpeed=1.0\n\
             MultipleFactory=0.8\n\
             WallBuildSpeedCoefficient=0.5\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GACNST\n\
             1=NACNST\n\
             2=GAWALL\n\
             [GACNST]\n\
             Factory=BuildingType\n\
             Owner=Americans\n\
             [NACNST]\n\
             Factory=BuildingType\n\
             Owner=Americans\n\
             [GAWALL]\n\
             Name=Wall\n\
             Cost=1000\n\
             Strength=100\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans\n\
             Wall=yes\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("wall rules should parse");

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "NACNST", 12, 10);

    let americans = sim.interner.intern("Americans");
    let wall = rules.object("GAWALL").expect("wall should exist");
    let inputs = super::factory::time_to_build_inputs(
        &sim,
        &rules,
        americans,
        ProductionCategory::Building,
        wall,
    );
    assert!(inputs.wall);
    assert_eq!(inputs.factory_count, 2);
    assert_eq!(
        inputs.wall_coefficient,
        crate::util::native_x87::NativeF64Bits::HALF
    );
    let tower = rules.object("GACNST").expect("GACNST");
    assert!(
        !super::factory::time_to_build_inputs(
            &sim,
            &rules,
            americans,
            ProductionCategory::Building,
            tower,
        )
        .wall
    );
}

#[test]
fn naval_unit_rally_uses_water_pathing_after_spawn() {
    let mut sim = Simulation::new();
    let rules = naval_production_rules();
    let terrain = water_terrain(32, 32);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain.clone());
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.terrain_costs.insert(
        SpeedType::Float,
        TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Float),
    );
    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 20, 20);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_archive_target(Some(crate::sim::combat::TargetKind::Cell(26, 21)));
    // spawn_structure supplied the human House and registered this yard in
    // its native base vector. Keep that identity/membership; only supply funds.
    let americans_display = sim.interner.get("Americans").unwrap();
    *super::credits_entry_for_owner(&mut sim, "Americans") =
        super::production_types::STARTING_CREDITS;
    assert!(sim.houses[&americans_display].is_human);
    assert_eq!(
        sim.houses[&americans_display].base_projection.buildings(),
        &[1],
        "the supplied producer remains in the canonical House base vector"
    );
    // Arm the native Ship-slot factory directly in the registry, then force it
    // ready so publication queues the destroyer's next-frame PLACE.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        0,
    );
    let held_id = sim
        .production
        .factory_shadow
        .view(americans_display, ProductionCategory::Ship)
        .and_then(|view| view.object.and_then(|object| object.entity_id))
        .expect("ship exists in limbo from StartProduction");
    let rng_before_delivery = sim.scenario_rng.logical_state();
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_display, ProductionCategory::Ship)
    );

    let spawned = dispatch_production_changes_for_tests(&mut sim, &rules, None);
    assert!(spawned, "completed naval production should spawn the unit");
    assert_eq!(sim.scenario_rng.logical_state(), rng_before_delivery);

    let ship = sim
        .substrate
        .entities
        .values()
        .find(|e| {
            sim.interner
                .resolve(e.type_ref)
                .eq_ignore_ascii_case("DEST")
        })
        .expect("spawned destroyer");
    assert_eq!(
        ship.stable_id, held_id,
        "delivery reuses Factory+0x58 identity"
    );
    assert_eq!(
        ship.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(26, 21)),
        "the selected producer owns the represented rally destination"
    );
    assert_eq!(
        ship.mission.queued(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
        "the producer rally queues Move before the immediate-path adapter"
    );
    assert!(
        ship.movement_target.is_some(),
        "the clear water fixture should also build its optional immediate A* path"
    );
}

#[test]
fn build_options_dedupe_house_specific_sidebar_clone() {
    let mut sim = Simulation::new();
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAAIRC\n\
         2=AMRADR\n\
         [GACNST]\n\
         Name=Construction Yard\n\
         Cost=3000\n\
         Strength=1000\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans,Alliance\n\
         Image=GACNST\n\
         Factory=BuildingType\n\
         [GAAIRC]\n\
         Name=Airforce Command\n\
         Cost=1000\n\
         Strength=600\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans,Alliance,British,French,Germans,Koreans\n\
         Image=GAAIRC\n\
         BuildCat=Tech\n\
         [AMRADR]\n\
         Name=Airforce Command\n\
         Cost=1000\n\
         Strength=600\n\
         Armor=wood\n\
         TechLevel=1\n\
         Owner=Americans,Alliance,British,French,Germans,Koreans\n\
         RequiredHouses=Americans\n\
         Image=GAAIRC\n\
         BuildCat=Tech\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules should parse");
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let americans_id = sim.interner.intern("Americans");
    sim.houses.insert(
        americans_id,
        crate::sim::house_state::HouseState::new(americans_id, 0, None, true, 50_000, 10),
    );
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);

    let americans = build_options_for_owner(&sim, &rules, "Americans");
    let airforce: Vec<_> = americans
        .iter()
        .filter(|opt| opt.display_name == "Airforce Command")
        .collect();

    assert_eq!(
        airforce.len(),
        1,
        "sidebar should only show one Airforce Command"
    );
    assert_eq!(airforce[0].type_id, sim.interner.intern("AMRADR"));
}

#[test]
fn published_completions_dispatch_each_owners_next_frame_place() {
    let mut sim = Simulation::new();
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let rules = basic_infantry_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Soviet", "NAHAND", 20, 20);

    let americans_id = sim.interner.intern("Americans");
    let soviet_id = sim.interner.intern("Soviet");
    // P5d: arm both factories directly in the registry, then force both to the completed-and-
    // held state, then publish and dispatch their next-frame PLACE events.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Soviet",
        "E1",
        ProductionCategory::Infantry,
        1,
    );

    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Infantry)
    );
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(soviet_id, ProductionCategory::Infantry)
    );

    let spawned = dispatch_production_changes_for_tests(&mut sim, &rules, None);
    assert!(spawned, "At least one queue completion should spawn");
    assert!(
        sim.production.factory_shadow.is_empty(),
        "Completed owner factories should be pruned"
    );

    let americans = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.owner)
                .eq_ignore_ascii_case("Americans")
                && sim.interner.resolve(e.type_ref).eq_ignore_ascii_case("E1")
        })
        .count();
    let soviet = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner.resolve(e.owner).eq_ignore_ascii_case("Soviet")
                && sim.interner.resolve(e.type_ref).eq_ignore_ascii_case("E1")
        })
        .count();
    assert_eq!(americans, 1);
    assert_eq!(soviet, 1);
}

#[test]
fn published_completions_dispatch_multiple_categories_for_one_owner() {
    let mut sim = Simulation::new();
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let rules = basic_multi_queue_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAWEAP", 14, 10);

    let americans_id = sim.interner.intern("Americans");
    // P5d: arm both category factories directly in the registry, then force both to the
    // completed-and-held state, then dispatch their next-frame PLACE events.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        1,
    );

    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Infantry)
    );
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Vehicle)
    );

    let spawned = dispatch_production_changes_for_tests(&mut sim, &rules, None);
    assert!(spawned);
    assert!(
        sim.production.factory_shadow.is_empty(),
        "all completed category factories should be pruned"
    );

    let infantry = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.owner)
                .eq_ignore_ascii_case("Americans")
                && sim.interner.resolve(e.type_ref).eq_ignore_ascii_case("E1")
        })
        .count();
    let vehicles = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.owner)
                .eq_ignore_ascii_case("Americans")
                && sim
                    .interner
                    .resolve(e.type_ref)
                    .eq_ignore_ascii_case("MTNK")
        })
        .count();
    assert_eq!(infantry, 1);
    assert_eq!(vehicles, 1);
}

#[test]
fn blocked_vehicle_delivery_refunds_disposes_and_promotes_next_item() {
    let mut sim = Simulation::new();
    let rules = super::lifecycle_tests::manager_rules();
    let terrain = water_terrain(32, 32);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);

    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);
    *super::credits_entry_for_owner(&mut sim, "Americans") = 1000;

    let americans_id = sim.interner.intern("Americans");
    // P5d: arm the front MTNK (active) then a second MTNK at a higher stamp (FIFO tail).
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        1,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        2,
    );

    // Supply the active vehicle's completion before its next-frame delivery
    // (which the water grid blocks).
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Vehicle)
    );

    let held = super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle);
    let children = super::lifecycle_tests::children(&sim, held);
    assert_eq!(children.len(), 3);
    let owned = sim.owned_object_counts(americans_id).1;
    let mut expected = sim.scenario_rng.clone();
    let main = sim.main_rng.logical_state();
    let mapgen = sim.mapgen_rng.logical_state();
    // test_arm_ready supplies paid Balance0; the wallet above is its supplied
    // post-payment balance. HousePlace4FB0E0 refunds and abandons a present producer's refused exit,
    // then StartNextQueued constructs the next graph (4FB587..4FB62A).
    assert!(!dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_eq!(credits_for_owner(&sim, "Americans"), 1700);
    for id in std::iter::once(held).chain(children) {
        assert!(!sim.substrate.entities.contains(id));
    }
    let successor =
        super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle);
    assert!(successor > held);
    let factory = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(factory.progress, 0);
    assert!(!factory.ready && factory.queue.is_empty());
    let object = sim.substrate.entities.get(successor).unwrap();
    assert!(object.lifecycle.in_limbo && !object.lifecycle.cell_marked);
    super::lifecycle_tests::assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(sim.main_rng.logical_state(), main);
    assert_eq!(sim.mapgen_rng.logical_state(), mapgen);
    assert_eq!(sim.owned_object_counts(americans_id).1, owned);
    let projected = queue_view_for_owner(&sim, &rules, "Americans");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].state, BuildQueueState::Building);
    assert_eq!(projected[0].progress, 0);
    let allocated = sim.substrate.next_stable_object_id;
    for _ in 0..3 {
        assert!(!dispatch_production_changes_for_tests(
            &mut sim, &rules, None
        ));
    }
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle),
        successor
    );
    assert_eq!(sim.substrate.next_stable_object_id, allocated);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(credits_for_owner(&sim, "Americans"), 1700);
}

#[test]
fn failed_vehicle_exit_promotes_a_fresh_identity_that_can_deliver_after_cells_clear() {
    let mut sim = Simulation::new();
    let rules = super::lifecycle_tests::manager_rules();
    let mut terrain = water_terrain(32, 32);
    let blocked_grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&blocked_grid));
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -32,
        off_100: -1,
        off_104: 64,
        off_108: 33,
    });
    sim.resolved_terrain = Some(terrain.clone());

    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);

    let americans_id = sim.interner.intern("Americans");
    *super::credits_entry_for_owner(&mut sim, "Americans") = 50_000;
    // P5d: arm the front MTNK (active) then a second MTNK at a higher stamp (FIFO tail).
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        1,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        2,
    );

    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Vehicle)
    );
    // test_arm_ready supplies the paid Balance0/post-payment wallet prior.
    let refused = super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle);
    let children = super::lifecycle_tests::children(&sim, refused);
    assert_eq!(children.len(), 3);
    let owned = sim.owned_object_counts(americans_id).1;
    let mut expected = sim.scenario_rng.clone();
    assert!(!dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    for id in std::iter::once(refused).chain(children) {
        assert!(!sim.substrate.entities.contains(id));
    }
    let successor =
        super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle);
    assert!(successor > refused);
    super::lifecycle_tests::assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(sim.owned_object_counts(americans_id).1, owned);
    let factory = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(factory.progress, 0);
    assert!(factory.queue.is_empty());
    assert_eq!(credits_for_owner(&sim, "Americans"), 50_700);
    for cell in &mut terrain.cells {
        cell.is_water = false;
        cell.land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
        cell.yr_cell_land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
        cell.terrain_class = crate::rules::terrain_rules::TerrainClass::Clear;
        cell.speed_costs.track = Some(100);
        cell.zone_type = crate::map::resolved_terrain::zone_class::GROUND;
    }
    let clear_grid = PathGrid::from_resolved_terrain(&terrain);
    sim.resolved_terrain = Some(terrain);
    sim.path_grid = Some(std::sync::Arc::new(clear_grid));
    // The failed identity cannot retry. Supply completion of its newly started
    // successor and exercise that object's own next-frame PLACE edge.
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Vehicle)
    );
    assert!(dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let delivered = sim.substrate.entities.get(successor).unwrap();
    assert!(!delivered.lifecycle.in_limbo && delivered.lifecycle.cell_marked);
    assert_eq!(super::lifecycle_tests::children(&sim, successor).len(), 3);
    assert!(!sim.substrate.entities.contains(refused));
    assert_eq!(sim.owned_object_counts(americans_id).1, owned);
    assert!(
        sim.production
            .factory_shadow
            .view(americans_id, ProductionCategory::Vehicle)
            .is_none_or(|factory| factory.object.is_none())
    );
    assert!(queue_view_for_owner(&sim, &rules, "Americans").is_empty());
}

#[test]
fn paused_category_projection_and_factory_charge_remain_independent() {
    let mut sim = Simulation::new();
    let rules = basic_multi_queue_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAWEAP", 14, 10);

    let americans_id = sim.interner.intern("Americans");
    *super::credits_entry_for_owner(&mut sim, "Americans") = 50_000;
    // P5d: arm both category factories directly in the registry.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        1,
    );

    let paused = suspend_production(&mut sim, "Americans", ProductionCategory::Infantry);
    assert!(paused);

    // Run the actual frame owner, which now performs revalidation and charging.
    sim.houses
        .get_mut(&americans_id)
        .unwrap()
        .tracking
        .set_buildings_for_test(2);
    for _ in 0..40 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }

    // Project the registry to the sidebar view for state assertions (the per-item
    // `state`/`remaining_base_frames` mirror is retired).
    let view = queue_view_for_owner(&sim, &rules, "Americans");
    let infantry = view
        .iter()
        .find(|q| q.queue_category == ProductionCategory::Infantry)
        .expect("infantry factory should remain");
    let vehicle = view
        .iter()
        .find(|q| q.queue_category == ProductionCategory::Vehicle)
        .expect("vehicle factory should remain");

    assert_eq!(infantry.state, BuildQueueState::Paused);
    assert_eq!(vehicle.state, BuildQueueState::Building);
    let infantry = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Infantry)
        .unwrap();
    let vehicle = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(infantry.progress, 0);
    assert!(vehicle.progress > 0);
}

/// Canceling a finished building abandons the factory's object, refunding the cost
/// less the unpaid balance (Abandon_Production 0x004FAA10, refund at 0x004FABA6), and
/// drops the ready entry with it.
#[test]
fn cancel_by_type_removes_ready_building_and_refunds() {
    use super::cancel_by_type_for_owner;

    let mut sim = Simulation::new();
    let rules = build_catalog_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    // The refund goes to an existing house; a cancel never creates one.
    *super::credits_entry_for_owner(&mut sim, "Americans") = 5000;

    let americans_id = sim.interner.intern("Americans");
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "GAREFN",
        ProductionCategory::Building,
        0,
    );
    // The fixture finishes the build with its balance paid off.
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Building)
    );
    assert!(!dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_eq!(sim.production.ready_by_owner[&americans_id].len(), 1);

    let before_credits = credits_for_owner(&sim, "Americans");

    let cancelled = cancel_by_type_for_owner(&mut sim, &rules, "Americans", "GAREFN", false);
    assert!(cancelled, "should cancel ready building");

    // Ready queue should be empty now.
    let ready_count = sim
        .production
        .ready_by_owner
        .get(&americans_id)
        .map(|q| q.len())
        .unwrap_or(0);
    assert_eq!(ready_count, 0, "ready queue should be empty after cancel");
    assert!(
        sim.production
            .factory_shadow
            .view(americans_id, ProductionCategory::Building)
            .is_none()
    );

    // Cost should be refunded.
    let after_credits = credits_for_owner(&sim, "Americans");
    let refund = rules.object("GAREFN").map(|o| o.cost).unwrap_or(0);
    assert!(refund > 0, "GAREFN should have a cost");
    assert_eq!(after_credits, before_credits + refund);
}

/// A build starts without money: nothing on the PRODUCE path checks the wallet
/// (`HouseClass::Begin_Production @ 0x004FA350`), and the per-step charge in
/// `step_all` pays for the build as it goes. An empty wallet still starts the
/// build and is debited nothing.
#[test]
fn enqueue_starts_a_build_without_money_and_debits_nothing() {
    let mut sim = Simulation::new();
    let rules = basic_multi_queue_rules();
    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10); // a UnitType war factory
    *super::credits_entry_for_owner(&mut sim, "Americans") = 0;
    assert!(super::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "MTNK"
    ));
    assert_eq!(credits_for_owner(&sim, "Americans"), 0);
    let americans_id = sim.interner.intern("Americans");
    let mtnk_id = sim.interner.intern("MTNK");
    let factory = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Vehicle)
        .expect("the build starts a factory");
    assert_eq!(factory.object.map(|o| o.type_id), Some(mtnk_id));
    assert_eq!(factory.progress, 0);
}

fn hold_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[General]\nMaximumQueuedObjects=2\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n1=AMCV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWEAP\n\
         [MTNK]\nCost=700\nStrength=300\nSpeed=6\nTechLevel=1\nOwner=Americans\n\
         [AMCV]\nCost=3000\nStrength=1000\nSpeed=4\nTechLevel=1\nOwner=Americans\nBuildLimit=1\n\
         [GAWEAP]\nOwner=Americans\nFactory=UnitType\n",
    ))
    .expect("hold rules")
}

/// A funded American house with a war factory, for the hold and queue-cap tests.
fn hold_world() -> (Simulation, RuleSet, InternedId) {
    let rules = hold_rules();
    let mut sim = Simulation::new();
    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);
    *super::credits_entry_for_owner(&mut sim, "Americans") = 50_000;
    let owner = sim.interner.intern("Americans");
    (sim, rules, owner)
}

/// A PRODUCE resumes a held build even at its type's build limit: the held build
/// counts toward the limit, and `CanBuild(type, 1, 1)` passes a type one of the
/// house's factories holds (`0x004F8348`). The build start (`0x004C9EA0`)
/// restarts the rate from the resume frame and queues nothing.
#[test]
fn a_held_build_at_its_build_limit_resumes_on_produce() {
    let (mut sim, rules, owner) = hold_world();
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "AMCV"));
    assert!(suspend_production(
        &mut sim,
        "Americans",
        ProductionCategory::Vehicle
    ));
    // The factory is already stopped, so a second hold is refused (0x004C9E60).
    assert!(!suspend_production(
        &mut sim,
        "Americans",
        ProductionCategory::Vehicle
    ));
    assert_eq!(
        queue_view_for_owner(&sim, &rules, "Americans")[0].state,
        BuildQueueState::Paused
    );
    let option = super::production_tech::build_option_for_owner(&sim, &rules, "Americans", "AMCV")
        .expect("the type stays listed");
    assert_eq!(
        option.reason,
        Some(super::BuildDisabledReason::AtBuildLimit)
    );

    sim.session.binary_frame = 300;
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "AMCV"));
    let factory = sim
        .production
        .factory_shadow
        .test_factory_mut(owner, ProductionCategory::Vehicle)
        .unwrap();
    assert!(!factory.manual);
    assert!(factory.step_rate_frames > 0);
    assert_eq!(
        factory.step_timer,
        CdTimer::started(300, i32::from(factory.step_rate_frames))
    );
    assert!(factory.queue.is_empty(), "a resume queues nothing");
    assert_eq!(
        queue_view_for_owner(&sim, &rules, "Americans")[0].state,
        BuildQueueState::Building
    );
    // Running, the build no longer passes the limit: gamemd's StartProduction
    // refuses the append (0x004C9CEA), and VERA refuses it before that.
    assert!(!enqueue_by_type(&mut sim, &rules, "Americans", "AMCV"));
}

/// `[General] MaximumQueuedObjects=` caps the builds waiting behind the active
/// one: StartProduction refuses the append past it (`0x004C9CDE`) and scolds the
/// house's player (`0x004C9D3B..0x004C9D5F`).
#[test]
fn a_produce_past_the_queue_cap_is_refused_with_a_scold() {
    let (mut sim, rules, owner) = hold_world();
    assert_eq!(rules.general.maximum_queued_objects, 2);
    let refusals = |sim: &Simulation| {
        sim.sound_events
            .iter()
            .filter(|event| {
                matches!(event, SimSoundEvent::ProductionRefused { owner: refused } if *refused == owner)
            })
            .count()
    };
    for _ in 0..3 {
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", "MTNK"));
    }
    assert_eq!(refusals(&sim), 0);
    assert!(!enqueue_by_type(&mut sim, &rules, "Americans", "MTNK"));
    assert_eq!(refusals(&sim), 1);
    let factory = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(factory.queue.len(), 2);
}
