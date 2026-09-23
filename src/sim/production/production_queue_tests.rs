//! Production queue tests — verifies build queue ordering, credit deduction, prerequisite
//! checks, multi-factory speed bonus, and queue pause/resume behavior.

use std::collections::BTreeMap;

use super::{
    BuildQueueState, ProductionCategory, build_options_for_owner, cancel_by_type_for_owner,
    credits_for_owner, enqueue_by_type, queue_view_for_owner, tick_production,
    toggle_pause_for_owner_category,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;

// Re-use test helpers from the main production_tests module.
// P5d: build state is now constructed by ARMING the registry (`arm_build_via`), not by
// inserting `BuildQueueItem`s into the retired `queues_by_owner`.
use super::tests::{
    arm_build_via, basic_infantry_rules, basic_multi_queue_rules, build_catalog_rules,
    naval_production_rules, production_modifier_rules, spawn_structure, water_terrain,
};

fn factory_constructor_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=E2\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPILE\n1=GAWEAP\n\
         [E1]\nCost=200\nStrength=100\nSpeed=4\nTechLevel=1\nOwner=Americans\n\
         [E2]\nCost=300\nStrength=125\nSpeed=4\nTechLevel=1\nOwner=Americans\n\
         [MTNK]\nCost=700\nStrength=300\nSpeed=6\nTechLevel=1\nOwner=Americans\n\
         [GAPILE]\nFactory=InfantryType\n\
         [GAWEAP]\nFactory=UnitType\n",
    ))
    .expect("factory constructor rules")
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
        "E1"
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

#[test]
fn build_catalog_exposes_sidebar_categories_and_required_houses() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    // Pre-intern all rule type IDs so build_options_for_owner can resolve them.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

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
        "[InfantryTypes]\n\
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
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

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
        .spawn_object("AMCV", "Commander", 20, 22, 64, &rules, &height_map)
        .expect("MCV should spawn");
    assert!(sim.deploy_mcv(mcv, &rules, &height_map));

    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), &height_map, None, None, 33);
    }

    let options = build_options_for_owner(&sim, &rules, "Commander");
    let power = options
        .iter()
        .find(|opt| opt.type_id == sim.interner.intern("GAPOWR"))
        .expect("deployed yard should unlock first building");
    assert!(power.enabled);
    assert_eq!(power.reason, None);
}

#[test]
fn queue_view_uses_owner_power_modifier() {
    let mut sim = Simulation::new();
    let rules = production_modifier_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Soviet", "NAHAND", 20, 20);
    spawn_structure(&mut sim, 3, "Soviet", "GAPOWR", 22, 20);

    // Populate cached power states so the speed multiplier sees the deficit.
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        &rules,
        &sim.interner,
    );

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        900,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Soviet",
        "E1",
        ProductionCategory::Infantry,
        900,
        1,
    );

    let americans = queue_view_for_owner(&sim, &rules, "Americans");
    let soviet = queue_view_for_owner(&sim, &rules, "Soviet");

    assert_eq!(americans[0].total_ms, 118_800);
    assert_eq!(soviet[0].total_ms, 59_400);
}

#[test]
fn matching_factory_bonus_is_category_specific() {
    let mut sim = Simulation::new();
    let rules = production_modifier_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAPILE", 12, 10);
    spawn_structure(&mut sim, 3, "Americans", "GAWEAP", 14, 10);
    spawn_structure(&mut sim, 4, "Americans", "GAPOWR", 16, 10);

    let infantry_rate =
        super::effective_progress_rate_ppm_for_type(&sim, &rules, "Americans", "E1");
    let vehicle_rate =
        super::effective_progress_rate_ppm_for_type(&sim, &rules, "Americans", "MTNK");

    assert_eq!(infantry_rate, 1_250_000);
    assert_eq!(vehicle_rate, 1_000_000);
}

#[test]
fn base_build_frames_follow_ra2_cost_buildspeed_formula() {
    let rules = production_modifier_rules();
    let obj = rules.object("E1").expect("E1 should exist");

    assert_eq!(super::build_time_base_frames(&rules, obj), 900);
}

#[test]
fn wall_build_speed_coefficient_applies_after_factory_scaling() {
    let mut sim = Simulation::new();
    let ini = IniFile::from_str(
        "[General]\n\
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

    let wall = rules.object("GAWALL").expect("wall should exist");
    let base_frames = super::build_time_base_frames(&rules, wall);
    let total_frames = super::effective_time_to_build_frames_for_type(
        &sim,
        &rules,
        "Americans",
        "GAWALL",
        base_frames,
    );

    assert_eq!(base_frames, 900);
    assert_eq!(total_frames, 360);
}

#[test]
#[ignore = "retired (P5b): tick_production no longer advances a frames timer — the per-step \
            charge in the registry sweep drives progress, and the low-power/factory-bonus rate \
            now lives in build_step_time. Per-category rate differentiation is pinned by \
            matching_factory_bonus_is_category_specific."]
fn low_power_and_factory_bonus_apply_per_owner_and_category() {
    let mut sim = Simulation::new();
    let rules = production_modifier_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAPILE", 12, 10);
    spawn_structure(&mut sim, 3, "Americans", "GAWEAP", 14, 10);
    spawn_structure(&mut sim, 4, "Soviet", "NAHAND", 20, 20);
    spawn_structure(&mut sim, 5, "Soviet", "GAWEAP", 22, 20);
    spawn_structure(&mut sim, 6, "Soviet", "GAPOWR", 24, 20);

    // Populate cached power states so the speed multiplier sees the deficit.
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        &rules,
        &sim.interner,
    );

    let americans_id = sim.interner.intern("Americans");
    let soviet_id = sim.interner.intern("Soviet");
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        60_000,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Soviet",
        "MTNK",
        ProductionCategory::Vehicle,
        60_000,
        1,
    );

    let _ = tick_production(&mut sim, &rules, &height_map, None);

    // P5d: the per-item `remaining_base_frames` mirror is retired; mid-build state lives in
    // the registry as `progress`. The active build's remaining base frames are derived as
    // `active_total_base_frames * (54 - progress) / 54`.
    let remaining_frames_for =
        |sim: &Simulation, owner: crate::sim::intern::InternedId, cat: ProductionCategory| -> u32 {
            sim.production
                .factory_shadow
                .view(owner, cat)
                .map(|v| {
                    let steps_left =
                        super::factory::PRODUCTION_STEPS.saturating_sub(v.progress) as u64;
                    let total = 60_000u64;
                    ((total * steps_left) / super::factory::PRODUCTION_STEPS as u64) as u32
                })
                .expect("factory should still exist")
        };
    let americans_remaining =
        remaining_frames_for(&sim, americans_id, ProductionCategory::Infantry);
    let soviet_remaining = remaining_frames_for(&sim, soviet_id, ProductionCategory::Vehicle);

    // P5D-REVIEW: ignored/retired test. The old per-frame-timer remaining values (59_991 /
    // 59_985) cannot be reproduced — `tick_production` no longer advances a frames timer, so
    // progress stays 0 here and remaining stays the full total. Construction is translated so
    // the test compiles; the value assertions are documented as no longer applicable.
    assert_eq!(americans_remaining, 60_000);
    assert_eq!(soviet_remaining, 60_000);
}

#[test]
fn naval_unit_rally_uses_water_pathing_after_spawn() {
    let mut sim = Simulation::new();
    let rules = naval_production_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let terrain = water_terrain(32, 32);
    let grid = PathGrid::from_resolved_terrain(&terrain);
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
    sim.substrate.entities.get_mut(1).unwrap().rally_target = Some((26, 21));
    let americans_key = sim.interner.intern("AMERICANS");
    let americans_display = sim.interner.intern("Americans");
    sim.houses.insert(
        americans_key,
        crate::sim::house_state::HouseState::new(
            americans_display,
            0,
            None,
            true,
            super::production_types::STARTING_CREDITS,
            10,
        ),
    );
    if let Some(h) = sim.houses.get_mut(&americans_key) {
        h.rally_point = Some((26, 21));
    }
    // Arm the native Ship-slot factory directly in the registry, then force it
    // ready so `tick_production` spawns the destroyer this tick.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        100,
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

    let spawned = tick_production(&mut sim, &rules, &height_map, Some(&grid));
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
        "[InfantryTypes]\n\
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
fn tick_production_advances_each_owner_queue() {
    let mut sim = Simulation::new();
    let rules = basic_infantry_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Soviet", "NAHAND", 20, 20);

    let americans_id = sim.interner.intern("Americans");
    let soviet_id = sim.interner.intern("Soviet");
    // P5d: arm both factories directly in the registry, then force both to the completed-and-
    // held state so `tick_production` delivers/spawns from that state this tick.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        100,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Soviet",
        "E1",
        ProductionCategory::Infantry,
        100,
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

    let spawned = tick_production(&mut sim, &rules, &height_map, None);
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
fn tick_production_advances_multiple_queue_categories_for_same_owner() {
    let mut sim = Simulation::new();
    let rules = basic_multi_queue_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAWEAP", 14, 10);

    let americans_id = sim.interner.intern("Americans");
    // P5d: arm both category factories directly in the registry, then force both to the
    // completed-and-held state so `tick_production` delivers them this tick.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        ProductionCategory::Infantry,
        100,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        100,
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

    let spawned = tick_production(&mut sim, &rules, &height_map, None);
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
fn blocked_vehicle_delivery_keeps_completed_item_and_holds_next_queue_item() {
    let mut sim = Simulation::new();
    let rules = super::lifecycle_tests::manager_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let terrain = water_terrain(32, 32);
    let grid = PathGrid::from_resolved_terrain(&terrain);
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
        100,
        1,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        100,
        2,
    );

    // Force the active (front) vehicle to ready so `tick_production` attempts delivery
    // (which the water grid blocks).
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Vehicle)
    );

    let held_id_before =
        super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle);
    let children_before = super::lifecycle_tests::children(&sim, held_id_before);
    assert_eq!(children_before.len(), 3);
    let owned_before = sim.owned_object_counts(americans_id).1;
    let rng_before = sim.scenario_rng.clone();
    let allocated_before = sim.substrate.next_stable_object_id;
    let spawned = tick_production(&mut sim, &rules, &height_map, Some(&grid));
    assert!(
        !spawned,
        "blocked completed vehicle should not spawn or advance"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        1000,
        "blocked vehicle delivery is not a failed production refund"
    );

    // The completed-held active build + its untouched FIFO tail must both persist.
    let view = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Vehicle)
        .expect("vehicle factory should remain");
    // Active (head) is the completed-held MTNK; tail still holds the one queued MTNK
    // (head + tail == the old queue.len() of 2).
    assert!(
        view.object.is_some(),
        "completed-held active build must persist"
    );
    assert_eq!(view.queue.len(), 1, "one queued tail item must remain");
    // Head state is Done; its derived remaining base frames is 0 (progress == 54).
    let active_steps_left = super::factory::PRODUCTION_STEPS
        .saturating_sub(view.progress.min(super::factory::PRODUCTION_STEPS));
    assert_eq!(
        active_steps_left, 0,
        "head is complete -> 0 remaining base frames"
    );
    // Tail item is Queued, not started; its remaining base frames == its full total of 100.
    assert_eq!(
        view.queue[0].total_base_frames, 100,
        "next queued item must not start while completed vehicle is pending"
    );
    // The projected sidebar view shows the head as Done and the tail as Queued.
    let projected = queue_view_for_owner(&sim, &rules, "Americans");
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].state, BuildQueueState::Done);
    assert_eq!(projected[1].state, BuildQueueState::Queued);
    let held_id = view
        .object
        .and_then(|object| object.entity_id)
        .expect("blocked delivery retains the StartProduction identity");
    let held = sim.substrate.entities.get(held_id).unwrap();
    assert!(held.lifecycle.in_limbo && !held.lifecycle.cell_marked);
    assert_eq!(sim.interner.resolve(held.type_ref), "MTNK");
    for _ in 0..3 {
        assert!(!tick_production(&mut sim, &rules, &height_map, Some(&grid)));
    }
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle),
        held_id_before
    );
    assert_eq!(
        super::lifecycle_tests::children(&sim, held_id_before),
        children_before
    );
    assert_eq!(sim.scenario_rng.logical_state(), rng_before.logical_state());
    assert_eq!(sim.substrate.next_stable_object_id, allocated_before);
    assert_eq!(sim.owned_object_counts(americans_id).1, owned_before + 0);
}

#[test]
fn pending_vehicle_delivery_success_consumes_completed_item_and_starts_next_item() {
    let mut sim = Simulation::new();
    let rules = super::lifecycle_tests::manager_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let mut terrain = water_terrain(32, 32);
    let blocked_grid = PathGrid::from_resolved_terrain(&terrain);
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
        100,
        1,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        100,
        2,
    );

    // Force the front vehicle to ready once. The first (blocked-grid) delivery leaves the
    // registry untouched, so it stays ready for the later clear-grid delivery.
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans_id, ProductionCategory::Vehicle)
    );

    let held_id_before =
        super::lifecycle_tests::held_id(&sim, americans_id, ProductionCategory::Vehicle);
    let children_before = super::lifecycle_tests::children(&sim, held_id_before);
    assert_eq!(children_before.len(), 3);
    let owned_before = sim.owned_object_counts(americans_id).1;
    let rng_before = sim.scenario_rng.clone();
    let allocated_before = sim.substrate.next_stable_object_id;
    let blocked = tick_production(&mut sim, &rules, &height_map, Some(&blocked_grid));
    assert!(!blocked, "first delivery attempt should remain pending");

    assert_eq!(
        super::lifecycle_tests::children(&sim, held_id_before),
        children_before
    );
    assert_eq!(sim.scenario_rng.logical_state(), rng_before.logical_state());
    assert_eq!(sim.substrate.next_stable_object_id, allocated_before);
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

    let spawned = tick_production(&mut sim, &rules, &height_map, Some(&clear_grid));
    assert!(
        spawned,
        "later successful delivery should consume the pending completed vehicle"
    );

    let tanks = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            sim.interner
                .resolve(entity.type_ref)
                .eq_ignore_ascii_case("MTNK")
        })
        .count();
    assert_eq!(
        tanks, 2,
        "one delivered tank and one freshly promoted limbo tank exist"
    );

    // The delivered active build is cleared and the tail MTNK is promoted into the active
    // slot (one item left, now the active Building head with an empty tail).
    let view = sim
        .production
        .factory_shadow
        .view(americans_id, ProductionCategory::Vehicle)
        .expect("next queued item should have started");
    assert!(view.object.is_some(), "promoted MTNK is the active build");
    let promoted_id = view.object.unwrap().entity_id.unwrap();
    assert!(
        sim.substrate
            .entities
            .get(promoted_id)
            .is_some_and(|entity| entity.lifecycle.in_limbo)
    );
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .filter(|entity| {
                sim.interner
                    .resolve(entity.type_ref)
                    .eq_ignore_ascii_case("MTNK")
                    && !entity.lifecycle.in_limbo
            })
            .count(),
        1
    );
    assert!(view.queue.is_empty(), "the FIFO tail is now empty");
    // The promoted build has not been charged this tick (step_delay = 0 -> first charge next
    // tick), so progress is 0 and its remaining base frames == its full total of 100.
    assert_eq!(view.progress, 0);
    let steps_left = super::factory::PRODUCTION_STEPS
        .saturating_sub(view.progress.min(super::factory::PRODUCTION_STEPS));
    let remaining_base_frames =
        ((100u64 * steps_left as u64) / super::factory::PRODUCTION_STEPS as u64) as u32;
    assert_eq!(
        remaining_base_frames, 100,
        "successful delivery starts the next item without advancing its timer in this tick"
    );
    // The projected sidebar view shows the single promoted item as Building.
    let projected = queue_view_for_owner(&sim, &rules, "Americans");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].state, BuildQueueState::Building);
    assert!(
        !sim.substrate
            .entities
            .get(held_id_before)
            .unwrap()
            .lifecycle
            .in_limbo
    );
    assert_eq!(
        super::lifecycle_tests::children(&sim, held_id_before),
        children_before
    );
    let promoted_children = super::lifecycle_tests::children(&sim, promoted_id);
    assert_eq!(promoted_children.len(), 3);
    assert!(
        promoted_children
            .iter()
            .all(|id| !children_before.contains(id))
    );
    let mut expected = rng_before;
    for id in std::iter::once(promoted_id).chain(promoted_children) {
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .techno_ctor_random_word,
            (expected.next_u32() & 0xffff) as u16
        );
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(sim.owned_object_counts(americans_id).1, owned_before + 4);
}

#[test]
fn paused_category_projection_and_factory_charge_remain_independent() {
    let mut sim = Simulation::new();
    let rules = basic_multi_queue_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

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
        1000,
        0,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        1000,
        1,
    );

    let paused =
        toggle_pause_for_owner_category(&mut sim, "Americans", ProductionCategory::Infantry);
    assert!(paused);

    // Run the actual frame owner, which now performs revalidation and charging.
    sim.houses
        .get_mut(&americans_id)
        .unwrap()
        .tracking
        .set_buildings_for_test(2);
    for _ in 0..40 {
        sim.advance_tick(&[], Some(&rules), &height_map, None, None, 67);
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

#[test]
fn cancel_by_type_removes_ready_building_and_refunds() {
    use super::cancel_by_type_for_owner;

    let mut sim = Simulation::new();
    let rules = build_catalog_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);

    // Place a building in the ready queue (simulating completion).
    let americans_id = sim.interner.intern("Americans");
    let garefn_id = sim.interner.intern("GAREFN");
    sim.production
        .ready_by_owner
        .entry(americans_id)
        .or_default()
        .push_back(garefn_id);

    let before_credits = credits_for_owner(&sim, "Americans");

    let cancelled = cancel_by_type_for_owner(&mut sim, &rules, "Americans", "GAREFN");
    assert!(cancelled, "should cancel ready building");

    // Ready queue should be empty now.
    let ready_count = sim
        .production
        .ready_by_owner
        .get(&americans_id)
        .map(|q| q.len())
        .unwrap_or(0);
    assert_eq!(ready_count, 0, "ready queue should be empty after cancel");

    // Cost should be refunded.
    let after_credits = credits_for_owner(&sim, "Americans");
    let refund = rules.object("GAREFN").map(|o| o.cost).unwrap_or(0);
    assert!(refund > 0, "GAREFN should have a cost");
    assert_eq!(after_credits, before_credits + refund);
}

#[test]
fn cancel_by_type_prefers_build_queue_over_ready_queue() {
    use super::cancel_by_type_for_owner;

    let mut sim = Simulation::new();
    let rules = build_catalog_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);

    // Put GAREFN in both the build queue AND ready queue.
    let americans_id = sim.interner.intern("Americans");
    let garefn_id = sim.interner.intern("GAREFN");
    sim.production
        .ready_by_owner
        .entry(americans_id)
        .or_default()
        .push_back(garefn_id);
    // P5d: arm the active GAREFN build directly in the registry (the cancel authority). The
    // cancel then abandons this active build-queue copy first, before touching the ready queue.
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "GAREFN",
        ProductionCategory::Building,
        10000,
        0,
    );

    // First cancel should remove from build queue (not ready queue).
    let cancelled = cancel_by_type_for_owner(&mut sim, &rules, "Americans", "GAREFN");
    assert!(cancelled);

    // Ready queue should still have the item.
    let ready_count = sim
        .production
        .ready_by_owner
        .get(&americans_id)
        .map(|q| q.len())
        .unwrap_or(0);
    assert_eq!(ready_count, 1, "ready queue should still have the item");

    // Second cancel should remove from ready queue.
    let cancelled2 = cancel_by_type_for_owner(&mut sim, &rules, "Americans", "GAREFN");
    assert!(cancelled2);

    let ready_count2 = sim
        .production
        .ready_by_owner
        .get(&americans_id)
        .map(|q| q.len())
        .unwrap_or(0);
    assert_eq!(
        ready_count2, 0,
        "ready queue should be empty after second cancel"
    );
}

/// C3 (the charge flip): enqueuing an affordable item does NOT debit the wallet upfront
/// — the per-step `step_all` charge debits it over the build. The can-afford-to-START
/// affordability gate still permits the enqueue.
#[test]
fn no_upfront_charge_at_enqueue() {
    let mut sim = Simulation::new();
    let rules = basic_multi_queue_rules();
    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10); // a UnitType war factory
    *super::credits_entry_for_owner(&mut sim, "Americans") = 5000;
    let before = credits_for_owner(&sim, "Americans");
    let ok = super::enqueue_by_type(&mut sim, &rules, "Americans", "MTNK");
    assert!(
        ok,
        "an affordable MTNK is enqueuable (the affordability gate still permits START)"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        before,
        "enqueue does NOT debit upfront — the per-step charge does"
    );
}
