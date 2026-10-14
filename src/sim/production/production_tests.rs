//! Production integration tests — end-to-end tests for build completion, unit spawning,
//! harvester auto-creation, and sell/undeploy flows through the full production pipeline.

use super::production_spawn::mark_war_factory_spawn_contact;
use super::{ProductionCategory, STARTING_CREDITS, credits_for_owner, is_matching_factory};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::miner::ResourceType;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

pub(super) fn basic_infantry_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAPILE\n\
             1=NAHAND\n\
             [E1]\n\
             Name=GI\n\
             Cost=200\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Soviet\n\
             [GAPILE]\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n\
             [NAHAND]\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n",
    );
    RuleSet::from_ini(&ini).expect("basic infantry rules should parse")
}

pub(super) fn basic_multi_queue_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAPILE\n\
             1=GAWEAP\n\
             [E1]\n\
             Name=GI\n\
             Cost=200\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [MTNK]\n\
             Name=Tank\n\
             Cost=700\n\
             Strength=300\n\
             Armor=heavy\n\
             Speed=6\n\
             Sight=6\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [GAPILE]\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n\
             [GAWEAP]\n\
             Owner=Americans,Soviet\n\
             Factory=UnitType\n",
    );
    RuleSet::from_ini(&ini).expect("basic multi queue rules should parse")
}

pub(super) fn production_modifier_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[General]\n\
             BuildSpeed=1.0\n\
             MultipleFactory=0.8\n\
             LowPowerPenaltyModifier=1.0\n\
             MinLowPowerProductionSpeed=0.5\n\
             MaxLowPowerProductionSpeed=0.9\n\
             [InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAPILE\n\
             1=NAHAND\n\
             2=GAWEAP\n\
             3=GAPOWR\n\
             [E1]\n\
             Name=GI\n\
             Cost=1000\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Soviet\n\
             [MTNK]\n\
             Name=Tank\n\
             Cost=1000\n\
             Strength=300\n\
             Armor=heavy\n\
             Speed=6\n\
             Sight=6\n\
             TechLevel=1\n\
             Owner=Americans,Soviet\n\
             [GAPILE]\n\
             Power=-20\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n\
             [NAHAND]\n\
             Power=-20\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n\
             [GAWEAP]\n\
             Power=-20\n\
             Owner=Americans,Soviet\n\
             Factory=UnitType\n\
             [GAPOWR]\n\
             Strength=1000\nPower=200\n",
    );
    RuleSet::from_ini(&ini).expect("production modifier rules should parse")
}

pub(super) fn build_catalog_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             1=HARV\n\
             [AircraftTypes]\n\
             0=ORCA\n\
             [BuildingTypes]\n\
             0=GACNST\n\
             1=GTUR\n\
             2=GAREFN\n\
             3=GAPILE\n\
             4=GAWEAP\n\
             5=GAAIRC\n\
             [E1]\n\
             Name=GI\n\
             Cost=200\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             [MTNK]\n\
             Name=Tank\n\
             Cost=900\n\
             Strength=300\n\
             Armor=heavy\n\
             Speed=6\n\
             Sight=6\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             [HARV]\n\
             Name=Harvester\n\
             Harvester=yes\n\
             Dock=GAREFN\n\
             Cost=1400\n\
             Strength=600\n\
             Armor=heavy\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             [ORCA]\n\
             Name=Orca\n\
             Cost=1200\n\
             Strength=250\n\
             Armor=light\n\
             Speed=12\n\
             Sight=8\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             Prerequisite=GAAIRC\n\
             [GACNST]\n\
             Name=Construction Yard\n\
             Cost=3000\n\
             Strength=1000\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             BuildCat=Tech\n\
             Factory=BuildingType\n\
             [GTUR]\n\
             Name=Guardian GI\n\
             Cost=600\n\
             Strength=400\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             RequiredHouses=Americans\n\
             BuildCat=Combat\n\
             [GAREFN]\n\
             Name=Ore Refinery\n\
             Cost=2000\n\
             Strength=900\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             BuildCat=Tech\n\
             Refinery=yes\nDockUnload=yes\n\
             FreeUnit=HARV\n\
             Foundation=3x3\n\
             [GAPILE]\n\
             TechLevel=-1\n\
             Owner=Americans,Alliance\n\
             Factory=InfantryType\n\
             [GAWEAP]\n\
             TechLevel=-1\n\
             Owner=Americans,Alliance\n\
             Factory=UnitType\n\
             [GAAIRC]\n\
             TechLevel=-1\n\
             Owner=Americans,Alliance\n\
             Factory=AircraftType\n[Clear]\nBuildable=yes\n",
    );
    RuleSet::from_ini(&ini).expect("build catalog rules should parse")
}

pub(super) fn naval_production_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=DEST\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GAYARD\n\
         [DEST]\n\
         Name=Destroyer\n\
         Cost=1000\n\
         Strength=600\n\
         Armor=heavy\n\
         Speed=6\n\
         ROT=5\n\
         Naval=yes\n\
         SpeedType=Float\n\
         MovementZone=Water\n\
         Locomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}\n\
         TechLevel=1\n\
         Owner=Americans\n\
         [GAYARD]\n\
         Name=Naval Yard\n\
         Strength=1500\n\
         Owner=Americans\n\
         Factory=UnitType\n\
         WeaponsFactory=yes\n\
         Naval=yes\n\
         Foundation=4x4\n\
         SpeedType=Float\n\
         WaterBound=yes\n\
         ExitCoord=512,256,0\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GAYARD]\nFoundation=4x4\n"),
    )
    .expect("naval production rules and ART should parse")
}

pub(super) fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    let mut cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            cells.push(ResolvedTerrainCell {
                land_type: 2,
                yr_cell_land_type: 2,
                terrain_class: crate::rules::terrain_rules::TerrainClass::Water,
                speed_costs: crate::rules::terrain_rules::SpeedCostProfile {
                    float: Some(100),
                    hover: Some(100),
                    ..crate::rules::terrain_rules::SpeedCostProfile::default()
                },
                is_water: true,
                zone_type: crate::map::resolved_terrain::zone_class::WATER,
                base_land_type: 2,
                base_yr_cell_land_type: 2,
                base_terrain_class: crate::rules::terrain_rules::TerrainClass::Water,
                base_speed_costs: crate::rules::terrain_rules::SpeedCostProfile {
                    float: Some(100),
                    hover: Some(100),
                    ..crate::rules::terrain_rules::SpeedCostProfile::default()
                },
                ..crate::map::resolved_terrain::test_flat_cell(x, y)
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

pub(super) fn placement_radius_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GACNST\n\
             1=GAPOWR\n\
             2=GAGAP\n\
             [GACNST]\nFactory=BuildingType\n\
             Name=Construction Yard\n\
             Cost=3000\n\
             Strength=1000\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans\n\
             Foundation=2x2\n\
             BaseNormal=yes\n\
             [GAPOWR]\n\
             Name=Power Plant\n\
             Cost=800\n\
             Strength=750\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans\n\
             Foundation=2x2\n\
             Adjacent=0\n\
             [GAGAP]\n\
             Name=Gap Generator\n\
             Cost=1000\n\
             Strength=900\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans\n\
             Foundation=2x2\n\
             BaseNormal=no\n\
             Adjacent=0\n[Clear]\nBuildable=yes\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str(
            "[GACNST]\nFoundation=2x2\n[GAPOWR]\nFoundation=2x2\n[GAGAP]\nFoundation=2x2\n",
        ),
    )
    .expect("placement radius rules and ART should parse")
}

pub(super) fn sell_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             0=E1\n\
             1=E2\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAPOWR\n\
             1=NAHAND\n\
             2=NABNKR\n\
             [E1]\n\
             Name=GI\n\
             Cost=200\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             Occupier=yes\n\
             Size=1\n\
             [E2]\n\
             Name=Conscript\n\
             Cost=100\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Russians,Soviet\n\
             [GAPOWR]\n\
             Name=Power Plant\n\
             Cost=800\n\
             Strength=750\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Americans,Alliance\n\
             Foundation=2x2\n\
             Crewed=yes\n\
             [NAHAND]\n\
             Name=Barracks\n\
             Cost=500\n\
             Strength=500\n\
             Armor=wood\n\
             TechLevel=1\n\
             Owner=Russians,Soviet\n\
             Foundation=2x2\n\
             Crewed=yes\n\
             [NABNKR]\n\
             Name=Battle Bunker\n\
             Cost=0\n\
             Strength=400\n\
             Armor=wood\n\
             Foundation=1x1\n\
             CanBeOccupied=yes\n\
             CanOccupyFire=yes\n\
             MaxNumberOccupants=5\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str(
            "[GAPOWR]\nFoundation=2x2\n[NAHAND]\nFoundation=2x2\n[NABNKR]\nFoundation=1x1\n",
        ),
    )
    .expect("sell rules and ART should parse");
    // Each binds a Buildup (`GAPOWRMK`, `NAHANDMK`, `NABNKRMK`), so each sells.
    for type_id in ["GAPOWR", "NAHAND", "NABNKR"] {
        rules.set_buildup_control_for_test(type_id, [0, 25, 2]);
    }
    rules
}

/// Rules with Factory= keys for testing data-driven factory matching.
/// Includes standard RA2 factories plus custom modded ones (MYBARR, XAIRFLD).
pub(super) fn factory_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GACNST\n\
             1=GAPILE\n\
             2=GAWEAP\n\
             3=GAWEAT\n\
             4=GAAIRC\n\
             5=MYBARR\n\
             6=XAIRFLD\n\
             [GACNST]\n\
             Owner=Americans,Soviet\n\
             Factory=BuildingType\n\
             [MTNK]\n\
             Owner=Americans,Soviet\n\
             Name=Medium Tank\n\
             Strength=300\n\
             Armor=heavy\n\
             Speed=6\n\
             [GAPILE]\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n\
             Foundation=3x2\n\
             [GAWEAP]\n\
             Owner=Americans,Soviet\n\
             Factory=UnitType\n\
             WeaponsFactory=yes\n\
             Foundation=5x3\n\
             ExitCoord=512,256,0\n\
             [GAWEAT]\n\
             Owner=Americans,Soviet\n\
             Factory=UnitType\n\
             WeaponsFactory=yes\n\
             Foundation=5x3\n\
             ExitCoord=512,256,0\n\
             [GAAIRC]\n\
             Owner=Americans,Soviet\n\
             Factory=AircraftType\n\
             Foundation=3x2\n\
             ExitCoord=384,128,0\n\
             [MYBARR]\n\
             Owner=Americans,Soviet\n\
             Factory=InfantryType\n\
             ExitCoord=-64,64,0\n\
             Foundation=2x2\n\
             [XAIRFLD]\n\
             Owner=Americans,Soviet\n\
             Factory=AircraftType\n\
             ExitCoord=384,128,0\n",
    );
    let art = IniFile::from_str(
        "[GAPILE]\nFoundation=3x2\n[GAWEAP]\nFoundation=5x3\n\
         [GAWEAT]\nFoundation=5x3\n[GAAIRC]\nFoundation=3x2\n\
         [MYBARR]\nFoundation=2x2\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
        .expect("factory rules and ART should parse")
}

/// Explicit synthetic map prior for focused Infantry delivery tests. Native
/// GetDock44EFB0 requires physical cells and Map::InBounds568300, unlike the
/// retired category-only foundation-centre adapter.
pub(super) fn install_infantry_delivery_fixture_map(sim: &mut Simulation) {
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        64,
    ));
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 16,
        off_fc: -64,
        off_100: -64,
        off_104: 128,
        off_108: 128,
    });
    sim.playfield_size_height = Some(64);
    sim.session.map_width = 16;
    sim.session.map_height = 64;
    sim.session.game_mode_nonzero = true;
}

pub(super) fn spawn_structure(
    sim: &mut Simulation,
    rules: &RuleSet,
    sid: u64,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
) {
    let owner_id = sim.interner.intern(owner);
    let type_id_interned = sim.interner.intern(type_id);
    sim.houses.entry(owner_id).or_insert_with(|| {
        crate::sim::house_state::HouseState::new(owner_id, 0, None, true, STARTING_CREDITS, 10)
    });
    // LogicClass visits Houses through its constructor-ordered vector. Raw
    // placed-building fixtures must admit their House as well as its storage.
    if !sim.session.house_order.contains(&owner_id) {
        sim.session.house_order.push(owner_id);
    }
    let mut ge = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health::new(1000),
        type_id_interned,
        crate::map::entities::EntityCategory::Structure,
        0,
        5,
        false,
    );
    ge.lifecycle.set_in_limbo_for_test(false);
    ge.in_playfield = true;
    ge.finish_building_construction_for_test();
    ge.building_art.set_actually_placed_for_test(true);
    ge.tracking_facts = crate::sim::house_tracking::TrackingFacts::of(
        crate::map::entities::EntityCategory::Structure,
        rules.object(type_id),
        Some(rules),
    );
    sim.substrate.entities.insert(ge);
    // Use the lifecycle boundary so the fixture is a placed (not factory-held)
    // structure and raw-store consumers see the same Mark state as gameplay.
    sim.add_entity_occupancy(sid);
    sim.append_house_base_building_for_test(sid);
    // Its house counts it as tracked and on the map, as CanBuild's
    // prerequisites read them.
    sim.update_house_tracking(sid, crate::sim::house_tracking::HouseTracking::add_tracking);
    sim.house_added_to_game(sid);
    sim.update_house_tracking(sid, |tracking, entity| {
        tracking.increment_factory_count(entity)
    });
    if sim.substrate.next_stable_object_id <= sid {
        sim.substrate.next_stable_object_id = sid + 1;
    }
}

/// Give a [`spawn_structure`] fixture the foundation construction copies from
/// its type's `Foundation=` (`stamp_building_cell_profile`), which its
/// GetCoords reads. Its occupancy stays on the north-west cell.
pub(super) fn stamp_type_foundation(sim: &mut Simulation, rules: &RuleSet, sid: u64) {
    let entity = sim
        .substrate
        .entities
        .get(sid)
        .expect("a spawned structure");
    let foundation = rules
        .object(sim.interner.resolve(entity.type_ref()))
        .expect("the structure's type")
        .foundation
        .clone();
    sim.substrate
        .entities
        .get_mut(sid)
        .expect("a spawned structure")
        .type_facts
        .set_foundation_for_test(foundation);
}

/// P5d: arm a registry factory's queue-of-record directly (replaces the retired
/// `queues_by_owner` insert of a `BuildQueueItem`). Interns owner/type, resolves cost from
/// `rules`, and calls the registry `enqueue` (create-the-active-build, or append to the FIFO
/// tail if a build is already active for this `(owner, category)`). Use one call per item, in
/// enqueue order; `order` is the temporal stamp (the active build's `insertion_seq` / a tail
/// entry's `enqueue_order`). The `remaining`-frames concept is retired (progress lives in the
/// registry); set a build's progress afterward via `factories.test_factory_mut` if needed.
pub(super) fn arm_build_via(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    queue_category: ProductionCategory,
    order: u64,
) {
    let oid = sim.interner.intern(owner);
    let tid = sim.interner.intern(type_id);
    let cost = sim.object_type(tid, rules).map_or(0, |o| o.cost.max(0));
    let started =
        sim.production
            .factories
            .test_enqueue_kernel(oid, queue_category, tid, order, cost);
    if started {
        super::construct_active_factory_fixture(sim, rules, oid, queue_category, tid)
            .expect("test production type must construct at StartProduction");
    }
}

#[test]
fn weat_factory_matches_vehicle_category() {
    let rules = factory_rules();
    assert!(is_matching_factory(
        &rules,
        "GAWEAT",
        ObjectCategory::Vehicle
    ));
    assert!(!is_matching_factory(
        &rules,
        "GAWEAT",
        ObjectCategory::Infantry
    ));
}

#[test]
fn custom_modded_factory_recognized_via_factory_key() {
    let rules = factory_rules();
    // MYBARR has Factory=InfantryType — should be recognized without hardcoding its name.
    assert!(is_matching_factory(
        &rules,
        "MYBARR",
        ObjectCategory::Infantry
    ));
    assert!(!is_matching_factory(
        &rules,
        "MYBARR",
        ObjectCategory::Vehicle
    ));
    // XAIRFLD has Factory=AircraftType.
    assert!(is_matching_factory(
        &rules,
        "XAIRFLD",
        ObjectCategory::Aircraft
    ));
}

#[test]
fn exit_coord_parsed_and_used_for_spawn() {
    let rules = factory_rules();
    // GAWEAP has ExitCoord=512,256,0 → 2 cells right, 1 cell down.
    let gaweap = rules.object("GAWEAP").expect("GAWEAP exists");
    assert_eq!(gaweap.exit_coord, Some((512, 256, 0)));

    // MYBARR has ExitCoord=-64,64,0 → rounds to (0, 0) cell offset.
    let mybarr = rules.object("MYBARR").expect("MYBARR exists");
    assert_eq!(mybarr.exit_coord, Some((-64, 64, 0)));

    // GACNST has no ExitCoord.
    let gacnst = rules.object("GACNST").expect("GACNST exists");
    assert_eq!(gacnst.exit_coord, None);

    // Spawn test: GAWEAP at (20,20), ExitCoord→primary cell (22,21).
    let mut sim = Simulation::new();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GAWEAP", 20, 20);
    let producer = sim.substrate.entities.get(1).unwrap();
    let exit = crate::sim::movement::building_exit_coordinate(
        crate::sim::movement::ground_pose::position_world_coord(&producer.position),
        gaweap,
        || {
            crate::sim::movement::ground_pose::object_get_coords(
                producer,
                sim.resolved_terrain.as_ref(),
            )
        },
    );
    assert_eq!(
        (exit.x / 256, exit.y / 256),
        (22, 21),
        "primary exit cell from ExitCoord=512,256,0"
    );
}

#[test]
fn war_factory_spawn_contact_is_marked_per_produced_mover() {
    let rules = factory_rules();
    let mut sim = Simulation::new();
    spawn_structure(&mut sim, &rules, 10, "Americans", "GAWEAP", 20, 20);

    // Supply the ordinary ExitCoord pose. This isolates the shared contact
    // owner; producer selection and Unlimbo have their own native comparisons.
    let produced = sim
        .spawn_object("MTNK", "Americans", 22, 21, 64, &rules)
        .expect("produced tank should spawn");
    let unrelated = sim
        .spawn_object("MTNK", "Americans", 30, 30, 64, &rules)
        .expect("unrelated tank should spawn");

    assert!(mark_war_factory_spawn_contact(
        &mut sim,
        &rules,
        10,
        produced,
        crate::sim::world::FrameEffects::default(),
    ));
    assert!(
        sim.substrate
            .entities
            .get(produced)
            .unwrap()
            .has_live_contact_with(10),
        "produced vehicle should be contacted with its factory"
    );
    assert!(
        !sim.substrate
            .entities
            .get(unrelated)
            .unwrap()
            .has_live_contact_with(10),
        "unrelated vehicles must not inherit the war-factory row exception"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(produced)
            .unwrap()
            .tether
            .partner(),
        Some(10),
        "WF exit must set the dock-entered (+0x418) flag toward the factory"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(unrelated)
            .unwrap()
            .tether
            .partner(),
        None,
        "unrelated vehicles get no dock-entered flag"
    );
}

#[test]
fn naval_factory_spawn_uses_water_exit_cells() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(32, 32);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 32,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(32);

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAYARD", 20, 20);
    stamp_type_foundation(&mut sim, &rules, 1);
    let produced = sim
        .construct_object_limbo_at_height(
            "DEST",
            "Americans",
            0,
            0,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("construct the original ExitObject receiver");
    assert_eq!(
        super::production_queue::exit_produced_object(
            &mut sim,
            &rules,
            1,
            produced,
            None,
            crate::sim::world::FrameEffects::default()
        ),
        crate::sim::ai_base_building::BuildingExit::Placed,
        "naval factory delivers its actual held Unit through shared ExitObject"
    );
    let produced = sim.substrate.entities.get(produced).unwrap();

    assert_eq!(
        (produced.position.rx, produced.position.ry),
        (22, 22),
        "4x4 yard fallback starts at BuildingClass::GetCoords foundation centre"
    );
}

#[test]
fn mixed_land_and_naval_factories_bind_independent_vehicle_and_ship_slots() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(
        "[Countries]\n0=Americans\n[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n1=DEST\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWEAP\n1=GAYARD\n\
         [MTNK]\nCost=700\nStrength=300\nSpeedType=Track\nTechLevel=1\nOwner=Americans\n\
         [DEST]\nCost=1000\nStrength=600\nNaval=yes\nSpeedType=Float\nMovementZone=Water\nTechLevel=1\nOwner=Americans\n\
         [GAWEAP]\nStrength=1000\nOwner=Americans\nFactory=UnitType\nWeaponsFactory=yes\nNaval=no\nExitCoord=512,256,0\n\
         [GAYARD]\nStrength=1500\nOwner=Americans\nFactory=UnitType\nWeaponsFactory=yes\nNaval=yes\nFoundation=4x4\n",
    ), &IniFile::from_str("[GAYARD]\nFoundation=4x4\n"))
    .expect("mixed stock-shaped Vehicle/Ship rules");
    let mut sim = Simulation::new();
    let mut terrain = water_terrain(40, 40);
    let land_exit = terrain.cell_mut(6, 5).unwrap();
    land_exit.is_water = false;
    land_exit.land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
    land_exit.yr_cell_land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
    land_exit.terrain_class = crate::rules::terrain_rules::TerrainClass::Clear;
    let grid = PathGrid::test_all_passable(40, 40);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);
    sim.session.map_width = 40;
    sim.session.map_height = 40;

    // The older/lower stable-id land factory must never win the Ship slot.
    spawn_structure(&mut sim, &rules, 1, "Americans", "GAWEAP", 4, 4);
    spawn_structure(&mut sim, &rules, 2, "Americans", "GAYARD", 20, 20);
    stamp_type_foundation(&mut sim, &rules, 1);
    stamp_type_foundation(&mut sim, &rules, 2);
    // Direct fixture insertion bypasses Building::Unlimbo's 448070 call.
    // Publish both native primary slots before testing delivery selection.
    super::initialize_factory_primary(&mut sim, 1, &rules);
    super::initialize_factory_primary(&mut sim, 2, &rules);
    let vehicle_candidates = super::producer_candidates_for_owner_category(
        &sim.substrate.entities,
        &rules,
        "Americans",
        ProductionCategory::Vehicle,
        true,
        &sim.interner,
    );
    let ship_candidates = super::producer_candidates_for_owner_category(
        &sim.substrate.entities,
        &rules,
        "Americans",
        ProductionCategory::Ship,
        true,
        &sim.interner,
    );
    assert_eq!(
        vehicle_candidates
            .iter()
            .map(|candidate| candidate.0)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(
        ship_candidates
            .iter()
            .map(|candidate| candidate.0)
            .collect::<Vec<_>>(),
        vec![2]
    );

    let americans = sim.interner.intern("Americans");
    assert_eq!(
        super::find_factory(
            &sim,
            &rules,
            americans,
            rules.object("MTNK").unwrap(),
            false,
            true,
            false,
        ),
        Some(1),
        "native typed Vehicle FindFactory binds GAWEAP"
    );
    assert_eq!(
        super::find_factory(
            &sim,
            &rules,
            americans,
            rules.object("DEST").unwrap(),
            false,
            true,
            false,
        ),
        Some(2),
        "native typed Naval Unit FindFactory binds GAYARD"
    );

    assert_eq!(
        sim.production
            .primary_factory(americans, ProductionCategory::Vehicle)
            .unwrap(),
        1
    );
    assert_eq!(
        sim.production
            .primary_factory(americans, ProductionCategory::Ship)
            .unwrap(),
        2
    );

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
        "DEST",
        ProductionCategory::Ship,
        2,
    );
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Vehicle)
            .and_then(|view| view.object)
            .is_some(),
        "Vehicle and Ship queues coexist before delivery"
    );
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Ship)
            .and_then(|view| view.object)
            .is_some()
    );

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let destroyer = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner() == americans
                && sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case("DEST")
        })
        .expect("GAYARD delivered DEST through naval FNPC/Unlimbo");
    assert_eq!((destroyer.position.rx, destroyer.position.ry), (22, 22));
    assert!(destroyer.lifecycle.cell_marked && !destroyer.lifecycle.in_limbo());
    assert_eq!(
        sim.production
            .primary_factory(americans, ProductionCategory::Ship)
            .unwrap(),
        2,
        "the older GAWEAP cannot receive the Ship completion"
    );
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Ship)
            .is_none(),
        "successful Ship delivery advances only the Ship queue"
    );
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Vehicle)
            .and_then(|view| view.object)
            .is_some(),
        "the independent land Vehicle queue remains active"
    );
}

#[test]
fn naval_delivery_nonzero_canenter_refunds_without_trying_second_producer() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let mut terrain = water_terrain(40, 40);
    for cell in &mut terrain.cells {
        cell.speed_costs.float = Some(0);
        cell.base_speed_costs.float = Some(0);
    }
    for candidate in [(12, 12), (22, 22)] {
        let cell = terrain.cell_mut(candidate.0, candidate.1).unwrap();
        cell.speed_costs.float = Some(100);
        cell.base_speed_costs.float = Some(100);
    }
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAYARD", 10, 10);
    spawn_structure(&mut sim, &rules, 2, "Americans", "GAYARD", 20, 20);
    stamp_type_foundation(&mut sim, &rules, 1);
    stamp_type_foundation(&mut sim, &rules, 2);
    super::initialize_factory_primary(&mut sim, 1, &rules);
    super::initialize_factory_primary(&mut sim, 2, &rules);
    let mut blocker =
        crate::sim::game_entity::GameEntity::test_default(50, "DEST", "Soviet", 12, 12);
    blocker.set_category_for_test(crate::map::entities::EntityCategory::Unit);
    blocker.set_owner_for_test(sim.interner.intern("Soviet"));
    blocker.set_type_ref_for_test(sim.interner.intern("DEST"));
    sim.substrate.entities.insert(blocker);
    sim.substrate.occupancy.add(
        12,
        12,
        50,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    let held = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Ship);
    let credits = credits_for_owner(&sim, "Americans");
    assert!(
        !super::dispatch_production_changes_for_tests(&mut sim, &rules, None),
        "nonzero Unit CanEnter rejects the selected producer's one attempt"
    );
    assert!(!sim.substrate.entities.contains(held));
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Ship)
            .is_none_or(|factory| factory.object.is_none())
    );
    assert_eq!(credits_for_owner(&sim, "Americans"), credits + 1000);
    assert_eq!(
        sim.production
            .primary_factory(americans, ProductionCategory::Ship),
        Some(1),
        "HousePlace does not try the second eligible producer after Exit0"
    );
    assert!(
        sim.substrate.entities.contains(50),
        "the cell blocker remains"
    );
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .filter(|entity| {
                sim.interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case("DEST")
                    && entity.owner() == americans
            })
            .count(),
        0,
        "refusal destroys the held Unit without alternate delivery"
    );
    assert_eq!(sim.houses[&americans].stats.built(), 0);
}

#[test]
fn naval_empty_fnpc_refunds_then_a_fresh_successor_records_delivery_once() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let mut terrain = water_terrain(40, 40);
    for cell in &mut terrain.cells {
        cell.speed_costs.float = Some(0);
        cell.base_speed_costs.float = Some(0);
    }
    let allocated = (0..40)
        .flat_map(|ry| (0..40).map(move |rx| (rx, ry)))
        .filter(|cell| *cell != (0, 0))
        .collect::<Vec<_>>();
    terrain.test_set_native_allocated_cells(&allocated);
    let retained_dummy = terrain.shared_cell_dummy();
    let pre_stamped = crate::sim::cell_rect::get_cellclass_fallback(Some(&terrain), -7, 5);
    let crate::sim::cell_rect::CellRef::Dummy { cell: pre_stamped } = pre_stamped else {
        panic!("off-map lookup must return the shared dummy");
    };
    assert!(pre_stamped.same_identity(&retained_dummy));
    assert_eq!(retained_dummy.snapshot().coord, (-7, 5));

    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);
    sim.session.map_width = 40;
    sim.session.map_height = 40;
    let americans = sim.interner.intern("Americans");
    sim.houses.insert(
        americans,
        crate::sim::house_state::HouseState::new(americans, 0, None, true, STARTING_CREDITS, 10),
    );
    spawn_structure(&mut sim, &rules, 1, "Americans", "GAYARD", 10, 10);
    spawn_structure(&mut sim, &rules, 2, "Americans", "GAYARD", 20, 20);
    stamp_type_foundation(&mut sim, &rules, 1);
    stamp_type_foundation(&mut sim, &rules, 2);
    super::initialize_factory_primary(&mut sim, 1, &rules);
    super::initialize_factory_primary(&mut sim, 2, &rules);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        2,
    );
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );
    let refused = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Ship);
    let entity_count = sim.substrate.entities.len();
    let owned_units = sim.owned_object_counts(americans).1;
    let credits = credits_for_owner(&sim, "Americans");
    let mut expected = sim.scenario_rng.clone();
    assert!(!super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_eq!(
        retained_dummy.snapshot().coord,
        (0, 0),
        "the caller resolves FNPC's sentinel and restamps the retained dummy"
    );
    assert!(!sim.substrate.entities.contains(refused));
    let successor = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Ship);
    assert!(
        successor > refused,
        "Exit0 destroys before StartNextQueued constructs"
    );
    let factory = sim
        .production
        .factories
        .view(americans, ProductionCategory::Ship)
        .unwrap();
    assert_eq!(factory.progress, 0);
    assert!(factory.queue.is_empty());
    let object = sim.substrate.entities.get(successor).unwrap();
    assert!(object.lifecycle.in_limbo() && !object.lifecycle.cell_marked);
    super::lifecycle_tests::assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(sim.substrate.entities.len(), entity_count);
    assert_eq!(sim.owned_object_counts(americans).1, owned_units);
    assert_eq!(credits_for_owner(&sim, "Americans"), credits + 1000);
    assert_eq!(sim.houses[&americans].stats.built(), 0);

    let bytes =
        bincode::serialize(&sim.production.factories).expect("serialize the fresh active factory");
    sim.production.factories = bincode::deserialize(&bytes).unwrap();
    // Publishing alone does not invent another PLACE for an unfinished successor.
    assert!(!super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Ship),
        successor
    );
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(
        sim.production
            .primary_factory(americans, ProductionCategory::Ship),
        Some(1)
    );
    assert_eq!(sim.houses[&americans].stats.built(), 0);
    for cell in &mut sim.resolved_terrain.as_mut().unwrap().cells {
        cell.speed_costs.float = Some(100);
        cell.base_speed_costs.float = Some(100);
    }
    let grid = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
    sim.path_grid = Some(std::sync::Arc::new(grid));
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );
    assert!(super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let delivered = sim.substrate.entities.get(successor).unwrap();
    assert!(delivered.lifecycle.cell_marked && !delivered.lifecycle.in_limbo());
    assert_eq!(
        sim.houses[&americans].stats.built(),
        1,
        "Record_Last_Built4FB4B7 follows only the successor's successful exit"
    );
    assert_eq!(sim.substrate.entities.len(), entity_count);
    assert_eq!(sim.owned_object_counts(americans).1, owned_units);
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Ship)
            .is_none_or(|factory| factory.object.is_none())
    );
}

#[test]
fn naval_delivery_success_uses_producer_rally_then_move_and_recentres() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(40, 40);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);
    sim.session.map_width = 40;
    sim.session.map_height = 40;

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAYARD", 10, 10);
    sim.remove_entity_occupancy(1);
    {
        let yard = sim.substrate.entities.get_mut(1).unwrap();
        yard.type_facts.set_foundation_for_test("4x4".to_string());
        yard.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(20, 10)));
    }
    sim.add_entity_occupancy(1);

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner() == americans
                && sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case("DEST")
        })
        .expect("naval Unit delivered");
    assert_eq!(
        (produced.position.rx, produced.position.ry),
        (14, 12),
        "rally fast path walks east out of the 4x4 producer and bypasses FNPC"
    );
    assert_eq!(
        produced.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(20, 10)),
        "the producer rally remains the represented owner destination"
    );
    // The Ship setter accepts the rally without a route; its first Process
    // requests one.
    assert_eq!(
        crate::sim::movement::movement_goal_cell(produced),
        Some((20, 10)),
        "selected producer's rally target owns the destination"
    );
    assert_eq!(
        produced
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .and_then(|ship| ship.destination())
            .map(|coord| (coord.x / 256, coord.y / 256)),
        Some((20, 10))
    );
    assert_eq!(
        produced.mission.queued(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
        "ExitObject queues Move with commence argument zero after assigning the target"
    );
    assert_eq!(
        (produced.position.sub_x, produced.position.sub_y),
        (
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
        ),
        "success tail recentres through CellClass::Get_Center_Coords"
    );
    assert!(
        sim.production
            .factories
            .view(americans, ProductionCategory::Ship)
            .is_none(),
        "successful delivery advances and prunes the completed queue"
    );
}

#[test]
fn naval_rally_destination_and_move_survive_without_path_grid() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(40, 40);
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);
    sim.session.map_width = 40;
    sim.session.map_height = 40;

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAYARD", 10, 10);
    sim.remove_entity_occupancy(1);
    {
        let yard = sim.substrate.entities.get_mut(1).unwrap();
        yard.type_facts.set_foundation_for_test("4x4".to_string());
        yard.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(20, 10)));
    }
    sim.add_entity_occupancy(1);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner() == americans
                && sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case("DEST")
        })
        .expect("rally fast path delivers without a PathGrid");
    assert_eq!(
        produced.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(20, 10)),
        "Assign_Destination commits independently of immediate path authority"
    );
    assert_eq!(
        produced.mission.queued(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
        "Queue_Mission(Move, 0) is not gated by PathGrid availability"
    );
    assert!(
        produced
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty(),
        "the class setter searches no route; the Ship's first Process does"
    );
}

#[test]
fn naval_rally_destination_and_move_survive_beyond_the_path_grid() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(40, 40);
    // The rally fast path exits the 4x4 yard at this cache's last cell. The
    // distant rally lies outside the deliberately truncated path grid; the
    // Ship setter publishes it without a search.
    let grid = PathGrid::test_all_passable(15, 15);
    sim.install_fixture_path_grid(Some(&grid));
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);
    sim.session.map_width = 40;
    sim.session.map_height = 40;

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAYARD", 10, 10);
    sim.remove_entity_occupancy(1);
    {
        let yard = sim.substrate.entities.get_mut(1).unwrap();
        yard.type_facts.set_foundation_for_test("4x4".to_string());
        yard.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(39, 39)));
    }
    sim.add_entity_occupancy(1);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factories
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner() == americans
                && sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case("DEST")
        })
        .expect("naval Unit delivers toward a rally beyond the path grid");
    assert_eq!((produced.position.rx, produced.position.ry), (14, 14));
    assert_eq!(
        produced.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(39, 39)),
        "the producer-owned destination is published unchanged"
    );
    assert_eq!(
        produced.mission.queued(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
        "ExitObject queues the native deferred Move mission"
    );
    // Unit741970 -> Ship69F450 installs no route; the first Process asks.
    assert!(
        crate::sim::movement::motion_query::is_moving(&produced) == Some(true),
        "scheduled Process"
    );
    assert!(
        produced
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(
        crate::sim::movement::movement_goal_cell(produced),
        Some((39, 39))
    );
}

#[test]
fn custom_exit_coord_modded_factory_is_parsed() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=MODFACT\n\
         [MODFACT]\n\
         Factory=UnitType\n\
         ExitCoord=768,512,0\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("modded rules should parse");
    let modfact = rules.object("MODFACT").expect("MODFACT exists");
    // 768/256=3 cells right, 512/256=2 cells down.
    assert_eq!(modfact.exit_coord, Some((768, 512, 0)));

    // This non-WeaponsFactory type reaches another native ExitObject arm.
    // Its parser assertion does not establish a gameplay spawn coordinate.
}

#[test]
#[ignore = "WIP: harvester path-grid movement not yet landed"]
fn harvester_moves_to_ore_and_back_with_path_grid() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    let grid = PathGrid::new(64, 64);

    let harvester_sid = sim
        .spawn_object("HARV", "Americans", 10, 10, 64, &rules)
        .expect("spawn harvester");
    spawn_structure(&mut sim, &rules, 2, "Americans", "GAREFN", 8, 10);
    // Whole-multiple of the ore base (120) so the cell drains cleanly. The
    // production overlay seeder always stores `(frame+1) * base`, so cells
    // in real maps never carry a sub-density-level leftover.
    crate::sim::tiberium::test_support::place_stock_amount(
        &mut sim,
        (12, 10),
        ResourceType::Ore,
        6 * 120,
    );

    let before = credits_for_owner(&sim, "Americans");
    let mut moved = false;
    // Run enough ticks for a full harvest cycle: search → move to ore →
    // harvest bales → return to refinery → dock → unload.
    for _ in 0..3000 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
        let pos = sim
            .substrate
            .entities
            .get(harvester_sid)
            .map(|e| (e.position.rx, e.position.ry))
            .expect("harvester position");
        if pos != (10, 10) {
            moved = true;
        }
    }

    assert!(
        moved,
        "harvester should physically move toward ore/refinery"
    );
    // With the new Miner-component system, credits are earned via bale
    // unloading (25 per ore bale) rather than the legacy flat-140 system.
    let earned = credits_for_owner(&sim, "Americans") - before;
    assert!(
        earned > 0,
        "harvester should have earned some credits, got {earned}"
    );
}

/// Each house has its own wallet; a house that does not exist has no money
/// and is not made up.
#[test]
fn owner_credits_are_isolated() {
    let mut sim = Simulation::new();
    super::house_for_test(&mut sim, "Soviet");
    super::house_for_test(&mut sim, "Americans")
        .economy
        .add_credits(-750);
    assert_eq!(credits_for_owner(&sim, "Americans"), STARTING_CREDITS - 750);
    assert_eq!(credits_for_owner(&sim, "Soviet"), STARTING_CREDITS);
    assert_eq!(credits_for_owner(&sim, "Germans"), 0);
    assert!(
        sim.interner
            .get("Germans")
            .is_none_or(|id| !sim.houses.contains_key(&id))
    );
}
