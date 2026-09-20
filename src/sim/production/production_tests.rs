//! Production integration tests — end-to-end tests for build completion, unit spawning,
//! harvester auto-creation, and sell/undeploy flows through the full production pipeline.

use std::collections::BTreeMap;

use super::production_spawn::{
    find_spawn_selection_for_owner, find_spawn_selection_for_owner_with_type,
    mark_war_factory_spawn_contact,
};
use super::war_factory_exit::tick_war_factory_exit_contacts;
use super::{
    ProductionCategory, STARTING_CREDITS, credits_for_owner, find_spawn_cell_for_owner,
    is_matching_factory, structure_satisfies_prerequisite,
};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Health;
use crate::sim::miner::ResourceType;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

pub(super) fn basic_infantry_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
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
             Factory=InfantryType\n\
             [NAHAND]\n\
             Factory=InfantryType\n",
    );
    RuleSet::from_ini(&ini).expect("basic infantry rules should parse")
}

pub(super) fn basic_multi_queue_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
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
             Factory=InfantryType\n\
             [GAWEAP]\n\
             Factory=UnitType\n",
    );
    RuleSet::from_ini(&ini).expect("basic multi queue rules should parse")
}

pub(super) fn production_modifier_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\n\
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
             Factory=InfantryType\n\
             [NAHAND]\n\
             Power=-20\n\
             Factory=InfantryType\n\
             [GAWEAP]\n\
             Power=-20\n\
             Factory=UnitType\n\
             [GAPOWR]\n\
             Strength=1000\nPower=200\n",
    );
    RuleSet::from_ini(&ini).expect("production modifier rules should parse")
}

pub(super) fn build_catalog_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
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
             Refinery=yes\n\
             FreeUnit=HARV\n\
             Foundation=3x3\n\
             [GAPILE]\n\
             TechLevel=-1\n\
             Factory=InfantryType\n\
             [GAWEAP]\n\
             TechLevel=-1\n\
             Factory=UnitType\n\
             [GAAIRC]\n\
             TechLevel=-1\n\
             Factory=AircraftType\n",
    );
    RuleSet::from_ini(&ini).expect("build catalog rules should parse")
}

pub(super) fn naval_production_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
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
         Factory=UnitType\n\
         WeaponsFactory=yes\n\
         Naval=yes\n\
         Foundation=4x4\n\
         SpeedType=Float\n\
         WaterBound=yes\n\
         ExitCoord=512,256,0\n",
    );
    RuleSet::from_ini(&ini).expect("naval production rules should parse")
}

pub(super) fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    let mut cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            cells.push(ResolvedTerrainCell {
                rx: x,
                ry: y,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: Some(0),
                land_type: 2,
                yr_cell_land_type: 2,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: crate::rules::terrain_rules::TerrainClass::Water,
                speed_costs: crate::rules::terrain_rules::SpeedCostProfile {
                    float: Some(100),
                    hover: Some(100),
                    ..crate::rules::terrain_rules::SpeedCostProfile::default()
                },
                is_water: true,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: crate::map::resolved_terrain::zone_class::WATER,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 2,
                base_yr_cell_land_type: 2,
                base_terrain_class: crate::rules::terrain_rules::TerrainClass::Water,
                base_speed_costs: crate::rules::terrain_rules::SpeedCostProfile {
                    float: Some(100),
                    hover: Some(100),
                    ..crate::rules::terrain_rules::SpeedCostProfile::default()
                },
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

pub(super) fn placement_radius_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GACNST\n\
             1=GAPOWR\n\
             2=GAGAP\n\
             [GACNST]\n\
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
             Adjacent=0\n",
    );
    RuleSet::from_ini(&ini).expect("placement radius rules should parse")
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
             2=CAGAS01\n\
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
             [CAGAS01]\n\
             Name=GasStation\n\
             Cost=0\n\
             Strength=400\n\
             Armor=wood\n\
             Foundation=1x1\n\
             CanBeOccupied=yes\n\
             CanOccupyFire=yes\n\
             MaxNumberOccupants=5\n",
    );
    RuleSet::from_ini(&ini).expect("sell rules should parse")
}

/// Rules with Factory= keys for testing data-driven factory matching.
/// Includes standard RA2 factories plus custom modded ones (MYBARR, XAIRFLD).
pub(super) fn factory_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
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
             Factory=BuildingType\n\
             [MTNK]\n\
             Name=Medium Tank\n\
             Strength=300\n\
             Armor=heavy\n\
             Speed=6\n\
             [GAPILE]\n\
             Factory=InfantryType\n\
             Foundation=3x2\n\
             [GAWEAP]\n\
             Factory=UnitType\n\
             WeaponsFactory=yes\n\
             Foundation=5x3\n\
             ExitCoord=512,256,0\n\
             [GAWEAT]\n\
             Factory=UnitType\n\
             WeaponsFactory=yes\n\
             Foundation=5x3\n\
             ExitCoord=512,256,0\n\
             [GAAIRC]\n\
             Factory=AircraftType\n\
             Foundation=3x2\n\
             ExitCoord=384,128,0\n\
             [MYBARR]\n\
             Factory=InfantryType\n\
             ExitCoord=-64,64,0\n\
             Foundation=2x2\n\
             [XAIRFLD]\n\
             Factory=AircraftType\n\
             ExitCoord=384,128,0\n",
    );
    RuleSet::from_ini(&ini).expect("factory rules should parse")
}

/// Rules with [General] PrerequisiteXxx groups for testing data-driven
/// prerequisite alias resolution.
pub(super) fn prerequisite_group_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\n\
         PrerequisitePower=GAPOWR,NAPOWR,NANRCT\n\
         PrerequisiteProc=GAREFN,NAREFN\n\
         PrerequisiteRadar=GAAIRC,NARADR\n\
         PrerequisiteTech=GATECH,NATECH\n\
         PrerequisiteBarracks=GAPILE,NAHAND\n\
         PrerequisiteFactory=GAWEAP,NAWEAP\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GAPOWR\n\
         1=NAPOWR\n\
         2=NANRCT\n\
         3=GAREFN\n\
         4=NAREFN\n\
         5=GAAIRC\n\
         6=NARADR\n\
         7=GATECH\n\
         8=NATECH\n\
         9=GAPILE\n\
         10=NAHAND\n\
         11=GAWEAP\n\
         12=NAWEAP\n\
         [GAPOWR]\n\
         [NAPOWR]\n\
         [NANRCT]\n\
         [GAREFN]\n\
         [NAREFN]\n\
         [GAAIRC]\n\
         [NARADR]\n\
         [GATECH]\n\
         [NATECH]\n\
         [GAPILE]\n\
         [NAHAND]\n\
         [GAWEAP]\n\
         [NAWEAP]\n",
    );
    RuleSet::from_ini(&ini).expect("prerequisite group rules should parse")
}

pub(super) fn spawn_structure(
    sim: &mut Simulation,
    sid: u64,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
) {
    let owner_id = sim.interner.intern(owner);
    let type_id_interned = sim.interner.intern(type_id);
    let mut ge = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health { current: 1000 },
        type_id_interned,
        crate::map::entities::EntityCategory::Structure,
        0,
        5,
        false,
    );
    ge.lifecycle.in_limbo = false;
    ge.in_playfield = true;
    sim.substrate.entities.insert(ge);
    // Use the lifecycle boundary so the fixture is a placed (not factory-held)
    // structure and raw-store consumers see the same Mark state as gameplay.
    sim.add_entity_occupancy(sid);
    if sim.substrate.next_stable_object_id <= sid {
        sim.substrate.next_stable_object_id = sid + 1;
    }
}

/// P5d: arm a registry factory's queue-of-record directly (replaces the retired
/// `queues_by_owner` insert of a `BuildQueueItem`). Interns owner/type, resolves cost from
/// `rules`, and calls the registry `enqueue` (create-the-active-build, or append to the FIFO
/// tail if a build is already active for this `(owner, category)`). Use one call per item, in
/// enqueue order; `order` is the temporal stamp (the active build's `insertion_seq` / a tail
/// entry's `enqueue_order`). The `remaining`-frames concept is retired (progress lives in the
/// registry); set a build's progress afterward via `factory_shadow.test_factory_mut` if needed.
pub(super) fn arm_build_via(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    queue_category: ProductionCategory,
    total_base_frames: u32,
    order: u64,
) {
    let oid = sim.interner.intern(owner);
    let tid = sim.interner.intern(type_id);
    let cost = sim.object_type(tid, rules).map_or(0, |o| o.cost.max(0));
    let started = sim.production.factory_shadow.enqueue(
        oid,
        queue_category,
        tid,
        order,
        total_base_frames,
        cost,
    );
    if started {
        super::construct_active_factory_fixture(sim, rules, oid, queue_category, tid)
            .expect("test production type must construct at StartProduction");
    }
}

#[test]
fn structure_satisfies_prerequisite_with_groups() {
    let rules = prerequisite_group_rules();
    // Direct match: GAWEAP satisfies GAWEAP.
    assert!(structure_satisfies_prerequisite(&rules, "GAWEAP", "GAWEAP"));
    // Alias match: NAWEAP is in PrerequisiteFactory list, which also maps to WARFACTORY.
    assert!(structure_satisfies_prerequisite(
        &rules,
        "NAWEAP",
        "WARFACTORY"
    ));
    assert!(structure_satisfies_prerequisite(
        &rules, "GAWEAP", "FACTORY"
    ));
    // Power alias.
    assert!(structure_satisfies_prerequisite(&rules, "GAPOWR", "POWER"));
    assert!(structure_satisfies_prerequisite(&rules, "NANRCT", "POWER"));
    // Barracks alias + TENT secondary alias.
    assert!(structure_satisfies_prerequisite(
        &rules, "GAPILE", "BARRACKS"
    ));
    assert!(structure_satisfies_prerequisite(&rules, "NAHAND", "TENT"));
    // Unknown alias: not in any group and not a direct match.
    assert!(!structure_satisfies_prerequisite(&rules, "GAPOWR", "RADAR"));
}

#[test]
fn custom_modded_prerequisite_group_recognized() {
    let ini = IniFile::from_str(
        "[General]\n\
         PrerequisitePower=MODPOWR,MODSOLR\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=MODPOWR\n\
         1=MODSOLR\n\
         [MODPOWR]\n\
         Name=Mod Power\n\
         [MODSOLR]\n\
         Name=Mod Solar\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("modded rules should parse");
    assert!(structure_satisfies_prerequisite(&rules, "MODPOWR", "POWER"));
    assert!(structure_satisfies_prerequisite(&rules, "MODSOLR", "POWER"));
    assert!(!structure_satisfies_prerequisite(&rules, "GAPOWR", "POWER"));
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
    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 20, 20);
    let spawn = find_spawn_cell_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Vehicle,
        None,
        false,
    )
    .expect("should find spawn cell");
    assert_eq!(
        spawn,
        (22, 21),
        "primary exit cell from ExitCoord=512,256,0"
    );
}

#[test]
fn war_factory_spawn_contact_is_marked_per_produced_mover() {
    let rules = factory_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    spawn_structure(&mut sim, 10, "Americans", "GAWEAP", 20, 20);

    let selection = find_spawn_selection_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Vehicle,
        None,
        false,
    )
    .expect("war factory should provide a spawn selection");
    assert_eq!(selection.producer_id, 10);
    assert_eq!(selection.cell, (22, 21));

    let produced = sim
        .spawn_object(
            "MTNK",
            "Americans",
            selection.cell.0,
            selection.cell.1,
            64,
            &rules,
            &height_map,
        )
        .expect("produced tank should spawn");
    let unrelated = sim
        .spawn_object("MTNK", "Americans", 30, 30, 64, &rules, &height_map)
        .expect("unrelated tank should spawn");

    assert!(mark_war_factory_spawn_contact(
        &mut sim,
        &rules,
        selection.producer_id,
        produced,
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
            .dock_entered_with,
        Some(10),
        "WF exit must set the dock-entered (+0x418) flag toward the factory"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(unrelated)
            .unwrap()
            .dock_entered_with,
        None,
        "unrelated vehicles get no dock-entered flag"
    );
}

#[test]
fn war_factory_exit_contact_held_while_on_footprint() {
    let rules = factory_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    spawn_structure(&mut sim, 10, "Americans", "GAWEAP", 20, 20);
    // Spawn the produced tank ON the factory's occupancy cell. `spawn_structure`
    // registers the test structure at a single occupancy cell (its origin 20,20 —
    // see production_tests.rs spawn_structure), so (20,20) is the cell that has a
    // Structure occupant under it. (Real production occupies the full footprint via
    // entity_occupancy_cells; the test helper is the single-cell simplification.)
    let produced = sim
        .spawn_object("MTNK", "Americans", 20, 20, 64, &rules, &height_map)
        .expect("produced tank should spawn");
    assert!(mark_war_factory_spawn_contact(
        &mut sim, &rules, 10, produced
    ));

    tick_war_factory_exit_contacts(
        &mut sim.substrate.entities,
        &sim.substrate.occupancy,
        &rules,
        &sim.interner,
    );

    let mover = sim.substrate.entities.get(produced).unwrap();
    assert!(
        mover.has_live_contact_with(10),
        "contact must persist while the vehicle is still on the factory footprint"
    );
    assert_eq!(mover.dock_entered_with, Some(10));
}

#[test]
fn war_factory_exit_contact_breaks_when_unit_clears_footprint() {
    let rules = factory_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    spawn_structure(&mut sim, 10, "Americans", "GAWEAP", 20, 20);
    // Spawn the produced tank on a clear cell well away from the foundation.
    let produced = sim
        .spawn_object("MTNK", "Americans", 30, 30, 64, &rules, &height_map)
        .expect("produced tank should spawn");
    assert!(mark_war_factory_spawn_contact(
        &mut sim, &rules, 10, produced
    ));

    tick_war_factory_exit_contacts(
        &mut sim.substrate.entities,
        &sim.substrate.occupancy,
        &rules,
        &sim.interner,
    );

    let mover = sim.substrate.entities.get(produced).unwrap();
    assert!(
        !mover.has_live_contact_with(10),
        "contact must break once the vehicle has cleared the factory footprint"
    );
    assert_eq!(
        mover.dock_entered_with, None,
        "the dock-entered flag (+0x418) must clear with the contact"
    );
}

#[test]
fn war_factory_exit_break_ignores_non_weapons_factory_producer() {
    // Protects the refinery dock lifecycle: a non-UnitType producer's dock-entered
    // flag must never be broken by this sweep. GAPILE (Factory=InfantryType) stands
    // in for any non-WeaponsFactory-land producer.
    let rules = factory_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    spawn_structure(&mut sim, 10, "Americans", "GAPILE", 20, 20);
    let mover = sim
        .spawn_object("MTNK", "Americans", 30, 30, 64, &rules, &height_map)
        .expect("mover should spawn");
    // Manually emulate a non-WF dock-entered link (as the refinery bus would set).
    let m = sim.substrate.entities.get_mut(mover).unwrap();
    m.mark_live_contact_with(10);
    m.dock_entered_with = Some(10);

    tick_war_factory_exit_contacts(
        &mut sim.substrate.entities,
        &sim.substrate.occupancy,
        &rules,
        &sim.interner,
    );

    let m = sim.substrate.entities.get(mover).unwrap();
    assert!(
        m.has_live_contact_with(10),
        "non-WeaponsFactory dock-entered links must be left to their own lifecycle"
    );
    assert_eq!(m.dock_entered_with, Some(10));
}

#[test]
fn infantry_spawn_uses_foundation_center_cell() {
    let rules = factory_rules();
    // GAPILE has Foundation=3x2 in the fixture, no ExitCoord.
    // Foundation-center cell of a building at (20, 20) is (20 + 3/2, 20 + 2/2)
    // = (21, 21) — the cell inside the foundation that gamemd's
    // building->GetCoord() lepton lands in.
    let mut sim = Simulation::new();
    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 20, 20);
    let spawn = find_spawn_cell_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Infantry,
        None,
        false,
    )
    .expect("infantry spawn from GAPILE should succeed");
    assert_eq!(
        spawn,
        (21, 21),
        "infantry spawns at foundation-center cell of 3x2 GAPILE at (20, 20)"
    );
}

#[test]
fn infantry_spawn_ignores_exit_coord() {
    let rules = factory_rules();
    // MYBARR has ExitCoord=-64,64,0 AND Foundation=2x2.
    // gamemd's infantry alt path NEVER reads ExitCoord; the unit Unlimbos at
    // building->GetCoord() = foundation center. For a 2x2 barracks at (10, 10)
    // that's (10 + 2/2, 10 + 2/2) = (11, 11). The (-64, 64) ExitCoord must
    // have zero effect.
    let mut sim = Simulation::new();
    spawn_structure(&mut sim, 1, "Americans", "MYBARR", 10, 10);
    let spawn = find_spawn_cell_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Infantry,
        None,
        false,
    )
    .expect("infantry spawn from MYBARR should succeed");
    assert_eq!(
        spawn,
        (11, 11),
        "infantry spawn ignores ExitCoord=-64,64,0; uses foundation-center cell"
    );
}

#[test]
fn infantry_spawn_succeeds_when_center_cell_blocked() {
    let rules = factory_rules();
    // The producing GAPILE itself occupies (20, 20) via spawn_structure's
    // single-cell registration. The new foundation-center cell (21, 21) is
    // inside the building's footprint. gamemd's infantry alt path performs
    // no passability check at the spawn step — only vehicles are
    // hard-blocked by building cells. Infantry succeed.
    let mut sim = Simulation::new();
    spawn_structure(&mut sim, 1, "Americans", "GAPILE", 20, 20);
    // Also occupy the foundation-center cell explicitly to make the test
    // robust against future changes to spawn_structure's occupancy footprint.
    sim.substrate.occupancy.add(
        21,
        21,
        1,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    let spawn = find_spawn_cell_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Infantry,
        None,
        false,
    )
    .expect("infantry spawn should succeed even with building bit on center cell");
    assert_eq!(
        spawn,
        (21, 21),
        "infantry spawn ignores foundation occupancy and lands at center cell"
    );
}

#[test]
fn naval_factory_spawn_uses_water_exit_cells() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(32, 32);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 32,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(32);

    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 20, 20);
    let spawn = find_spawn_cell_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Vehicle,
        Some(&grid),
        true,
    )
    .expect("naval factory should find a water exit cell");

    assert_eq!(
        spawn,
        (22, 22),
        "4x4 yard fallback starts at BuildingClass::GetCoords foundation centre"
    );
}

#[test]
fn mixed_land_and_naval_factories_bind_independent_vehicle_and_ship_slots() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n1=DEST\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWEAP\n1=GAYARD\n\
         [MTNK]\nCost=700\nSpeedType=Track\nTechLevel=1\nOwner=Americans\n\
         [DEST]\nCost=1000\nNaval=yes\nSpeedType=Float\nMovementZone=Water\nTechLevel=1\nOwner=Americans\n\
         [GAWEAP]\nFactory=UnitType\nWeaponsFactory=yes\nNaval=no\nExitCoord=512,256,0\n\
         [GAYARD]\nFactory=UnitType\nWeaponsFactory=yes\nNaval=yes\nFoundation=4x4\n",
    ))
    .expect("mixed stock-shaped Vehicle/Ship rules");
    let mut sim = Simulation::new();
    let mut terrain = water_terrain(40, 40);
    let land_exit = terrain.cell_mut(6, 5).unwrap();
    land_exit.is_water = false;
    land_exit.land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
    land_exit.yr_cell_land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
    land_exit.terrain_class = crate::rules::terrain_rules::TerrainClass::Clear;
    let grid = PathGrid::test_all_passable(40, 40);
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
    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 4, 4);
    spawn_structure(&mut sim, 2, "Americans", "GAYARD", 20, 20);
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

    let land_selection = find_spawn_selection_for_owner_with_type(
        &mut sim,
        &rules,
        "Americans",
        Some("MTNK"),
        ObjectCategory::Vehicle,
        Some(&grid),
        false,
    )
    .expect("land Vehicle binds GAWEAP");
    let ship_selection = find_spawn_selection_for_owner_with_type(
        &mut sim,
        &rules,
        "Americans",
        Some("DEST"),
        ObjectCategory::Vehicle,
        Some(&grid),
        true,
    )
    .expect("naval Unit binds GAYARD");
    assert_eq!(land_selection.producer_id, 1);
    assert_eq!(ship_selection.producer_id, 2);

    let americans = sim.interner.intern("Americans");
    assert_eq!(
        sim.production.active_producer_by_owner[&americans][&ProductionCategory::Vehicle],
        1
    );
    assert_eq!(
        sim.production.active_producer_by_owner[&americans][&ProductionCategory::Ship],
        2
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
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        100,
        2,
    );
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );
    assert!(
        sim.production
            .factory_shadow
            .view(americans, ProductionCategory::Vehicle)
            .and_then(|view| view.object)
            .is_some(),
        "Vehicle and Ship queues coexist before delivery"
    );
    assert!(
        sim.production
            .factory_shadow
            .view(americans, ProductionCategory::Ship)
            .and_then(|view| view.object)
            .is_some()
    );

    assert!(super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&grid),
    ));
    let destroyer = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner == americans
                && sim
                    .interner
                    .resolve(entity.type_ref)
                    .eq_ignore_ascii_case("DEST")
        })
        .expect("GAYARD delivered DEST through naval FNPC/Unlimbo");
    assert_eq!((destroyer.position.rx, destroyer.position.ry), (22, 22));
    assert!(destroyer.lifecycle.cell_marked && !destroyer.lifecycle.in_limbo);
    assert_eq!(
        sim.production.active_producer_by_owner[&americans][&ProductionCategory::Ship],
        2,
        "the older GAWEAP cannot receive the Ship completion"
    );
    assert!(
        sim.production
            .factory_shadow
            .view(americans, ProductionCategory::Ship)
            .is_none(),
        "successful Ship delivery advances only the Ship queue"
    );
    assert!(
        sim.production
            .factory_shadow
            .view(americans, ProductionCategory::Vehicle)
            .and_then(|view| view.object)
            .is_some(),
        "the independent land Vehicle queue remains active"
    );
}

#[test]
fn naval_delivery_nonzero_canenter_keeps_pending_and_does_not_try_second_producer() {
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
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 40,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.playfield_size_height = Some(40);

    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAYARD", 20, 20);
    let mut blocker =
        crate::sim::game_entity::GameEntity::test_default(50, "DEST", "Soviet", 12, 12);
    blocker.category = crate::map::entities::EntityCategory::Unit;
    blocker.owner = sim.interner.intern("Soviet");
    blocker.type_ref = sim.interner.intern("DEST");
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
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    let spawned =
        super::production_queue::tick_production(&mut sim, &rules, &BTreeMap::new(), Some(&grid));
    assert!(
        !spawned,
        "nonzero Unit CanEnter result rejects the one attempt"
    );
    let held = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Ship)
        .expect("completed object remains held");
    assert!(held.object.is_some() && held.progress == super::PRODUCTION_STEPS);
    let held_id = held
        .object
        .and_then(|object| object.entity_id)
        .expect("the completed queue owns one limbo Unit identity");
    assert_eq!(
        sim.production.active_producer_by_owner[&americans][&ProductionCategory::Ship],
        1,
        "the already selected producer remains authoritative"
    );
    let held_entity = sim.substrate.entities.get(held_id).unwrap();
    assert!(held_entity.lifecycle.in_limbo && !held_entity.lifecycle.cell_marked);
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .filter(|entity| {
                sim.interner
                    .resolve(entity.type_ref)
                    .eq_ignore_ascii_case("DEST")
                    && entity.owner == americans
            })
            .count(),
        1,
        "failure retains one queue-held Unit and creates no alternate delivery"
    );
}

#[test]
fn naval_empty_fnpc_reuses_pending_identity_and_accounts_completion_once() {
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
    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAYARD", 20, 20);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
        1,
    );
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
        2,
    );
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );
    let entity_count_before = sim.substrate.entities.len();
    let owned_units_before = sim.houses[&americans].owned_unit_count;

    assert!(!super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&grid),
    ));
    assert_eq!(
        retained_dummy.snapshot().coord,
        (0, 0),
        "the caller resolves FNPC's sentinel and restamps the retained dummy"
    );
    let held_id = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Ship)
        .and_then(|view| view.object.and_then(|object| object.entity_id))
        .expect("zero-cell Unlimbo refusal retains the completed Unit");
    let held = sim.substrate.entities.get(held_id).unwrap();
    assert!(held.lifecycle.in_limbo && !held.lifecycle.cell_marked);
    assert_eq!(sim.substrate.entities.len(), entity_count_before);
    assert_eq!(sim.houses[&americans].owned_unit_count, owned_units_before);
    assert_eq!(
        sim.houses[&americans].stats.built, 1,
        "completion is accounted before the refused delivery"
    );

    let registry_bytes = bincode::serialize(&sim.production.factory_shadow)
        .expect("serialize refused completed factory");
    sim.production.factory_shadow =
        bincode::deserialize(&registry_bytes).expect("restore refused completed factory");

    assert!(!super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&grid),
    ));
    let retried_id = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Ship)
        .and_then(|view| view.object.and_then(|object| object.entity_id))
        .unwrap();
    assert_eq!(retried_id, held_id, "retry must reuse the held identity");
    assert_eq!(retained_dummy.snapshot().coord, (0, 0));
    assert_eq!(sim.substrate.entities.len(), entity_count_before);
    assert_eq!(
        sim.houses[&americans].owned_unit_count, owned_units_before,
        "sentinel retry must not account for a second Unit"
    );
    assert_eq!(
        sim.houses[&americans].stats.built, 1,
        "the serialized held-object latch prevents retry accounting"
    );
    assert_eq!(
        sim.production.active_producer_by_owner[&americans][&ProductionCategory::Ship],
        1,
        "the selected producer remains authoritative after empty FNPC"
    );
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .filter(|entity| {
                entity.owner == americans
                    && sim
                        .interner
                        .resolve(entity.type_ref)
                        .eq_ignore_ascii_case("DEST")
            })
            .count(),
        1,
        "no alternate candidate or producer creates another Unit"
    );

    for cell in &mut sim.resolved_terrain.as_mut().unwrap().cells {
        cell.speed_costs.float = Some(100);
        cell.base_speed_costs.float = Some(100);
    }
    let success_grid = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
    assert!(super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&success_grid),
    ));
    let delivered = sim.substrate.entities.get(held_id).unwrap();
    assert!(delivered.lifecycle.cell_marked && !delivered.lifecycle.in_limbo);
    assert_eq!(
        sim.houses[&americans].stats.built, 1,
        "successful retry commits the already-accounted held identity"
    );
    let promoted = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Ship)
        .expect("tail item is promoted after delivery");
    assert_eq!(promoted.progress, 0);
    let promoted_id = promoted
        .object
        .and_then(|object| object.entity_id)
        .expect("StartNextQueued constructs the promoted Unit immediately");
    assert!(
        sim.substrate
            .entities
            .get(promoted_id)
            .is_some_and(|entity| entity.lifecycle.in_limbo)
    );

    // Free the first delivered anchor so the promoted item can exercise its own
    // completion edge without turning this test into a CanEnter blocker case.
    sim.remove_entity_occupancy(held_id);
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );
    assert!(super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&success_grid),
    ));
    assert_eq!(
        sim.houses[&americans].stats.built, 2,
        "the newly promoted object owns one fresh completion edge"
    );
}

#[test]
fn naval_delivery_success_uses_producer_rally_then_move_and_recentres() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(40, 40);
    let grid = PathGrid::from_resolved_terrain(&terrain);
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

    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 10, 10);
    sim.remove_entity_occupancy(1);
    {
        let yard = sim.substrate.entities.get_mut(1).unwrap();
        yard.foundation = "4x4".to_string();
        yard.rally_target = Some((20, 10));
    }
    sim.add_entity_occupancy(1);

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    assert!(super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&grid)
    ));
    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner == americans
                && sim
                    .interner
                    .resolve(entity.type_ref)
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
    assert_eq!(
        produced
            .movement_target
            .as_ref()
            .and_then(|movement| movement.path.last().copied()),
        Some((20, 10)),
        "selected producer's rally target owns the destination"
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
            .factory_shadow
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

    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 10, 10);
    sim.remove_entity_occupancy(1);
    {
        let yard = sim.substrate.entities.get_mut(1).unwrap();
        yard.foundation = "4x4".to_string();
        yard.rally_target = Some((20, 10));
    }
    sim.add_entity_occupancy(1);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    assert!(super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        None,
    ));
    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner == americans
                && sim
                    .interner
                    .resolve(entity.type_ref)
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
        produced.movement_target.is_none(),
        "no-grid delivery has no immediate A* execution path"
    );
}

#[test]
fn naval_rally_destination_and_move_survive_failed_immediate_path() {
    let rules = naval_production_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(40, 40);
    // The rally fast path exits the 4x4 yard at this cache's last cell. The
    // distant rally and its complete ten-cell substitute search are outside
    // the deliberately truncated immediate-path cache.
    let grid = PathGrid::test_all_passable(15, 15);
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

    spawn_structure(&mut sim, 1, "Americans", "GAYARD", 10, 10);
    sim.remove_entity_occupancy(1);
    {
        let yard = sim.substrate.entities.get_mut(1).unwrap();
        yard.foundation = "4x4".to_string();
        yard.rally_target = Some((39, 39));
    }
    sim.add_entity_occupancy(1);
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "DEST",
        ProductionCategory::Ship,
        1,
        1,
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Ship)
    );

    assert!(super::production_queue::tick_production(
        &mut sim,
        &rules,
        &BTreeMap::new(),
        Some(&grid),
    ));
    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner == americans
                && sim
                    .interner
                    .resolve(entity.type_ref)
                    .eq_ignore_ascii_case("DEST")
        })
        .expect("naval Unit still delivers when immediate A* fails");
    assert_eq!((produced.position.rx, produced.position.ry), (14, 14));
    assert_eq!(
        produced.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(39, 39)),
        "failed A* cannot erase the producer-owned destination"
    );
    assert_eq!(
        produced.mission.queued(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
        "failed A* cannot suppress the native deferred Move mission"
    );
    assert!(
        produced.movement_target.is_none(),
        "the fixture must actually force the immediate-path failure"
    );
}

#[test]
fn custom_exit_coord_modded_factory() {
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

    let mut sim = Simulation::new();
    spawn_structure(&mut sim, 1, "Americans", "MODFACT", 10, 10);
    let spawn = find_spawn_cell_for_owner(
        &mut sim,
        &rules,
        "Americans",
        ObjectCategory::Vehicle,
        None,
        false,
    )
    .expect("should find spawn cell");
    assert_eq!(
        spawn,
        (13, 12),
        "exit at (10+3, 10+2) from ExitCoord=768,512,0"
    );
}

#[test]
#[ignore = "WIP: harvester path-grid movement not yet landed"]
fn harvester_moves_to_ore_and_back_with_path_grid() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let grid = PathGrid::new(64, 64);

    let harvester_sid = sim
        .spawn_object("HARV", "Americans", 10, 10, 64, &rules, &height_map)
        .expect("spawn harvester");
    spawn_structure(&mut sim, 2, "Americans", "GAREFN", 8, 10);
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
        let _ = sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 33);
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

#[test]
fn owner_credits_are_isolated() {
    let mut sim = Simulation::new();
    *super::credits_entry_for_owner(&mut sim, "Americans") -= 750;
    assert_eq!(credits_for_owner(&sim, "Americans"), STARTING_CREDITS - 750);
    assert_eq!(credits_for_owner(&sim, "Soviet"), STARTING_CREDITS);
}
