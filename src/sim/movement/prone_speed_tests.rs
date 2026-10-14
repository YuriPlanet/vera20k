//! Prone infantry speed integration tests.

use std::collections::BTreeMap;

use super::tick_movement_with_grids;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{test_intern, test_interner};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;

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
    rules.install_art_data(art);
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
        Health::new(100),
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
        &mut rng,
        0,
        0, // binary_frame (test)
        None,
        None,
        None,
        &TerrainSpeedConfig::default(),
        &mut interner,
        Some(&rules),
        &mut sounds,
        &mut lifecycle_requests,
    );

    assert!(lifecycle_requests.is_empty());
    // Foot speed comes from live E1 Speed=4 (10 leptons/frame), not the stale
    // 165-leptons/second request stamp. The Infantry override is local; the
    // Foot owner's GetCurrentSpeed stays unadjusted.
    assert_eq!(
        crate::sim::movement::owner_current_speed(
            entities.get(1).unwrap(),
            rules.object("E1"),
            rules.general.veteran_speed,
            &std::collections::BTreeMap::new(),
        ),
        10
    );

    entities.get(1).expect("entity exists").position.sub_x
}

#[test]
fn crawls_yes_prone_movement_uses_ceiling_two_thirds_speed() {
    // Infantry521D80: 10 - trunc(10/3) = 7, then the eastward paid Walk step.
    assert_eq!(advance_prone_mover(true), SimFixed::from_num(135));
}

#[test]
fn crawls_no_prone_movement_uses_speed_plus_half() {
    // Infantry521DBC: 10 + trunc(10/2) = 15.
    assert_eq!(advance_prone_mover(false), SimFixed::from_num(143));
}
