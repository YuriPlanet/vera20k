//! Prone infantry speed integration tests.

use std::collections::BTreeMap;

use super::locomotor::MovementLayer;
use super::tick_movement_with_grids;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{Health, MovementTarget};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{test_intern, test_interner};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

fn infantry_rules(crawls: bool) -> RuleSet {
    let rules_ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [E1]\nStrength=100\nArmor=flak\nSpeed=4\nImage=GI\n",
    );
    let mut rules = RuleSet::from_ini(&rules_ini).expect("rules parse");
    let art_ini = IniFile::from_str(&format!(
        "[GI]\nCrawls={}\n",
        if crawls { "yes" } else { "no" }
    ));
    let art = ArtRegistry::from_ini(&art_ini);
    rules.merge_art_data(&art);
    rules
}

fn prone_mover() -> GameEntity {
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        0,
        0,
        0,
        64,
        test_intern("Americans"),
        Health { current: 100 },
        test_intern("E1"),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    entity.position.sub_x = SimFixed::from_num(128);
    entity.position.sub_y = SimFixed::from_num(128);
    entity.infantry.as_mut().expect("infantry runtime").is_prone = true;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    // Isolate the prone budget on an already accepted straight head. Fresh
    // center placement consumes Scenario RNG and chooses a functional subcell.
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .set_step_head(Some(crate::sim::components::DriveCoord::cell(1, 0, 0)));
    entity.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(165),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    entity
}

fn advance_prone_mover(crawls: bool) -> SimFixed {
    let rules = infantry_rules(crawls);
    let mut entities = EntityStore::new();
    entities.insert(prone_mover());

    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut occupancy = OccupancyGrid::new();
    let mut sounds = Vec::new();
    let mut lifecycle_requests = Vec::new();
    let mut next_occupancy_enter_order = crate::sim::world::EnterOrderCounter::new();
    let terrain_costs: BTreeMap<SpeedType, TerrainCostGrid> = BTreeMap::new();

    tick_movement_with_grids(
        &mut entities,
        None,
        None,
        &terrain_costs,
        &Default::default(),
        &mut occupancy,
        &mut crate::sim::occupancy::CellOccupationGrid::new(),
        &mut crate::sim::occupancy::RawCellOccupationGrid::new(),
        &mut next_occupancy_enter_order,
        &mut rng,
        0,
        0, // binary_frame (test)
        None,
        None,
        None,
        &TerrainSpeedConfig::default(),
        SIM_ZERO,
        9,
        60,
        &mut interner,
        Some(&rules),
        &mut sounds,
        &mut lifecycle_requests,
    );

    assert!(lifecycle_requests.is_empty());

    entities.get(1).expect("entity exists").position.sub_x
}

fn expected_sub_x_after_one_frame(frame_budget: i32) -> SimFixed {
    SimFixed::from_num(128 + frame_budget)
}

#[test]
fn crawls_yes_prone_movement_uses_ceiling_two_thirds_speed() {
    // ceil(11 * 2/3) = 8 leptons for this frame.
    assert_eq!(advance_prone_mover(true), expected_sub_x_after_one_frame(8));
}

#[test]
fn crawls_no_prone_movement_uses_speed_plus_half() {
    // 11 + floor(11/2) = 16 leptons for this frame.
    assert_eq!(
        advance_prone_mover(false),
        expected_sub_x_after_one_frame(16)
    );
}
