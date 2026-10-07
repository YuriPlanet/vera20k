//! Building placement tests — verifies foundation overlap detection, placement validity,
//! and per-owner placement pool management for the production system.

use std::collections::VecDeque;

use super::{
    BuildingPlacementError, ProductionCategory, ProductionPlacement, credits_for_owner,
    cycle_active_producer_for_owner_category, foundation_dimensions,
    place_production_with_overlays, placement_preview_for_owner_with_overlays,
    placement_preview_for_owner_without_overlays, producer_candidates_for_owner_category,
    publish_production_changes, ready_buildings_for_owner,
};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{
    RampDirection, ResolvedTerrainCell, ResolvedTerrainGrid, zone_class,
};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
use crate::sim::combat::AttackTarget;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{BuildingUp, Health};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::overlay_grid::{OverlayGrid, recalc_overlay_passability};
use crate::sim::pathfinding::PathGrid;
use crate::sim::power_system::has_active_radar;
use crate::sim::world::Simulation;

// Re-use test helpers from the main production_tests module.
use super::tests::{
    arm_build_via, build_catalog_rules, factory_rules, placement_radius_rules, sell_rules,
    spawn_structure,
};

fn stock_refinery_completion_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=CMIN\n\
         1=HARV\n\
         2=BLOCKER\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=NACNST\n\
         2=GAREFN\n\
         3=NAREFN\n\
         4=NAINDP\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Factory=BuildingType\n\
         [NACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Factory=BuildingType\n\
         [GAREFN]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Refinery=yes\nDockUnload=yes\n\
         FreeUnit=CMIN\n\
         [NAREFN]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Refinery=yes\nDockUnload=yes\n\
         FreeUnit=HARV\n\
         [CMIN]\n\
         Harvester=yes\n\
         Dock=GAREFN\n\
         Cost=1400\n\
         Speed=4\n\
         Storage=20\n\
         [HARV]\n\
         Harvester=yes\n\
         Dock=NAREFN\n\
         Cost=1400\n\
         Speed=4\n\
         Storage=20\n\
         [BLOCKER]\n\
         Cost=1\n\
         Speed=0\n\
         [NAINDP]\n\
         Strength=1000\n\
         FactoryPlant=yes\n\
         UnitsCostBonus=.75\n[Clear]\nBuildable=yes\n",
    );
    let art_ini = IniFile::from_str(
        "[GACNST]\n\
         Foundation=4x4\n\
         [NACNST]\n\
         Foundation=4x4\n\
         [GAREFN]\n\
         Foundation=4x3\n\
         [NAREFN]\n\
         Foundation=4x3\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini)
        .expect("stock refinery completion rules should parse");
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
}

fn ready_and_place(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    ready_building(sim, rules, owner, type_id);
    let owner_id = sim.interner.intern(owner);
    let type_ref = sim.interner.get(type_id).expect("ready type interned");
    assert!(place_production_with_overlays(
        sim,
        rules,
        owner,
        ProductionPlacement::Building {
            type_id: type_id,
            cell: (rx, ry)
        },
        None
    ));
    sim.substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner == owner_id
                && entity.type_ref == type_ref
                && entity.position.rx == rx
                && entity.position.ry == ry
                && entity.category == EntityCategory::Structure
        })
        .map(|entity| entity.stable_id)
        .expect("placed building should exist")
}

fn set_ticks_until_completion(sim: &mut Simulation, stable_id: u64, ticks: u16) {
    let now = sim.session.binary_frame as i32;
    let building = sim
        .substrate
        .entities
        .get_mut(stable_id)
        .expect("placed building should have BuildingUp");
    assert!(
        building.building_up(),
        "placed building should have BuildingUp"
    );
    building.install_building_up(BuildingUp::completing_in_ticks(i32::from(ticks), now), now);
}

#[test]
fn gap_operational_actual_placement_waits_for_build_up_and_next_building_turn() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GACNST\n1=GAGAP\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\nFactory=BuildingType\nBaseNormal=yes\nPower=500\nStrength=1000\nFoundation=1x1\n\
         [GAGAP]\nOwner=Americans,Alliance,Russians,Soviet\nGapGenerator=yes\nGapRadiusInCells=10\nPowered=yes\nPower=-100\nStrength=600\nCost=1000\nFoundation=1x1\n[Clear]\nBuildable=yes\n",
    )).unwrap();
    let mut sim = placement_sim();
    sim.fog.width = 64;
    sim.fog.height = 64;
    let owner = sim.interner.intern("Americans");
    let viewer = sim.interner.intern("Soviet");
    for house in [owner, viewer] {
        sim.houses.insert(
            house,
            crate::sim::house_state::HouseState::new(house, 0, None, true, 10_000, 10),
        );
        sim.session.house_order.push(house);
    }
    sim.fog.reveal_all_for_owner(owner);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let grid = PathGrid::new(64, 64);
    let id = ready_and_place(&mut sim, &rules, "Americans", "GAGAP", 12, 10);
    assert!(sim.substrate.entities.get(id).unwrap().building_up());
    sim.set_logic_order_for_test(vec![1, id]);
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .building_last_operational
    );
    assert!(!sim.fog.is_cell_gap_covered(viewer, 12, 10));
    set_ticks_until_completion(&mut sim, id, 1);
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(!sim.substrate.entities.get(id).unwrap().building_up());
    assert!(
        !sim.fog.is_cell_gap_covered(viewer, 12, 10),
        "late completion is not a gap writer"
    );
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(sim.fog.is_cell_gap_covered(viewer, 12, 10));
}

fn block_building_foundation(
    path_grid: &mut PathGrid,
    rules: &RuleSet,
    type_id: &str,
    rx: u16,
    ry: u16,
) {
    let foundation = &rules
        .object(type_id)
        .expect("building rules should exist")
        .foundation;
    let (width, height) = foundation_dimensions(foundation);
    for y in ry..ry + height {
        for x in rx..rx + width {
            path_grid.set_blocked(x, y, true);
        }
    }
}

fn unit_ids(sim: &Simulation, owner: &str, type_id: &str) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| {
            entity.category == EntityCategory::Unit
                && sim
                    .interner
                    .resolve(entity.owner)
                    .eq_ignore_ascii_case(owner)
                && sim
                    .interner
                    .resolve(entity.type_ref)
                    .eq_ignore_ascii_case(type_id)
        })
        .map(|entity| entity.stable_id)
        .collect()
}

/// Facing byte a FreeUnit placed on the primary cell carries. Under the project
/// convention 0xC0 is west. Spelled out here rather than imported from the code
/// under test: a test that reads the production constant would accept any value
/// that constant is later changed to.
const FREE_UNIT_FACING_PRIMARY: u8 = 0xC0;
/// Facing byte a FreeUnit placed by the nearby-cell search carries — 0xA0,
/// southwest. Same reason as [`FREE_UNIT_FACING_PRIMARY`] for restating it.
const FREE_UNIT_FACING_FALLBACK: u8 = 0xA0;

/// The nearby-cell search's candidate pool for the stock 4x3 refinery fixture
/// below, in the search's own visit order.
///
/// The search is seeded from the building's NORTH-WEST footprint cell — (20,20)
/// here — and walks square Chebyshev rings: segment 1 emits North then South for
/// `d = -r..=r`, segment 2 emits West then East for the interior rows. Ring 0 is
/// the seed itself, which the refinery occupies. On ring 1 the three cells that
/// fall on the 4x3 footprint — (20,21), (21,21) and (21,20) — are refused for the
/// same reason, leaving these five in this order. The fixture grid is flat, so
/// every survivor classifies as a direct candidate and the per-ring early-out
/// stops collection at ring 1: this list IS the pool the frame-counter modulo
/// indexes, not merely a subset of it. On raised ground the direct
/// classification — and with it the early-out — is a recorded residual of the
/// shared search port, so the pool there is wider than this.
///
/// UNCHECKED as parity: which cell gamemd's own search returns for a completed
/// stock refinery has not been derived. This pool is what VERA's search produces,
/// so these tests are a regression ratchet on the search mechanism (seed, ring
/// order, occupancy, frame-counter selection) — NOT evidence that the landing cell
/// matches gamemd's.
const STOCK_4X3_FALLBACK_POOL: [(u16, u16); 5] = [(19, 19), (19, 21), (20, 19), (21, 19), (19, 20)];

/// Build the stock Allied refinery-completion fixture and run the single tick that
/// completes it, returning the simulation and the refinery's stable id.
///
/// `start_frame` pins `session.binary_frame` before anything spawns. The nearby-cell
/// search consumes no RNG and selects `pool[frame % pool.len()]`, so a fixture that
/// leaves the frame implicit is really asserting a cell chosen by however many ticks
/// it happened to run — pinning it is what makes the landing cell a statement about
/// the mechanism instead of about the fixture.
///
/// `extra_blockers` are live ground occupants spawned after the refinery is placed
/// and before it completes. They are invisible to the static path grid, so they are
/// only excluded by a search that reads occupancy at placement time.
fn complete_stock_allied_refinery(
    start_frame: u32,
    extra_blockers: &[(u16, u16)],
) -> (Simulation, u64) {
    let mut sim = placement_sim();
    sim.session.binary_frame = start_frame;
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    block_building_foundation(&mut grid, &rules, "GAREFN", 20, 20);
    for &(rx, ry) in extra_blockers {
        sim.spawn_object("BLOCKER", "Russians", rx, ry, 0, &rules)
            .expect("fixture blocker should spawn");
    }
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(
        completion.spawned_entities,
        "frame {start_frame}: refinery completion should report the free unit"
    );
    (sim, refinery_id)
}

fn resolved_clear_grid_with_override(
    width: u16,
    height: u16,
    mut override_cell: impl FnMut(&mut ResolvedTerrainCell),
) -> ResolvedTerrainGrid {
    let clear_speed_costs = SpeedCostProfile {
        float: Some(0),
        ..crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS
    };
    crate::map::resolved_terrain::test_grid(width, height, |rx, ry| {
        let mut cell = ResolvedTerrainCell {
            speed_costs: clear_speed_costs,
            base_speed_costs: clear_speed_costs,
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        };
        override_cell(&mut cell);
        cell
    })
}

/// Insert a unit already standing in `cell`, without UnitClass::Unlimbo's
/// entry test: a blocker the fixture models as present before the building
/// around it.
fn spawn_standing_blocker(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u64 {
    let terrain = sim.resolved_terrain.take();
    let id = sim
        .spawn_object("BLOCKER", "Russians", cell.0, cell.1, 0, rules)
        .expect("fixture blocker spawns");
    sim.resolved_terrain = terrain;
    id
}

/// A Simulation on 64x64 clear ground inside the playfield, the MapClass
/// inputs placement always reads.
fn placement_sim() -> Simulation {
    let mut sim = Simulation::new();
    install_refinery_test_terrain(&mut sim);
    sim
}

/// Install the MapClass inputs that active UnitClass::Unlimbo always sees.
/// Blocker fixtures are inserted before this call so they model objects already
/// occupying the bay rather than a second constructor placement.
fn install_refinery_test_terrain(sim: &mut Simulation) {
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
}

fn naval_yard_placement_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAYARD\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=1000\n\
         Armor=wood\n\
         Foundation=2x2\n\
         BaseNormal=yes\n\
         Factory=BuildingType\n\
         Adjacent=12\n\
         [GAYARD]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=1500\n\
         Armor=concrete\n\
         Foundation=1x1\n\
         WaterBound=yes\n\
         Naval=yes\n\
         Adjacent=12\n[Clear]\nBuildable=yes\nFloat=0%\n\
         [Water]\nFloat=100%\n[Beach]\nFloat=0%\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GACNST]\nFoundation=2x2\n[GAYARD]\nFoundation=1x1\n"),
    )
    .expect("naval yard placement rules and ART should parse")
}

fn build_off_ally_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAPOWR\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=1000\n\
         Armor=wood\n\
         Foundation=2x2\n\
         BaseNormal=yes\n\
         Factory=BuildingType\n\
         EligibileForAllyBuilding=yes\n\
         [GAPOWR]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=750\n\
         Armor=wood\n\
         Foundation=2x2\n\
         Adjacent=0\n[Clear]\nBuildable=yes\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GACNST]\nFoundation=2x2\n[GAPOWR]\nFoundation=2x2\n"),
    )
    .expect("BuildOffAlly placement rules and ART should parse")
}

fn ground_occupant_placement_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         0=E1\n\
         [VehicleTypes]\n\
         0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAPOWR\n\
         2=GAWALL\n\
         [E1]\n\
         Strength=100\n\
         Armor=flak\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         Speed=4\n\
         [MTNK]\n\
         Strength=300\n\
         Armor=heavy\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=1000\n\
         Armor=wood\n\
         Foundation=2x2\n\
         BaseNormal=yes\n\
         Factory=BuildingType\n\
         [GAPOWR]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=750\n\
         Armor=wood\n\
         Foundation=2x2\n\
         Adjacent=0\n\
         [GAWALL]\n\
         Strength=300\n\
         Armor=concrete\n\
         Foundation=1x1\n\
         Adjacent=0\n\
         Wall=yes\n[Clear]\nBuildable=yes\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str(
            "[GACNST]\nFoundation=2x2\n[GAPOWR]\nFoundation=2x2\n[GAWALL]\nFoundation=1x1\n",
        ),
    )
    .expect("ground-occupant placement rules and ART should parse")
}

fn gsi_04_07_wall_placement_contract() -> (RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAWALL\n\
         2=WALLKIT\n\
         [OverlayTypes]\n\
         0=GASAND\n\
         1=CYCL\n\
         2=GAWALL\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Factory=BuildingType\n\
         Strength=1000\n\
         Armor=wood\n\
         Foundation=2x2\n\
         BaseNormal=yes\n\
         [GASAND]\n\
         Wall=yes\n\
         Armor=wood\n\
         Strength=100\n\
         [CYCL]\n\
         [GAWALL]\n\
         Wall=yes\n\
         Armor=concrete\n\
         Strength=300\n\
         Cost=100\n\
         TechLevel=1\n\
         Foundation=1x1\n\
         Adjacent=8\n\
         GuardRange=5\n\
         [WALLKIT]\n\
         Wall=yes\n\
         Armor=concrete\n\
         Strength=300\n\
         Foundation=1x1\n\
         Adjacent=8\n\
         GuardRange=5\n[Clear]\nBuildable=yes\n",
    );
    let art_ini = IniFile::from_str(
        "[GACNST]\nFoundation=2x2\n[GAWALL]\nToOverlay=GAWALL\n\
         [WALLKIT]\nToOverlay=GAWALL\n",
    );
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).expect("wall placement rules");
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    (rules, OverlayTypeRegistry::from_ini(&ini, None))
}

fn mark_allied(sim: &mut Simulation, a: &str, b: &str) {
    let a = a.to_ascii_uppercase();
    let b = b.to_ascii_uppercase();
    sim.house_alliances
        .entry(a.clone())
        .or_default()
        .insert(b.clone());
    sim.house_alliances.entry(b).or_default().insert(a);
}

fn ready_building(sim: &mut Simulation, rules: &RuleSet, owner: &str, type_id: &str) {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern(type_id);
    let category = rules
        .object(sim.interner.resolve(type_id))
        .map(super::production_tech::production_category_for_object)
        .expect("ready-building test type");
    let cost = sim
        .object_type(type_id, rules)
        .map_or(0, |object| object.cost.max(0));
    let started = sim
        .production
        .factory_shadow
        .test_enqueue_kernel(owner_id, category, type_id, 0, cost);
    assert!(started, "test fixture arms one fresh factory head");
    super::construct_active_factory_fixture(sim, rules, owner_id, category, type_id)
        .expect("ready-building fixture constructs at StartProduction");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner_id, category)
    );
    assert!(
        sim.production
            .factory_shadow
            .account_completed_object_once(owner_id, category),
        "ready projection is already completion-accounted"
    );
    sim.production
        .ready_by_owner
        .insert(owner_id, VecDeque::from([type_id]));
}

fn stock_power_contract_rules() -> RuleSet {
    let fixture = IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=GAPOWR\n\
         2=AMRADR\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\n\
         Strength=1000\n\
         Armor=concrete\n\
         Adjacent=2\n\
         Power=0\n\
         Factory=BuildingType\n\
         [GAPOWR]\nOwner=Americans,Alliance,Russians,Soviet\n\
         BuildCat=Power\n\
         Strength=750\n\
         Armor=wood\n\
         Adjacent=2\n\
         Power=200\n\
         [AMRADR]\nOwner=Americans,Alliance,Russians,Soviet\n\
         BuildCat=Tech\n\
         Strength=600\n\
         Armor=steel\n\
         Adjacent=2\n\
         Power=-50\n\
         Radar=yes\n[Clear]\nBuildable=yes\n",
    );
    RuleSet::from_ini(&fixture).expect("stock power-contract fixture should parse")
}

#[test]
fn completed_building_moves_into_ready_placement_pool() {
    let mut sim = placement_sim();
    let rules = build_catalog_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gacnst = sim.interner.intern("GACNST");
    *super::credits_entry_for_owner(&mut sim, "Americans") = 50_000;
    let built_before = sim.houses[&americans].stats.built();
    // P5d: arm the Building build directly in the registry (queue-of-record), then force it
    // to the completed-held state so Strip publication moves it into the ready-placement pool.
    super::tests::arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "GACNST",
        ProductionCategory::Building,
        1,
    );
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Building)
    );

    publish_production_changes(&mut sim, &rules);
    let held = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Building)
        .expect("completed building remains held by its Factory");
    assert!(held.ready);
    assert!(held.object.and_then(|object| object.entity_id).is_some());
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans")
            .into_iter()
            .map(|item| item.type_id)
            .collect::<Vec<_>>(),
        vec![gacnst]
    );
    let held_id = held.object.unwrap().entity_id.unwrap();
    let rng = sim.scenario_rng.logical_state();
    for _ in 0..3 {
        publish_production_changes(&mut sim, &rules);
    }
    // Record_Last_Built waits for the placement (`0x004FB4B7`).
    assert_eq!(sim.houses[&americans].stats.built(), built_before);
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building),
        held_id
    );
    assert_eq!(
        sim.production.ready_by_owner[&americans]
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![gacnst]
    );
    assert_eq!(sim.sound_events.iter().filter(|event| matches!(event, crate::sim::world::SimSoundEvent::BuildingComplete { owner } if *owner == americans)).count(), 1);
    assert_eq!(sim.scenario_rng.logical_state(), rng);
}

#[test]
fn place_ready_building_spawns_and_consumes_ready_item() {
    let mut sim = placement_sim();
    let rules = build_catalog_rules();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 18, 18);

    let americans = sim.interner.intern("Americans");
    // The house whose Record_Last_Built counts the placement.
    super::credits_entry_for_owner(&mut sim, "Americans");
    ready_building(&mut sim, &rules, "Americans", "GACNST");
    let built_before = sim.houses[&americans].stats.built();
    let held_id = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Building)
        .and_then(|view| view.object.and_then(|object| object.entity_id))
        .expect("Factory+0x58 identity exists before placement");
    super::tests::arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "GACNST",
        ProductionCategory::Building,
        50,
    );
    let mut expected = sim.scenario_rng.clone();
    let successor_word = (expected.next_u32() & 0xffff) as u16;

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GACNST",
            cell: (20, 20)
        },
        None
    ));
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
    assert_eq!(sim.houses[&americans].stats.built(), built_before + 1);

    let structures = sim
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
                    .eq_ignore_ascii_case("GACNST")
                && e.position.rx == 20
                && e.position.ry == 20
                && e.category == crate::map::entities::EntityCategory::Structure
        })
        .count();
    assert_eq!(structures, 1);
    let placed = sim
        .substrate
        .entities
        .get(held_id)
        .expect("same held identity placed");
    assert_eq!((placed.position.rx, placed.position.ry), (20, 20));
    assert!(!placed.lifecycle.in_limbo);
    let successor = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building);
    assert!(successor > held_id);
    assert_eq!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .techno_ctor_random_word,
        successor_word
    );
    assert!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .lifecycle
            .in_limbo
    );
    assert_eq!(
        sim.production
            .factory_shadow
            .view(americans, ProductionCategory::Building)
            .unwrap()
            .progress,
        0
    );
}

/// The frame a GAPOWR (`[0, 26, 2]`) placed by the owner's command in the
/// returned placement frame completes its build-up, through advance_tick.
fn placed_gapowr_completion(human: bool) -> (u32, Option<u32>) {
    let mut sim = placement_sim();
    let mut rules = stock_power_contract_rules();
    rules.set_buildup_control_for_test("GAPOWR", [0, 26, 2]);
    let grid = PathGrid::new(64, 64);
    let americans = sim.interner.intern("Americans");
    sim.houses.insert(
        americans,
        crate::sim::house_state::HouseState::new(americans, 0, None, human, 10_000, 10),
    );
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let gapowr = sim.interner.intern("GAPOWR");
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    let place = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            type_id: gapowr,
            rx: 12,
            ry: 10,
        },
    );
    let placed_frame = sim.session.binary_frame;
    let tick = sim.advance_tick(&[place], Some(&rules), Some(&grid), None, 67);
    assert_eq!(tick.executed_commands, 1);
    let placed = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.type_ref == gapowr)
        .map(|entity| entity.stable_id)
        .expect("the command places GAPOWR");

    let mut completed = None;
    for _ in 0..80 {
        let frame = sim.session.binary_frame;
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        if !sim.substrate.entities.get(placed).unwrap().building_up() {
            completed = Some(frame);
            break;
        }
    }
    (placed_frame, completed)
}

/// A building placed in frame P (the command tail) completes its build-up at
/// P + 2 + (count - 1) * rate from its type's Buildup control
/// (`sim::building_construction`): the PLACE route, an idle frame, then the
/// mission, whichever house sent the event. A computer yard places without
/// one (`sim::ai_base_building::exit_building`). The tactical capture ledger
/// pins the route.
#[test]
fn a_placed_building_completes_its_buildup_after_the_command_frame() {
    for human in [true, false] {
        let (placed, completed) = placed_gapowr_completion(human);
        assert_eq!(completed, Some(placed + 2 + 25 * 2), "human {human}");
    }
}

#[test]
fn stock_gapowr_placement_restores_power_and_radar_during_buildup() {
    let mut sim = placement_sim();
    let rules = stock_power_contract_rules();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "AMRADR", 10, 14);

    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");

    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    let outage = sim
        .power_states
        .get(&americans)
        .expect("stock radar house should have derived power state");
    assert_eq!(outage.total_output, 0);
    assert_eq!(outage.total_drain, 50);
    assert!(outage.is_low_power);
    assert!(
        !has_active_radar(&sim.power_states, americans),
        "stock American radar must be offline during house low power"
    );

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    let place = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            type_id: gapowr,
            rx: 12,
            ry: 10,
        },
    );

    let tick = sim.advance_tick(&[place], Some(&rules), Some(&grid), None, 67);
    assert_eq!(tick.executed_commands, 1);

    let placed = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner == americans
                && entity.type_ref == gapowr
                && entity.position.rx == 12
                && entity.position.ry == 10
        })
        .expect("production command should place stock GAPOWR");
    assert!(
        placed.building_up(),
        "power recovery must occur while the placement buildup is still active"
    );

    // Native Unlimbo requests a house power reassessment, but exact event-vs-
    // House update ordering within the placement command frame remains
    // unverified. Advance to the next guaranteed assessment while buildup is
    // still active rather than certifying an unsupported one-frame claim.
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(
        sim.substrate.entities.values().any(|entity| {
            entity.owner == americans && entity.type_ref == gapowr && entity.building_up()
        }),
        "GAPOWR must still be in its visible buildup during reassessment"
    );

    let recovered = sim
        .power_states
        .get(&americans)
        .expect("post-placement tick should reassess stock power");
    assert_eq!(recovered.total_output, 200);
    assert_eq!(recovered.total_drain, 50);
    assert!(!recovered.is_low_power);
    assert!(
        has_active_radar(&sim.power_states, americans),
        "existing stock American radar should recover while GAPOWR is building up"
    );
}

#[test]
fn place_ready_building_accepts_clear_mixed_height_footprint() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);

    for (cell, z) in [((12, 10), 0), ((13, 10), 1), ((12, 11), 2), ((13, 11), 3)] {
        sim.resolved_terrain
            .as_mut()
            .expect("placement fixture terrain")
            .cell_mut(cell.0, cell.1)
            .expect("footprint cell")
            .level = z;
    }

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("preview should exist");
    assert!(
        preview.valid,
        "mixed clear heights should not reject placement"
    );
    assert!(
        preview.cell_valid.iter().all(|valid| *valid),
        "all otherwise-clear mixed-height cells should be individually valid"
    );

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));

    assert!(sim.substrate.entities.values().any(|e| {
        sim.interner
            .resolve(e.type_ref)
            .eq_ignore_ascii_case("GAPOWR")
            && e.position.rx == 12
            && e.position.ry == 10
    }));
}

#[test]
fn place_ready_building_rejects_blocked_cell_inside_mixed_height_footprint() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if (cell.rx, cell.ry) == (13, 11) {
            cell.yr_cell_land_type = LandType::Rock.as_index();
        }
    }));
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);

    for (cell, z) in [((12, 10), 0), ((13, 10), 1), ((12, 11), 2), ((13, 11), 3)] {
        sim.resolved_terrain
            .as_mut()
            .expect("placement fixture terrain")
            .cell_mut(cell.0, cell.1)
            .expect("footprint cell")
            .level = z;
    }

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        1,
        "blocked placement must not consume the ready building"
    );
}

#[test]
fn stock_refinery_free_unit_spawns_on_building_up_completion_once() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    block_building_foundation(&mut grid, &rules, "GAREFN", 20, 20);
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, refinery_id, 2);

    assert!(unit_ids(&sim, "Americans", "CMIN").is_empty());
    let before_completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(!before_completion.spawned_entities);
    assert!(
        sim.substrate
            .entities
            .get(refinery_id)
            .is_some_and(|entity| entity.building_up())
    );
    assert!(unit_ids(&sim, "Americans", "CMIN").is_empty());

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    let building = sim.substrate.entities.get(refinery_id).unwrap();
    assert!(
        completion.spawned_entities,
        "completion frame{}: mission={:?}/{:?}, status={}, ready={}, body={:?}, stage={:?}, placed={}, free_units={:?}",
        sim.session.binary_frame - 1,
        building.mission.current(),
        building.mission.queued(),
        building.mission.handler_state(),
        building.building_ready_latch(),
        building.building_body_state(),
        building.native_stage(),
        building.building_actually_placed,
        unit_ids(&sim, "Americans", "CMIN")
    );
    assert!(
        sim.substrate
            .entities
            .get(refinery_id)
            .is_some_and(|entity| !entity.building_up())
    );
    assert_eq!(unit_ids(&sim, "Americans", "CMIN").len(), 1);

    let later = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(!later.spawned_entities);
    assert_eq!(unit_ids(&sim, "Americans", "CMIN").len(), 1);
}

#[test]
fn stock_4x3_refinery_free_unit_is_refused_its_footprint_and_placed_by_the_nearby_search() {
    // The primary cell is `(bx + W/2, by + H/2 + 1)`, which for a 4x3 refinery at
    // (20,20) is (22,22) — inside the building's own footprint. The fresh Unit has
    // no radio contact before `UnitClass::Unlimbo`; `Can_Enter_Cell @ 0x0073F0A0`
    // therefore retains the refinery as an ordinary building blocker. The nearby
    // search is the ordinary stock path and uses fallback facing 0xA0.
    //
    // The frame sweep is the point of the loop: selection is `pool[frame % len]` over
    // the ring-ordered pool, so walking the counter must walk the pool and must never
    // leave it.
    const PRIMARY_CELL: (u16, u16) = (22, 22);
    for frame in 0..(STOCK_4X3_FALLBACK_POOL.len() as u32 * 2) {
        let (sim, refinery_id) = complete_stock_allied_refinery(frame, &[]);
        let cmin_ids = unit_ids(&sim, "Americans", "CMIN");
        assert_eq!(
            cmin_ids.len(),
            1,
            "frame {frame}: completion constructs exactly one FreeUnit"
        );
        let miner = sim
            .substrate
            .entities
            .get(cmin_ids[0])
            .expect("the placed FreeUnit should be alive");
        let cell = (miner.position.rx, miner.position.ry);

        assert!(
            sim.substrate
                .occupancy
                .contains_entity(PRIMARY_CELL.0, PRIMARY_CELL.1, refinery_id),
            "frame {frame}: the refinery is the occupant that refuses the primary cell"
        );
        assert_ne!(
            cell, PRIMARY_CELL,
            "frame {frame}: the primary cell sits on the footprint and must be refused"
        );
        assert!(
            !sim.substrate.occupancy.contains_entity(
                PRIMARY_CELL.0,
                PRIMARY_CELL.1,
                miner.stable_id
            ),
            "frame {frame}: a refused primary must leave no occupancy residue"
        );

        assert_eq!(
            cell,
            STOCK_4X3_FALLBACK_POOL[(frame as usize) % STOCK_4X3_FALLBACK_POOL.len()],
            "frame {frame}: the nearby search selects pool[frame % len] in ring order"
        );
        assert!(
            sim.substrate
                .occupancy
                .contains_entity(cell.0, cell.1, miner.stable_id),
            "frame {frame}: the committed fallback cell must be marked"
        );
        assert_eq!(
            miner.body_facing.destination(),
            u16::from(FREE_UNIT_FACING_FALLBACK) << 8,
            "frame {frame}: a placement made by the nearby search uses the fallback facing"
        );
        assert_eq!(miner.mission.current().known(), Some(MissionType::Harvest));
    }
}

#[test]
fn refinery_whose_primary_cell_clears_its_footprint_keeps_the_primary_cell_and_facing() {
    // Guard against "fixed" meaning "the primary attempt was deleted". The same
    // `(bx + W/2, by + H/2 + 1)` arithmetic puts a 1x1 refinery's primary cell one
    // row SOUTH of the building, off its own footprint, where nothing refuses it —
    // and there the free unit is placed on the primary cell with the primary facing
    // and the nearby search never runs. Stock has no 1x1 refinery; this exists to
    // pin the order of the two mechanisms, not a shipping configuration.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=MODHARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=MODPROC\n\
         [GACNST]\nFactory=BuildingType\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Foundation=2x2\n\
         [MODPROC]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Refinery=yes\nDockUnload=yes\n\
         FreeUnit=MODHARV\n\
         Foundation=1x1\n\
         [MODHARV]\n\
         Strength=600\n\
         Harvester=yes\n\
         Dock=MODPROC\n\
         Speed=4\n[Clear]\nBuildable=yes\n",
    ))
    .expect("1x1 refinery rules should parse");
    let mut sim = placement_sim();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 18, 18);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "MODPROC", 20, 20);
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(completion.spawned_entities);
    let free_unit_ids = unit_ids(&sim, "Americans", "MODHARV");
    assert_eq!(free_unit_ids.len(), 1);
    let free_unit = sim
        .substrate
        .entities
        .get(free_unit_ids[0])
        .expect("the placed FreeUnit should be alive");
    assert_eq!(
        (free_unit.position.rx, free_unit.position.ry),
        (20, 21),
        "an admissible primary cell is used as-is; no nearby search runs"
    );
    assert_eq!(
        free_unit.body_facing.destination(),
        u16::from(FREE_UNIT_FACING_PRIMARY) << 8,
        "a primary placement keeps the primary facing"
    );
    assert!(
        sim.substrate
            .occupancy
            .contains_entity(20, 21, free_unit.stable_id)
    );
}

#[test]
fn occupied_primary_bay_uses_one_fallback_without_overlap() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    block_building_foundation(&mut grid, &rules, "GAREFN", 20, 20);
    let blocker_id = spawn_standing_blocker(&mut sim, &rules, (22, 22));
    assert!(sim.substrate.occupancy.contains_entity(22, 22, refinery_id));
    assert!(sim.substrate.occupancy.contains_entity(22, 22, blocker_id));
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(completion.spawned_entities);
    let cmin_ids = unit_ids(&sim, "Americans", "CMIN");
    assert_eq!(
        cmin_ids.len(),
        1,
        "fallback must reuse one constructed unit"
    );
    let miner = sim
        .substrate
        .entities
        .get(cmin_ids[0])
        .expect("fallback miner should remain alive");
    assert_ne!(
        (miner.position.rx, miner.position.ry),
        (22, 22),
        "FreeUnit must not overlap an independent primary-bay blocker"
    );
    assert_eq!(miner.body_facing.destination(), 0xA000);
    assert!(sim.substrate.occupancy.contains_entity(22, 22, blocker_id));
    assert!(
        !sim.substrate
            .occupancy
            .contains_entity(22, 22, miner.stable_id),
        "failed primary Reveal must not mark the miner into occupancy"
    );
}

#[test]
fn live_occupant_on_a_candidate_cell_drops_that_cell_from_the_fallback_pool() {
    // The nearby search runs AFTER the primary placement is refused and reads live
    // occupancy — it is not precomputed off the static path grid before the primary
    // is attempted. A vehicle parked on ring-1 candidate (19,19) is invisible to the
    // path grid but present in occupancy, so only a search that runs at placement
    // time can drop it. Proof: that one cell disappears from the pool and the
    // frame-counter modulo walks the remaining four in unchanged ring order — the
    // whole pool shifts by one entry, which a precomputed or occupancy-blind search
    // could not produce.
    const OCCUPIED_CANDIDATE: (u16, u16) = (19, 19);
    let expected_pool: Vec<(u16, u16)> = STOCK_4X3_FALLBACK_POOL
        .iter()
        .copied()
        .filter(|cell| *cell != OCCUPIED_CANDIDATE)
        .collect();
    assert_eq!(
        expected_pool.len(),
        STOCK_4X3_FALLBACK_POOL.len() - 1,
        "the fixture blocker must sit on exactly one pool entry"
    );

    for frame in 0..(expected_pool.len() as u32 * 2) {
        let (sim, _refinery_id) = complete_stock_allied_refinery(frame, &[OCCUPIED_CANDIDATE]);
        let cmin_ids = unit_ids(&sim, "Americans", "CMIN");
        assert_eq!(
            cmin_ids.len(),
            1,
            "frame {frame}: completion constructs exactly one FreeUnit"
        );
        let miner = sim
            .substrate
            .entities
            .get(cmin_ids[0])
            .expect("the placed FreeUnit should be alive");
        let cell = (miner.position.rx, miner.position.ry);

        assert_ne!(
            cell, OCCUPIED_CANDIDATE,
            "frame {frame}: an occupied candidate must never be selected"
        );
        assert_eq!(
            cell,
            expected_pool[(frame as usize) % expected_pool.len()],
            "frame {frame}: the shortened pool keeps ring order and is walked by the frame counter"
        );
        assert_eq!(
            miner.body_facing.destination(),
            u16::from(FREE_UNIT_FACING_FALLBACK) << 8
        );
        assert!(
            !sim.substrate.occupancy.contains_entity(
                OCCUPIED_CANDIDATE.0,
                OCCUPIED_CANDIDATE.1,
                miner.stable_id
            ),
            "frame {frame}: the rejected candidate must hold no occupancy residue"
        );
    }
}

/// The refund is the FreeUnit's GetRefund with `full` set, its Cost_Of, which
/// the house's Industrial Plant discounts: 1400 * .75.
#[test]
fn free_unit_total_placement_failure_refunds_once_and_leaves_no_entity() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    spawn_standing_blocker(&mut sim, &rules, (22, 22));
    for ry in 0..64 {
        for rx in 0..64 {
            grid.set_blocked(rx, ry, true);
        }
    }
    *super::credits_entry_for_owner(&mut sim, "Americans") = 100;
    sim.spawn_object("NAINDP", "Americans", 40, 40, 0, &rules)
        .expect("the Industrial Plant unlimbos");
    let americans = sim.interner.get("Americans").expect("owner should exist");
    let owned_units_before = sim.owned_object_counts(americans).1;
    install_refinery_test_terrain(&mut sim);
    for ry in 0..64 {
        for rx in 0..64 {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .and_then(|terrain| terrain.cell_mut(rx, ry))
                .expect("fixture terrain cell");
            cell.speed_costs = SpeedCostProfile::default();
            cell.base_speed_costs = SpeedCostProfile::default();
        }
    }
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(
        !completion.spawned_entities,
        "a constructed-then-destroyed FreeUnit is not a successful spawn"
    );
    assert!(unit_ids(&sim, "Americans", "CMIN").is_empty());
    assert!(
        !sim.substrate.entities.values().any(|entity| sim
            .interner
            .resolve(entity.type_ref)
            .eq_ignore_ascii_case("CMIN")),
        "same-tick pending-delete drain must leave no living or limbo CMIN"
    );
    assert_eq!(credits_for_owner(&sim, "Americans"), 1150);
    assert_eq!(
        sim.owned_object_counts(americans).1,
        owned_units_before,
        "the discarded FreeUnit must not stay counted"
    );

    let later = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(!later.spawned_entities);
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        1150,
        "consumed BuildingUp transition must not refund twice"
    );
}

#[test]
fn stock_soviet_refinery_completion_spawns_harv() {
    // The Soviet refinery is the same 4x3 shape as the Allied one, so its primary
    // cell (22,22) also lands on its own occupied footprint; only the FreeUnit type
    // differs. The deliberately nonzero frame proves the same ordered fallback pool
    // is used rather than always selecting its first entry.
    const SELECTION_FRAME: u32 = 3;
    let mut sim = placement_sim();
    sim.session.binary_frame = SELECTION_FRAME;
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Russians", "NACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Russians", "NAREFN", 20, 20);
    block_building_foundation(&mut grid, &rules, "NAREFN", 20, 20);
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(completion.spawned_entities);
    let harvester_id = unit_ids(&sim, "Russians", "HARV")
        .into_iter()
        .next()
        .expect("stock Soviet refinery should spawn HARV");
    let harvester = sim
        .substrate
        .entities
        .get(harvester_id)
        .expect("spawned HARV should exist");
    assert_ne!(
        (harvester.position.rx, harvester.position.ry),
        (22, 22),
        "the primary cell sits on the NAREFN footprint and must be refused"
    );
    assert_eq!(
        (harvester.position.rx, harvester.position.ry),
        STOCK_4X3_FALLBACK_POOL[(SELECTION_FRAME as usize) % STOCK_4X3_FALLBACK_POOL.len()],
        "the Soviet refinery walks the same ring-ordered pool as the Allied one"
    );
    assert_eq!(
        harvester.body_facing.destination(),
        u16::from(FREE_UNIT_FACING_FALLBACK) << 8
    );
    assert_eq!(
        harvester.mission.current().known(),
        Some(MissionType::Harvest)
    );
    assert!(unit_ids(&sim, "Russians", "CMIN").is_empty());
}

#[test]
fn non_refinery_completion_has_no_free_unit_or_credit_side_effect() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let grid = PathGrid::new(64, 64);
    let construction_yard_id = sim
        .spawn_object("GACNST", "Americans", 20, 20, 0, &rules)
        .expect("construction yard should spawn");
    sim.substrate
        .entities
        .get_mut(construction_yard_id)
        .expect("construction yard should exist")
        .install_building_up(BuildingUp::completing_in_ticks(1, 0), 0);
    let credits_before = credits_for_owner(&sim, "Americans");

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(!completion.spawned_entities);
    assert!(unit_ids(&sim, "Americans", "CMIN").is_empty());
    assert!(unit_ids(&sim, "Americans", "HARV").is_empty());
    assert_eq!(credits_for_owner(&sim, "Americans"), credits_before);
}

#[test]
fn simultaneous_refinery_completions_preserve_stable_id_order() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 14, 20);
    spawn_structure(&mut sim, 2, "Russians", "NACNST", 14, 35);
    let allied_refinery = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    let soviet_refinery = ready_and_place(&mut sim, &rules, "Russians", "NAREFN", 20, 35);
    assert!(allied_refinery < soviet_refinery);
    block_building_foundation(&mut grid, &rules, "GAREFN", 20, 20);
    block_building_foundation(&mut grid, &rules, "NAREFN", 20, 35);
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, allied_refinery, 1);
    set_ticks_until_completion(&mut sim, soviet_refinery, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(completion.spawned_entities);
    let cmin = unit_ids(&sim, "Americans", "CMIN");
    let harv = unit_ids(&sim, "Russians", "HARV");
    assert_eq!(cmin.len(), 1);
    assert_eq!(harv.len(), 1);
    assert!(
        cmin[0] < harv[0],
        "FreeUnits must allocate in completed-building stable-ID order"
    );
}

#[test]
fn modded_refinery_completion_uses_free_unit_from_rules() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=MODHARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=MODPROC\n\
         [GACNST]\nFactory=BuildingType\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Foundation=2x2\n\
         [MODPROC]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Refinery=yes\nDockUnload=yes\n\
         FreeUnit=MODHARV\n\
         Foundation=3x3\n\
         [MODHARV]\n\
         Strength=600\n\
         Harvester=yes\n\
         Dock=MODPROC\n\
         Speed=4\n[Clear]\nBuildable=yes\n",
    ))
    .expect("rules should parse");
    let mut sim = placement_sim();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 18, 18);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "MODPROC", 20, 20);
    assert!(unit_ids(&sim, "Americans", "MODHARV").is_empty());
    install_refinery_test_terrain(&mut sim);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(completion.spawned_entities);
    assert_eq!(unit_ids(&sim, "Americans", "MODHARV").len(), 1);
}

#[test]
fn refinery_without_free_unit_spawns_nothing_on_completion() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=MODHARV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=GACNST\n\
         1=MODPROC\n\
         [GACNST]\nFactory=BuildingType\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Foundation=2x2\n\
         [MODPROC]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\n\
         Refinery=yes\nDockUnload=yes\n\
         Foundation=3x3\n\
         [MODHARV]\n\
         Strength=600\n\
         Harvester=yes\n\
         Dock=MODPROC\n\
         Speed=4\n[Clear]\nBuildable=yes\n",
    ))
    .expect("rules should parse");
    let mut sim = placement_sim();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 18, 18);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "MODPROC", 20, 20);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    assert!(
        !sim.substrate
            .entities
            .get(refinery_id)
            .unwrap()
            .building_up(),
        "the no-FreeUnit control must actually complete construction"
    );
    assert!(!completion.spawned_entities);
    assert!(unit_ids(&sim, "Americans", "MODHARV").is_empty());
}

#[test]
fn place_ready_building_rejects_blocked_or_overlapping_cells() {
    let mut sim = placement_sim();
    let rules = build_catalog_rules();
    let mut grid = PathGrid::new(64, 64);
    grid.set_blocked(31, 31, true);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 30, 30);
    spawn_structure(&mut sim, 2, "Americans", "GACNST", 40, 40);

    let americans = sim.interner.intern("Americans");
    let gacnst = sim.interner.intern("GACNST");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gacnst, gacnst]));

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GACNST",
            cell: (31, 31)
        },
        None
    ));
    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GACNST",
            cell: (40, 40)
        },
        None
    ));
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        2,
        "invalid placement must not consume the ready building"
    );
}

#[test]
fn placement_command_rejects_marked_ground_mobiles_until_they_are_unmarked() {
    let mut rules = ground_occupant_placement_rules();
    // A retained death has real ART and a native sequence clock. An absent
    // death record retires on the first class visit, before this command.
    let death_art = IniFile::from_str("[E1]\nSequence=E1Sequence\n[E1Sequence]\nDie1=56,15,0\n");
    rules.install_art_data(ArtRegistry::from_ini(&death_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&death_art),
    );
    let grid = PathGrid::new(64, 64);

    for blocker_type in ["MTNK", "E1"] {
        let mut sim = placement_sim();
        spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
        let blocker_id = sim
            .spawn_object(blocker_type, "Americans", 13, 11, 0, &rules)
            .expect("real spawn/unlimbo should mark the mobile occupant");
        assert!(
            sim.substrate.occupancy.contains_entity(13, 11, blocker_id),
            "{blocker_type} must enter the authoritative Ground object list"
        );

        // A marked native death sequence continues to block until UnInit.
        if blocker_type == "E1" {
            sim.substrate
                .entities
                .get_mut(blocker_id)
                .unwrap()
                .health
                .current = 0;
            sim.begin_infantry_death_sequence(
                blocker_id,
                crate::sim::world::InfantryDeathSequence::Die1,
                &rules,
            );
        }

        ready_building(&mut sim, &rules, "Americans", "GAPOWR");
        let preview = placement_preview_for_owner_without_overlays(
            &sim,
            &rules,
            "Americans",
            "GAPOWR",
            12,
            10,
        )
        .expect("ready building should have a preview");
        assert!(!preview.valid, "{blocker_type} must reject the preview");
        assert_eq!(
            preview.cell_valid,
            vec![true, true, true, false],
            "only the non-origin foundation cell occupied by {blocker_type} should reject"
        );

        let americans = sim.interner.get("Americans").expect("owner interned");
        let gapowr = sim.interner.get("GAPOWR").expect("type interned");
        let entities_before = sim.substrate.entities.len();
        let next_id_before = sim.substrate.next_stable_object_id;
        let occupancy_generation_before = sim.substrate.occupancy.generation();
        let rejected = CommandEnvelope::new(
            americans,
            sim.session.tick + 1,
            Command::PlaceReadyBuilding {
                type_id: gapowr,
                rx: 12,
                ry: 10,
            },
        );
        let tick = sim.advance_tick(&[rejected], Some(&rules), Some(&grid), None, 67);

        assert_eq!(tick.executed_commands, 1);
        assert!(
            !tick.spawned_entities,
            "rejected placement must not report a spawned entity"
        );
        // `HouseClass::Place_Production 0x004FB369..0x004FB377`: the failed
        // Unlimbo speaks `EVA_CannotDeployHere` for the placing house.
        assert!(
            sim.sound_events.iter().any(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::CannotDeployHere { owner } if *owner == americans
            )),
            "rejected placement must emit EVA_CannotDeployHere for {blocker_type}"
        );
        assert_eq!(sim.substrate.entities.len(), entities_before);
        assert_eq!(sim.substrate.next_stable_object_id, next_id_before);
        assert_eq!(
            sim.substrate.occupancy.generation(),
            occupancy_generation_before,
            "rejected placement must not mutate CellClass-style membership"
        );
        assert_eq!(
            ready_buildings_for_owner(&sim, &rules, "Americans").len(),
            1,
            "rejected placement must preserve the ready building"
        );

        // Mark(UP) unlinks either blocker, and the tank's clears its 0x20.
        // The infantryman's sub-cell bit is left to his Limbo: `FootClass::
        // Limbo` (`0x004DB260`) has Walk release it through Infantry vt+0xF4.
        let _ = sim.techno_limbo_with_rules(blocker_id, &rules, None);
        assert!(
            !sim.substrate.occupancy.contains_entity(13, 11, blocker_id),
            "Limbo must remove the blocker before placement becomes legal"
        );
        let preview = placement_preview_for_owner_without_overlays(
            &sim,
            &rules,
            "Americans",
            "GAPOWR",
            12,
            10,
        )
        .expect("ready building should retain its preview after rejection");
        assert!(
            preview.valid,
            "the same foundation must become legal after {blocker_type} is unmarked"
        );

        let accepted = CommandEnvelope::new(
            americans,
            sim.session.tick + 1,
            Command::PlaceReadyBuilding {
                type_id: gapowr,
                rx: 12,
                ry: 10,
            },
        );
        let tick = sim.advance_tick(&[accepted], Some(&rules), Some(&grid), None, 67);
        assert!(tick.spawned_entities);
        assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
        assert!(sim.substrate.entities.values().any(|entity| {
            entity.type_ref == gapowr
                && entity.position.rx == 12
                && entity.position.ry == 10
                && entity.building_up()
        }));
    }
}

#[test]
fn placement_command_rejects_nonblocking_overlay_and_preserves_ready_building() {
    let mut sim = placement_sim();
    let rules = ground_occupant_placement_rules();
    let grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);

    let mut overlay_grid = OverlayGrid::new(64, 64);
    overlay_grid.place_overlay(13, 11, 7, 4);
    sim.overlay_grid = Some(overlay_grid);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("ready building should have a preview");
    assert!(!preview.valid, "any ordinary nonempty overlay must reject");
    assert_eq!(preview.cell_valid, vec![true, true, true, false]);

    let americans = sim.interner.get("Americans").expect("owner interned");
    let gapowr = sim.interner.get("GAPOWR").expect("type interned");
    let entities_before = sim.substrate.entities.len();
    let next_id_before = sim.substrate.next_stable_object_id;
    let occupancy_generation_before = sim.substrate.occupancy.generation();
    let rejected = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            type_id: gapowr,
            rx: 12,
            ry: 10,
        },
    );
    let tick = sim.advance_tick(&[rejected], Some(&rules), Some(&grid), None, 67);

    assert_eq!(tick.executed_commands, 1);
    assert!(!tick.spawned_entities);
    assert_eq!(sim.substrate.entities.len(), entities_before);
    assert_eq!(sim.substrate.next_stable_object_id, next_id_before);
    assert_eq!(
        sim.substrate.occupancy.generation(),
        occupancy_generation_before
    );
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        1,
        "rejected overlay placement must preserve the ready building"
    );
    let overlay = sim
        .overlay_grid
        .as_ref()
        .expect("overlay grid retained")
        .cell(13, 11);
    assert_eq!((overlay.overlay_id, overlay.overlay_data), (Some(7), 4));

    sim.overlay_grid
        .as_mut()
        .expect("overlay grid retained")
        .clear_overlay(13, 11);
    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("ready building should retain its preview after rejection");
    assert!(
        preview.valid,
        "the same foundation must become legal after the overlay is cleared"
    );

    let accepted = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            type_id: gapowr,
            rx: 12,
            ry: 10,
        },
    );
    let tick = sim.advance_tick(&[accepted], Some(&rules), Some(&grid), None, 67);
    assert!(tick.spawned_entities);
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
}

#[test]
fn empty_cell_wall_placement_still_works_but_wall_on_overlay_rejects() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();

    let mut clear_sim = placement_sim();
    spawn_structure(&mut clear_sim, 1, "Americans", "GACNST", 10, 10);
    clear_sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut clear_sim, &rules, "Americans", "GAWALL");
    let preview = placement_preview_for_owner_with_overlays(
        &clear_sim,
        &rules,
        "Americans",
        "GAWALL",
        12,
        10,
        Some(&registry),
    )
    .expect("ready wall should have a preview");
    assert!(
        preview.valid,
        "the ordinary empty-cell wall preview must remain accepted: {:?}",
        preview.reason
    );
    assert!(
        place_production_with_overlays(
            &mut clear_sim,
            &rules,
            "Americans",
            ProductionPlacement::Building {
                type_id: "GAWALL",
                cell: (12, 10)
            },
            Some(&registry)
        ),
        "the ordinary empty-cell wall commit must remain accepted"
    );

    let mut overlay_sim = placement_sim();
    spawn_structure(&mut overlay_sim, 1, "Americans", "GACNST", 10, 10);
    overlay_sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
    let mut overlay_grid = OverlayGrid::new(64, 64);
    overlay_grid.place_overlay(12, 10, 7, 4);
    overlay_sim.overlay_grid = Some(overlay_grid);
    ready_building(&mut overlay_sim, &rules, "Americans", "GAWALL");
    let placement_credits = 1_337;
    *super::credits_entry_for_owner(&mut overlay_sim, "Americans") = placement_credits;
    assert!(overlay_sim.rebuild_dynamic_navigation(&rules));
    let owner = overlay_sim.interner.get("Americans").expect("owner");
    let wall = rules.object("GAWALL").expect("wall rules");
    assert_eq!(wall.cost, 100, "fixture must exercise a nonzero wall cost");
    let category = super::production_tech::production_category_for_object(wall);
    let factory_before = {
        let view = overlay_sim
            .production
            .factory_shadow
            .view(owner, category)
            .expect("completed wall factory");
        (
            view.progress,
            view.on_hold,
            view.suspended,
            view.object.cloned(),
            view.queue.clone(),
            view.ready,
        )
    };
    let held_id = factory_before
        .3
        .as_ref()
        .and_then(|object| object.entity_id)
        .expect("completed wall retains Factory+0x58 identity");
    let ready_before = overlay_sim
        .production
        .ready_by_owner
        .get(&owner)
        .cloned()
        .expect("ready queue");
    let overlay_before = *overlay_sim
        .overlay_grid
        .as_ref()
        .expect("overlay grid")
        .cell(12, 10);
    let navigation_before = overlay_sim
        .path_grid_snapshot()
        .expect("published navigation before rejected placement");
    let preview = placement_preview_for_owner_with_overlays(
        &overlay_sim,
        &rules,
        "Americans",
        "GAWALL",
        12,
        10,
        Some(&registry),
    )
    .expect("ready wall should have a preview");
    assert!(!preview.valid, "an ordinary wall must not replace ore");
    assert!(
        !place_production_with_overlays(
            &mut overlay_sim,
            &rules,
            "Americans",
            ProductionPlacement::Building {
                type_id: "GAWALL",
                cell: (12, 10)
            },
            Some(&registry)
        ),
        "the occupied primary wall commit must be rejected"
    );
    assert_eq!(
        overlay_sim.production.ready_by_owner.get(&owner),
        Some(&ready_before),
        "rejection must preserve the completed wall product"
    );
    let factory_after = {
        let view = overlay_sim
            .production
            .factory_shadow
            .view(owner, category)
            .expect("rejected placement retains completed wall factory");
        (
            view.progress,
            view.on_hold,
            view.suspended,
            view.object.cloned(),
            view.queue.clone(),
            view.ready,
        )
    };
    assert_eq!(
        factory_after, factory_before,
        "rejection must preserve the authoritative completed factory object"
    );
    assert!(
        overlay_sim.substrate.entities.contains(held_id),
        "rejection must preserve the held Factory+0x58 entity"
    );
    assert_eq!(
        credits_for_owner(&overlay_sim, "Americans"),
        placement_credits,
        "rejection must preserve house credits"
    );
    assert_eq!(
        overlay_sim
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(12, 10),
        &overlay_before,
        "rejection must preserve the occupied overlay cell"
    );
    assert_eq!(
        overlay_sim.path_grid_snapshot().as_deref(),
        Some(navigation_before.as_ref()),
        "rejection must preserve the complete published navigation grid"
    );
}

#[test]
fn gsi_04_07_command_places_authoritative_owned_wall_without_entity() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let path_grid = PathGrid::new(64, 64);
    let mut sim = placement_sim();
    *super::credits_entry_for_owner(&mut sim, "Americans") = 50_000;
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").expect("owner");
    let type_id = sim.interner.get("GAWALL").expect("wall type");
    let category =
        super::production_tech::production_category_for_object(rules.object("GAWALL").unwrap());
    let held = super::lifecycle_tests::held_id(&sim, owner, category);
    // A second wall waits behind the finished one, as when VERA queued it while the
    // first was still building (gamemd never queues a building: see enqueue_by_type's
    // building-path residual). A PRODUCE sent now would take the finished head's
    // same-type branch instead (0x004FA5A8..0x004FA5C4), so the fixture appends it.
    let cost = rules.object("GAWALL").unwrap().cost.max(0);
    assert!(
        !sim.production
            .factory_shadow
            .test_enqueue_kernel(owner, category, type_id, 1, cost)
    );
    assert!(matches!(
        super::production_tech::revalidate_eligibility(&sim, &rules, "Americans", "GAWALL"),
        super::factory::BuildEligibility::Buildable
    ));
    let mut expected = sim.scenario_rng.clone();
    let successor_word = (expected.next_u32() & 0xffff) as u16;
    let entities_before = sim.substrate.entities.len();

    let tick = sim.advance_tick(
        &[CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::PlaceReadyBuilding {
                type_id,
                rx: 12,
                ry: 10,
            },
        )],
        Some(&rules),
        Some(&path_grid),
        Some(&registry),
        67,
    );

    assert_eq!(tick.executed_commands, 1);
    assert!(
        !tick.spawned_entities,
        "wall stamps no BuildingClass entity"
    );
    assert_eq!(
        sim.substrate.entities.len(),
        entities_before,
        "wall consumes one held identity and constructs exactly one successor"
    );
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
    let cell = sim.overlay_grid.as_ref().unwrap().cell(12, 10);
    assert_eq!(cell.overlay_id, Some(2));
    assert_eq!(cell.overlay_data, 0);
    assert_eq!(cell.wall_owner, Some(owner));
    assert!(!sim.substrate.entities.values().any(|entity| {
        entity.type_ref == type_id && (entity.position.rx, entity.position.ry) == (12, 10)
    }));
    assert_eq!(tick.state_hash, sim.state_hash());
    assert!(!sim.substrate.entities.contains(held));
    let successor = super::lifecycle_tests::held_id(&sim, owner, category);
    assert!(successor > held);
    assert_eq!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .techno_ctor_random_word,
        successor_word
    );
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, category)
            .unwrap()
            .progress,
        0
    );
}

#[test]
fn gsi_04_07_regular_wall_autofill_is_cardinal_ordered_bounded_and_consumes_once() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let placement_credits = 1_337;
    *super::credits_entry_for_owner(&mut sim, "Americans") = placement_credits;
    let owner = sim.interner.get("Americans").expect("owner");
    let wall_type = sim.interner.get("GAWALL").expect("wall type");
    let wall = rules.object("GAWALL").expect("wall rules");
    assert_eq!(wall.cost, 100, "fixture must exercise a nonzero wall cost");
    let category = super::production_tech::production_category_for_object(wall);
    let held_id = sim
        .production
        .factory_shadow
        .view(owner, category)
        .and_then(|view| view.object)
        .filter(|object| object.type_id == wall_type)
        .and_then(|object| object.entity_id)
        .expect("completed wall retains Factory+0x58 identity");
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        1,
        "fixture must begin with one authoritative completed wall"
    );
    super::tests::arm_build_via(&mut sim, &rules, "Americans", "GAWALL", category, 50);
    let mut expected_rng = sim.scenario_rng.clone();
    let successor_word = (expected_rng.next_u32() & 0xffff) as u16;
    let overlay_id = registry.id_for_name("GAWALL").expect("wall overlay");
    let origin = (18, 18);
    let endpoints = [(18, 13), (23, 18), (18, 23), (13, 18)];
    for (rx, ry) in endpoints {
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_owned_wall(rx, ry, overlay_id, 0x20, owner);
    }

    let expected = vec![
        (18, 17),
        (18, 16),
        (18, 15),
        (18, 14),
        (19, 18),
        (20, 18),
        (21, 18),
        (22, 18),
        (18, 19),
        (18, 20),
        (18, 21),
        (18, 22),
        (17, 18),
        (16, 18),
        (15, 18),
        (14, 18),
    ];
    let preview = placement_preview_for_owner_with_overlays(
        &sim,
        &rules,
        "Americans",
        "GAWALL",
        origin.0,
        origin.1,
        Some(&registry),
    )
    .expect("ready wall preview");
    assert!(preview.valid, "primary wall cell should be legal");
    assert_eq!(preview.wall_autofill_cells, expected);

    sim.tactical_dirty_cells.clear();
    sim.radar_terrain_dirty_cells.clear();
    sim.radar_terrain_dirty_generation = 0;

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWALL",
            cell: (origin.0, origin.1)
        },
        Some(&registry)
    ));
    assert!(
        ready_buildings_for_owner(&sim, &rules, "Americans").is_empty(),
        "the primary plus all fillers consume the one ready product"
    );
    let successor = super::lifecycle_tests::held_id(&sim, owner, category);
    assert!(successor > held_id);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, category)
            .unwrap()
            .progress,
        0
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .techno_ctor_random_word,
        successor_word
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    assert!(
        sim.substrate.entities.get(held_id).is_none(),
        "wall placement must destroy the consumed Factory+0x58 entity"
    );
    assert_eq!(
        credits_for_owner(&sim, "Americans"),
        placement_credits,
        "the paid primary plus free fillers must not debit credits at placement time"
    );
    let grid = sim.overlay_grid.as_ref().expect("overlay grid");
    for (rx, ry) in std::iter::once(origin).chain(expected.iter().copied()) {
        let cell = grid.cell(rx, ry);
        assert_eq!(
            cell.overlay_id,
            Some(overlay_id),
            "missing wall at ({rx},{ry})"
        );
        assert_eq!(
            cell.wall_owner,
            Some(owner),
            "wrong wall owner at ({rx},{ry})"
        );
    }
    for (rx, ry) in endpoints {
        assert_ne!(
            grid.cell(rx, ry).overlay_data & 0x0F,
            0,
            "endpoint connectivity should refresh at ({rx},{ry})"
        );
    }
    assert!(
        sim.zone_grid.is_some(),
        "wall placement must publish navigation"
    );
    let mut expected_tactical = Vec::new();
    for (rx, ry) in std::iter::once(origin).chain(expected.iter().copied()) {
        expected_tactical.extend([
            (rx, ry - 1),
            (rx + 1, ry),
            (rx, ry + 1),
            (rx - 1, ry),
            (rx, ry),
        ]);
    }
    assert_eq!(
        sim.tactical_dirty_cells, expected_tactical,
        "each clicked/filler Mark must finish its N/E/S/W/self tactical sequence before the next stamp"
    );
    for (rx, ry) in std::iter::once(origin).chain(expected.iter().copied()) {
        assert!(
            sim.resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(rx, ry))
                .is_some_and(|cell| cell.overlay_blocks),
            "resolved passability must include wall at ({rx},{ry})"
        );
    }
}

#[test]
fn gsi_04_07_regular_wall_autofill_rejects_out_of_range_and_foreign_endpoints() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let overlay_id = registry.id_for_name("GAWALL").expect("wall overlay");

    let mut out_of_range = placement_sim();
    spawn_structure(&mut out_of_range, 1, "Americans", "GACNST", 10, 10);
    out_of_range.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut out_of_range, &rules, "Americans", "GAWALL");
    let owner = out_of_range.interner.get("Americans").expect("owner");
    out_of_range
        .overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(24, 18, overlay_id, 0x20, owner);
    let preview = placement_preview_for_owner_with_overlays(
        &out_of_range,
        &rules,
        "Americans",
        "GAWALL",
        18,
        18,
        Some(&registry),
    )
    .expect("ready wall preview");
    assert!(
        preview.wall_autofill_cells.is_empty(),
        "GuardRange=5 must not close an endpoint six cells away"
    );

    let mut foreign_blocker = placement_sim();
    spawn_structure(&mut foreign_blocker, 1, "Americans", "GACNST", 10, 10);
    foreign_blocker.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut foreign_blocker, &rules, "Americans", "GAWALL");
    let owner = foreign_blocker.interner.get("Americans").expect("owner");
    let enemy = foreign_blocker.interner.intern("Russians");
    let grid = foreign_blocker.overlay_grid.as_mut().expect("overlay grid");
    grid.place_owned_wall(20, 18, overlay_id, 0x20, enemy);
    grid.place_owned_wall(23, 18, overlay_id, 0x20, owner);
    let preview = placement_preview_for_owner_with_overlays(
        &foreign_blocker,
        &rules,
        "Americans",
        "GAWALL",
        18,
        18,
        Some(&registry),
    )
    .expect("ready wall preview");
    assert!(
        preview.wall_autofill_cells.is_empty(),
        "a foreign wall must block, not terminate, the direction"
    );
    assert!(place_production_with_overlays(
        &mut foreign_blocker,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWALL",
            cell: (18, 18)
        },
        Some(&registry)
    ));
    assert_eq!(
        foreign_blocker
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(19, 18)
            .overlay_id,
        None,
        "a blocked direction must not leave a partial filler"
    );
}

#[test]
fn gsi_04_07_wall_placement_resolves_art_tooverlay_not_building_id() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "WALLKIT");
    assert!(registry.id_for_name("WALLKIT").is_none());

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "WALLKIT",
            cell: (12, 10)
        },
        Some(&registry)
    ));
    let wall = sim
        .overlay_grid
        .as_ref()
        .expect("overlay grid")
        .cell(12, 10);
    assert_eq!(wall.overlay_id, registry.id_for_name("GAWALL"));
    assert_eq!(wall.wall_owner, sim.interner.get("Americans"));
}

#[test]
fn gsi_04_07_wall_execution_recomputes_preview_gap_after_a_blocker_appears() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").expect("owner");
    let overlay_id = registry.id_for_name("GAWALL").expect("wall overlay");
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(23, 18, overlay_id, 0x20, owner);
    let preview = placement_preview_for_owner_with_overlays(
        &sim,
        &rules,
        "Americans",
        "GAWALL",
        18,
        18,
        Some(&registry),
    )
    .expect("ready wall preview");
    assert_eq!(
        preview.wall_autofill_cells,
        vec![(19, 18), (20, 18), (21, 18), (22, 18)]
    );

    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_overlay(20, 18, 7, 4);
    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWALL",
            cell: (18, 18)
        },
        Some(&registry)
    ));
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(19, 18)
            .overlay_id,
        None,
        "execution must rescan instead of trusting the earlier preview cells"
    );
}

#[test]
fn gsi_04_07_wall_placement_publishes_connectivity_neighbor_auto_destruction() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").expect("owner");
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(13, 10, 0, 0x20, owner);
    assert!(recalc_overlay_passability(
        sim.overlay_grid.as_mut().expect("overlay grid"),
        sim.resolved_terrain.as_mut().expect("resolved terrain"),
        &registry,
        13,
        10,
    ));
    sim.substrate
        .entities
        .get_mut(1)
        .expect("construction yard listener")
        .attack_target = Some(AttackTarget::for_cell(13, 10));
    sim.tactical_dirty_cells.clear();
    sim.radar_terrain_dirty_cells.clear();
    sim.radar_terrain_dirty_generation = 0;

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWALL",
            cell: (12, 10)
        },
        Some(&registry)
    ));
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(13, 10)
            .overlay_id,
        None,
        "placement connectivity refresh should auto-destroy the damaged isolated neighbor"
    );
    assert!(
        sim.resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(13, 10))
            .is_some_and(|cell| !cell.overlay_blocks),
        "neighbor removal must be published to resolved passability immediately"
    );
    assert!(
        sim.zone_grid.is_some(),
        "neighbor removal must publish zones"
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .expect("construction yard listener")
            .attack_target
            .is_none(),
        "cleanup removal must broadcast CellClass pointer expiry before returning"
    );
    let expected_dirty = vec![(12, 9), (13, 10), (12, 11), (11, 10), (12, 10)];
    assert_eq!(sim.tactical_dirty_cells, expected_dirty);
    assert_eq!(sim.radar_terrain_dirty_cells, expected_dirty);
    assert_eq!(
        sim.radar_terrain_dirty_generation, 5,
        "each first-unique native radar callback is published at its inline visit"
    );
    let live_path = sim
        .path_grid_snapshot()
        .expect("hosted placement path authority");
    assert!(
        live_path.is_walkable(13, 10),
        "removed neighbor must be passable before placement returns"
    );
    assert!(
        !live_path.is_walkable(12, 10),
        "new anchor must block before placement returns"
    );
}

/// A placement event places its own house's ready building: the Americans'
/// event cannot place the Russians' wall.
#[test]
fn gsi_04_07_placement_command_places_only_its_own_houses_production() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let path_grid = PathGrid::new(64, 64);
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Russians", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Russians", "GAWALL");
    let event_owner = sim.interner.intern("Americans");
    let wall_type = sim.interner.get("GAWALL").expect("wall type");

    let tick = sim.advance_tick(
        &[CommandEnvelope::new(
            event_owner,
            sim.session.tick + 1,
            Command::PlaceReadyBuilding {
                type_id: wall_type,
                rx: 12,
                ry: 10,
            },
        )],
        Some(&rules),
        Some(&path_grid),
        Some(&registry),
        67,
    );

    assert_eq!(tick.executed_commands, 1, "the due event was dispatched");
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(12, 10)
            .overlay_id,
        None
    );
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Russians").len(),
        1,
        "another house's event must not consume production"
    );
    assert_eq!(tick.state_hash, sim.state_hash());
}

#[test]
fn gsi_04_07_wall_replacement_requires_damaged_same_type_and_owner_and_stays_local() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").expect("owner");
    let enemy = sim.interner.intern("Russians");

    let preview = |sim: &Simulation| {
        placement_preview_for_owner_with_overlays(
            sim,
            &rules,
            "Americans",
            "GAWALL",
            12,
            10,
            Some(&registry),
        )
        .expect("wall preview")
        .valid
    };

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(12, 10, 2, 0x20);
    assert!(!preview(&sim), "unowned map wall is not replaceable");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(12, 10, 2, 0x20, enemy);
    assert!(!preview(&sim), "enemy wall is not replaceable");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(12, 10, 2, 0x0F, owner);
    assert!(
        !preview(&sim),
        "pristine same-owner wall is not replaceable"
    );

    let overlay_grid = sim.overlay_grid.as_mut().unwrap();
    overlay_grid.place_owned_wall(12, 10, 2, 0x20, owner);
    overlay_grid.place_owned_wall(13, 10, 2, 0x2F, owner);
    overlay_grid.place_owned_wall(30, 30, 2, 0x1B, owner);
    assert!(preview(&sim), "damaged same-owner wall is replaceable");
    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWALL",
            cell: (12, 10)
        },
        Some(&registry)
    ));

    let overlay_grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(overlay_grid.cell(12, 10).overlay_data, 0x02);
    assert_eq!(overlay_grid.cell(12, 10).wall_owner, Some(owner));
    assert_eq!(
        overlay_grid.cell(13, 10).overlay_data,
        0x28,
        "neighbor damage nibble is preserved while connectivity is refreshed"
    );
    assert_eq!(
        overlay_grid.cell(30, 30).overlay_data,
        0x1B,
        "placement does not globally rewrite wall frames"
    );
}

#[test]
fn place_ready_building_requires_base_normal_provider_within_adjacent_range() {
    let rules = placement_radius_rules();

    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));

    let mut far_sim = placement_sim();
    spawn_structure(&mut far_sim, 1, "Americans", "GACNST", 10, 10);
    let far_americans = far_sim.interner.intern("Americans");
    let far_gapowr = far_sim.interner.intern("GAPOWR");
    far_sim
        .production
        .ready_by_owner
        .insert(far_americans, VecDeque::from([far_gapowr]));
    // GACNST has Adjacent=6 (default), foundation 2x2 at (10,10).
    // Expanded zone: max_x = 10+2-1+7 = 18, so (20,10) is out of range.
    assert!(!place_production_with_overlays(
        &mut far_sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (20, 10)
        },
        None
    ));
}

#[test]
fn base_normal_false_structures_do_not_extend_build_area() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, 1, "Americans", "GAGAP", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
}

#[test]
fn build_off_ally_enabled_accepts_allied_eligible_provider() {
    let mut sim = placement_sim();
    let rules = build_off_ally_rules();

    spawn_structure(&mut sim, 1, "Alliance", "GACNST", 10, 10);
    // PLACE still needs this house's producer; only its radius comes from
    // the nearby ally. Keep the own yard outside this placement's build area.
    spawn_structure(&mut sim, 2, "Americans", "GACNST", 50, 50);
    mark_allied(&mut sim, "Americans", "Alliance");
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
}

#[test]
fn build_off_ally_disabled_rejects_allied_eligible_provider() {
    let mut sim = placement_sim();
    let rules = build_off_ally_rules();

    sim.session.game_options.build_off_ally = false;
    spawn_structure(&mut sim, 1, "Alliance", "GACNST", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GACNST", 50, 50);
    mark_allied(&mut sim, "Americans", "Alliance");
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
}

#[test]
fn build_off_ally_requires_eligibile_for_ally_building() {
    let mut sim = placement_sim();
    let rules = build_off_ally_rules();

    spawn_structure(&mut sim, 1, "Alliance", "GAPOWR", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GACNST", 50, 50);
    mark_allied(&mut sim, "Americans", "Alliance");
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
}

#[test]
fn build_off_ally_off_keeps_own_base_provider() {
    let mut sim = placement_sim();
    let rules = build_off_ally_rules();

    sim.session.game_options.build_off_ally = false;
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
}

#[test]
fn placement_preview_reports_out_of_build_area() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 20, 20)
            .expect("preview should exist");
    assert!(!preview.valid);
    assert_eq!(preview.reason, Some(BuildingPlacementError::OutOfBuildArea));
}

#[test]
fn placement_preview_reports_blocked_terrain() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if (cell.rx, cell.ry) == (12, 10) {
            cell.yr_cell_land_type = LandType::Rock.as_index();
        }
    }));

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("preview should exist");
    assert!(!preview.valid);
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
}

#[test]
fn place_ready_building_rejects_bridge_deck_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 12 && cell.ry == 10 {
            cell.has_bridge_deck = true;
            cell.bridge_walkable = true;
            cell.bridge_transition = true;
            cell.bridge_deck_level = 3;
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
    }));

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("preview should exist");
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
}

#[test]
fn place_ready_building_rejects_native_gap_restamp_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));
    sim.install_resolved_terrain_for_new_map(resolved_clear_grid_with_override(64, 64, |_| {}));
    assert!(crate::sim::build_site::can_place_building_at(
        &sim,
        &rules,
        None,
        rules.object("GAPOWR").unwrap(),
        (12, 10),
        Some(americans),
    ));
    let records = [crate::sim::bridge_state::BridgeEndpointRecord {
        endpoint_a: (11, 12),
        endpoint_b: (15, 12),
        active: false,
        bridge_kind: crate::sim::bridge_state::BridgeRecordKind::High,
    }];
    let flags = &mut sim.real_cell_bridge_flags_0x1180;
    crate::sim::bridge_state::gap_restamp::restamp_inactive_high_records(
        sim.resolved_terrain.as_mut().unwrap(),
        &records,
        |_, index, value| {
            if let Some(index) = index {
                flags.set_allocated_cell(index, value);
            }
        },
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(12, 10)
            .unwrap()
            .bridge_flags()
            & 0xC00,
        0xC00
    );

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("preview should exist");
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
    assert!(
        !preview.cell_valid[0],
        "binary CellClass+0x140 bit 0x400 blocks placement even without live bridge deck flags"
    );
}

#[test]
fn place_ready_building_rejects_canonical_ramp_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 12 && cell.ry == 10 {
            cell.has_ramp = true;
            cell.canonical_ramp = Some(RampDirection::West);
            cell.slope_type = 1;
            cell.ground_walk_blocked = false;
        }
    }));

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAPOWR", 12, 10)
            .expect("preview should exist");
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
    assert!(
        sim.resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(12, 10))
            .is_some_and(|cell| !cell.ground_walk_blocked),
        "canonical ramp fixture should stay movement-passable while rejecting placement"
    );
}

#[test]
fn place_ready_building_rejects_destroyed_bridge_over_blocked_ground() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    sim.production
        .ready_by_owner
        .insert(americans, VecDeque::from([gapowr]));
    let resolved = resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 12 && cell.ry == 10 {
            cell.ground_walk_blocked = true;
            cell.is_water = true;
            cell.base_build_blocked = true;
            cell.has_bridge_deck = true;
            cell.bridge_walkable = true;
            cell.bridge_transition = true;
            cell.bridge_deck_level = 3;
            // A legacy deck whose +44 bridge identity 47E040 has destroyed:
            // state 0, bit0x100 clear, bit0x400 set.
            cell.bridge_facts.overlay_id = Some(0x18);
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_DESTROYED_OR_RAMP;
            cell.bridge_facts.state_byte = 0;
        }
    });
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(&resolved, true, 5),
    );
    sim.resolved_terrain = Some(resolved);

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAPOWR",
            cell: (12, 10)
        },
        None
    ));
}

#[test]
fn gsi_04_04_water_bound_building_rejects_beach_zone() {
    let mut sim = placement_sim();
    let rules = naval_yard_placement_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAYARD");
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 20 && cell.ry == 20 {
            cell.is_water = true;
            cell.land_type = LandType::Beach.as_index();
            cell.yr_cell_land_type = cell.land_type;
            cell.zone_type = zone_class::BEACH;
            cell.terrain_class = TerrainClass::Water;
            cell.base_build_blocked = true;
        }
    }));

    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAYARD",
            cell: (20, 20)
        },
        None
    ));

    let preview =
        placement_preview_for_owner_without_overlays(&sim, &rules, "Americans", "GAYARD", 20, 20)
            .expect("preview should exist");
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
}

#[test]
fn gsi_04_04_water_bound_building_accepts_water_zone() {
    let mut sim = placement_sim();
    let rules = naval_yard_placement_rules();

    spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAYARD");
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 20 && cell.ry == 20 {
            cell.is_water = true;
            cell.land_type = LandType::Water.as_index();
            cell.yr_cell_land_type = cell.land_type;
            cell.zone_type = zone_class::WATER;
            cell.terrain_class = TerrainClass::Water;
            cell.base_build_blocked = true;
        }
    }));

    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAYARD",
            cell: (20, 20)
        },
        None
    ));
}

#[test]
fn producer_candidates_are_sorted_by_stable_id() {
    let mut sim = placement_sim();
    let rules = factory_rules();

    spawn_structure(&mut sim, 9, "Americans", "GAWEAP", 20, 20);
    spawn_structure(&mut sim, 3, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, 5, "Americans", "GAWEAP", 15, 15);

    let candidates = producer_candidates_for_owner_category(
        &sim.substrate.entities,
        &rules,
        "Americans",
        ProductionCategory::Vehicle,
        true,
        &sim.interner,
    );
    let ids: Vec<u64> = candidates.into_iter().map(|entry| entry.0).collect();
    assert_eq!(ids, vec![3, 5, 9]);
}

#[test]
fn cycle_active_producer_rotates_matching_factories() {
    let mut sim = placement_sim();
    let rules = factory_rules();

    spawn_structure(&mut sim, 3, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, 5, "Americans", "GAWEAP", 15, 15);
    spawn_structure(&mut sim, 9, "Americans", "GAWEAP", 20, 20);

    assert!(cycle_active_producer_for_owner_category(
        &mut sim,
        &rules,
        "Americans",
        ProductionCategory::Vehicle,
    ));
    assert_eq!(
        sim.production.primary_factory(
            sim.interner.intern("Americans"),
            ProductionCategory::Vehicle
        ),
        Some(3)
    );
    assert!(cycle_active_producer_for_owner_category(
        &mut sim,
        &rules,
        "Americans",
        ProductionCategory::Vehicle,
    ));
    assert_eq!(
        sim.production.primary_factory(
            sim.interner.intern("Americans"),
            ProductionCategory::Vehicle
        ),
        Some(5)
    );
}

#[test]
fn blocked_active_war_factory_does_not_spawn_from_second_factory() {
    let mut sim = placement_sim();
    let rules = factory_rules();
    let mut grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAWEAP", 30, 30);
    let americans = sim.interner.intern("Americans");
    sim.production
        .set_primary_factory_for_test(americans, ProductionCategory::Vehicle, 1);

    grid.set_blocked(12, 11, true);
    sim.install_fixture_path_grid(Some(&grid));

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        0,
    );
    let held = sim
        .production
        .factory_shadow
        .view(americans, ProductionCategory::Vehicle)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(americans, ProductionCategory::Vehicle)
    );
    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Mobile {
            category: ProductionCategory::Vehicle
        },
        None,
    ));
    let produced = sim.substrate.entities.get(held).unwrap();

    assert_eq!(
        (produced.position.rx, produced.position.ry),
        (12, 11),
        "native444565 retains the selected producer's coordinate despite PathGrid blockage"
    );
    assert!(produced.has_live_contact_with(1));
    assert!(!produced.has_live_contact_with(2));
}

#[test]
fn stock_war_factory_initial_exit_has_no_nearest_cell_fallback() {
    let mut sim = placement_sim();
    let rules = factory_rules();
    let mut grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);
    grid.set_blocked(12, 11, true);
    sim.install_fixture_path_grid(Some(&grid));

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        0,
    );
    let owner = sim.interner.get("Americans").unwrap();
    let held = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, ProductionCategory::Vehicle)
    );
    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Mobile {
            category: ProductionCategory::Vehicle
        },
        None,
    ));
    let produced = sim.substrate.entities.get(held).unwrap();

    assert_eq!(
        (produced.position.rx, produced.position.ry),
        (12, 11),
        "native factory scope bypasses class admission without choosing a nearby coordinate"
    );
    assert!(produced.lifecycle.cell_marked && !produced.lifecycle.in_limbo);
}

#[test]
fn stock_war_factory_clear_exitcoord_succeeds() {
    let mut sim = placement_sim();
    let rules = factory_rules();
    let grid = PathGrid::new(64, 64);
    sim.install_fixture_path_grid(Some(&grid));

    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);

    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        ProductionCategory::Vehicle,
        0,
    );
    let owner = sim.interner.get("Americans").unwrap();
    let held = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, ProductionCategory::Vehicle)
    );
    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Mobile {
            category: ProductionCategory::Vehicle
        },
        None,
    ));
    let produced = sim.substrate.entities.get(held).unwrap();

    assert_eq!(
        (produced.position.rx, produced.position.ry),
        (12, 11),
        "stock land war factory initial spawn uses ExitCoord=512,256,0"
    );
    assert!(produced.lifecycle.cell_marked && !produced.lifecycle.in_limbo);
}

#[test]
fn spawn_routing_prefers_active_producer_when_available() {
    let mut sim = placement_sim();
    let rules = factory_rules();
    let grid = PathGrid::new(64, 64);
    sim.install_fixture_path_grid(Some(&grid));

    spawn_structure(&mut sim, 3, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, 5, "Americans", "GAWEAP", 30, 30);
    let americans = sim.interner.intern("Americans");
    sim.production
        .set_primary_factory_for_test(americans, ProductionCategory::Vehicle, 5);

    let producer = super::find_factory(
        &sim,
        &rules,
        americans,
        rules.object("MTNK").unwrap(),
        false,
        true,
        false,
    )
    .expect("native FindFactory admits the primary producer");

    assert_eq!(
        producer, 5,
        "native FindFactory prefers the active war factory"
    );
}

/// A sale's refund (`TechnoClass vt+0x2BC` = `0x0070ADA0` ->
/// `TechnoTypeClass::GetRefund 0x00711F60`) reads no health, and the sale
/// drops every radio contact to the building.
#[test]
fn a_sale_refunds_regardless_of_health_and_clears_peer_contacts() {
    let rules = sell_rules();
    let refund = |health: i32| {
        let mut sim = placement_sim();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        *super::credits_entry_for_owner(&mut sim, "Americans") = 1000;
        spawn_structure(&mut sim, 1, "Americans", "GAPOWR", 20, 20);
        if let Some(ge) = sim.substrate.entities.get_mut(1) {
            ge.health = Health { current: health };
            ge.mark_live_contact_with(99);
        }
        let mut peer = GameEntity::test_default(99, "MTNK", "Americans", 22, 20);
        peer.owner = owner;
        peer.type_ref = sim.interner.intern("MTNK");
        peer.mark_live_contact_with(1);
        sim.substrate.entities.insert(peer);

        assert!(super::sell_building_now_for_test(&mut sim, &rules, 1));
        sim.flush_pending_delete();
        assert!(
            !sim.substrate.entities.contains(1),
            "sold building should be removed from the store"
        );
        assert!(
            !sim.substrate
                .entities
                .get(99)
                .unwrap()
                .has_live_contact_with(1),
            "selling a building should clear peer radio contacts to it"
        );
        credits_for_owner(&sim, "Americans") - 1000
    };
    let full = refund(750);
    assert!(full > 0);
    assert_eq!(refund(375), full, "a damaged building refunds the same");
}

/// `BuildingClass::Sell_Back @ 0x00447110`'s admission beyond the SELL
/// event's sales (`building_construction`'s route replays), with CanSell
/// (`0x004494C0`) beside it: a player's order on a Selling building clicks
/// again without a second sale; the computer's order (control 1) refuses,
/// silently, a Selling building or one carrying C4 (`+0x6DF`), which the
/// player's order and CanSell ignore; without a Buildup both orders are
/// refused, except by a `FirestormWall=` type, which leaves the map at once,
/// unpaid and silent (`0x004471C5`).
#[test]
fn sell_back_admits_by_control_buildup_and_firestorm_wall() {
    use super::{SellOrder, can_sell_building, sell_back};
    use crate::sim::components::PendingC4Detonation;
    use crate::sim::world::SimSoundEvent;
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n1=GAFWLL\n\
         [GAPOWR]\nOwner=Americans,Alliance,Russians,Soviet\nCost=800\nStrength=750\n\
         [GAFWLL]\nCost=100\nStrength=100\nFirestormWall=yes\n",
    ))
    .expect("sale admission rules should parse");
    let scene = |type_id: &str| {
        let mut sim = placement_sim();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        *super::credits_entry_for_owner(&mut sim, "Americans") = 1000;
        spawn_structure(&mut sim, 1, "Americans", type_id, 20, 20);
        sim
    };
    let clicks = |sim: &Simulation| {
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::SellClick { .. }))
            .count()
    };
    let selling = |sim: &Simulation| {
        sim.substrate.entities.get(1).is_some_and(|building| {
            building.mission.effective().known() == Some(MissionType::Selling)
        })
    };

    for order in [SellOrder::Player, SellOrder::Computer] {
        let mut sim = scene("GAPOWR");
        assert!(!can_sell_building(&sim, &rules, 1));
        assert!(!sell_back(&mut sim, &rules, 1, order, None), "{order:?}");
        assert!(!selling(&sim));
        assert_eq!(clicks(&sim), 0);

        let mut sim = scene("GAFWLL");
        assert!(can_sell_building(&sim, &rules, 1));
        assert!(sell_back(&mut sim, &rules, 1, order, None), "{order:?}");
        sim.flush_pending_delete();
        assert!(!sim.substrate.entities.contains(1), "{order:?}: removed");
        assert_eq!(clicks(&sim), 0);
        assert_eq!(credits_for_owner(&sim, "Americans"), 1000, "unpaid");
    }

    rules.set_buildup_control_for_test("GAPOWR", [0, 25, 2]);
    let mut sim = scene("GAPOWR");
    assert!(can_sell_building(&sim, &rules, 1));
    assert!(sell_back(&mut sim, &rules, 1, SellOrder::Player, None));
    assert!(selling(&sim));
    assert!(!can_sell_building(&sim, &rules, 1));
    let sale = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .mission
        .dispatch_timer();
    assert!(sell_back(&mut sim, &rules, 1, SellOrder::Player, None));
    assert_eq!(clicks(&sim), 2, "the repeated order clicks");
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .dispatch_timer(),
        sale,
        "the sale is not restarted"
    );
    assert!(!sell_back(&mut sim, &rules, 1, SellOrder::Computer, None));
    assert_eq!(clicks(&sim), 2);

    let mut sim = scene("GAPOWR");
    assert!(sell_back(&mut sim, &rules, 1, SellOrder::Computer, None));
    assert!(selling(&sim));
    assert_eq!(clicks(&sim), 1);

    let mut sim = scene("GAPOWR");
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .pending_c4_detonation = Some(PendingC4Detonation {
        timer: crate::sim::timer::CdTimer::started(0, 100),
        source_entity_id: None,
    });
    assert!(!sell_back(&mut sim, &rules, 1, SellOrder::Computer, None));
    assert!(!selling(&sim));
    assert_eq!(clicks(&sim), 0);
    assert!(can_sell_building(&sim, &rules, 1));
    assert!(sell_back(&mut sim, &rules, 1, SellOrder::Player, None));
    assert!(selling(&sim));
    assert_eq!(clicks(&sim), 1);
}

#[test]
fn sell_player_built_garrisoned_building_demolishes_and_ejects_alive() {
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let mut sim = placement_sim();
    let rules = sell_rules();
    *super::credits_entry_for_owner(&mut sim, "Americans") = 0;

    // Spawn a CanBeOccupied building OWNED by Americans with NO original_owner
    // (player-built, not captured). NABNKR in sell_rules has Cost=0 so the
    // refund is 0 — this test pins the demolition path (entities.remove fired)
    // and the alive-eject of the occupant, not the refund magnitude.
    spawn_structure(&mut sim, 30, "Americans", "NABNKR", 40, 40);
    // The legacy structure helper writes active/occupation state directly.
    // Admit this integration fixture through complete Techno Unlimbo before
    // adding its occupant, rather than inventing the retained +508 value.
    sim.remove_entity_occupancy(30);
    sim.substrate
        .entities
        .get_mut(30)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    assert!(matches!(
        sim.reveal_entity_with_rules(30, &rules),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate
            .entities
            .get(30)
            .unwrap()
            .cached_spatial_threat()
            .is_some()
    );
    let amer_id = sim.interner.intern("Americans");
    let e1_id = sim.interner.intern("E1");
    if let Some(t) = sim.substrate.entities.get_mut(30) {
        t.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 1),
        };
    }
    let mut pax = crate::sim::game_entity::GameEntity::test_default(31, "E1", "Americans", 39, 40);
    pax.owner = amer_id;
    pax.type_ref = e1_id;
    pax.passenger_role = PassengerRole::Inside {
        transport_id: 30,
        open_topped: false,
    };
    sim.substrate.entities.insert(pax);
    if let Some(t) = sim.substrate.entities.get_mut(30) {
        if let Some(c) = t.passenger_role.cargo_mut() {
            c.board(31, 1);
        }
    }
    // Accepted Building AddOccupant52298D refreshes after the append.
    sim.refresh_spatial_threat(30, &rules, None);

    assert!(super::sell_building_now_for_test(&mut sim, &rules, 30));

    // Building removed (deferred-delete: drain at end-of-tick to free the slot).
    sim.flush_pending_delete();
    assert!(
        !sim.substrate.entities.contains(30),
        "player-built garrison should be demolished on sell"
    );
    // Occupant placed on the map alive.
    let pax = sim.substrate.entities.get(31).expect("occupant exists");
    assert!(!pax.dying, "occupant should not be dying");
    assert!(pax.health.current > 0, "occupant should be alive");
    assert!(
        matches!(pax.passenger_role, PassengerRole::None),
        "occupant role should be None"
    );
}

#[test]
fn retained_wall_plane_runtime_placement_and_damage_update_once() {
    use crate::sim::overlay_grid::damage_wall_overlay_with_terrain;

    let ini = IniFile::from_str(
        "[OverlayTypes]\n0=WALL\n\
         [WALL]\nWall=yes\nStrength=100\n",
    );
    let art = IniFile::from_str("[WALL]\nDamageLevels=3\n");
    let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let mut sim = placement_sim();
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(5, 5, |_| {}));
    sim.overlay_grid = Some(OverlayGrid::new(5, 5));
    let owner = sim.interner.intern("WallOwner");
    assert!(super::wall_placement::stamp_wall(
        &mut sim, &registry, 2, 2, 0, owner
    ));
    let mut grid = sim.overlay_grid.take().unwrap();
    let mut terrain = sim.resolved_terrain.take().unwrap();
    let placed = grid.retained_neighbor_counts();
    for index in [6usize, 7, 8, 11, 13, 16, 17, 18] {
        assert_eq!(placed[index], 1);
    }
    assert_eq!(placed[12], 0);

    let mut rng = crate::sim::rng::SimRng::new(1);
    let before_partial = grid.retained_neighbor_counts().to_vec();
    let _ = grid.take_synchronous_navigation_cells();
    let partial = damage_wall_overlay_with_terrain(
        &mut grid,
        &registry,
        Some(&mut terrain),
        2,
        2,
        100,
        &mut rng,
    );
    assert!(partial.destroyed_cells.is_empty());
    assert_eq!(partial.changed_cells, vec![(2, 2)]);
    assert_eq!(grid.cell(2, 2).overlay_data, 0x10);
    assert!(
        grid.take_synchronous_navigation_cells().is_empty(),
        "partial damage returns before native's direct-removal Recalc"
    );
    assert_eq!(
        grid.retained_neighbor_counts(),
        before_partial.as_slice(),
        "nonterminal damage must not change retained counts"
    );

    let result = damage_wall_overlay_with_terrain(
        &mut grid,
        &registry,
        Some(&mut terrain),
        2,
        2,
        -1,
        &mut rng,
    );
    assert_eq!(result.destroyed_cells, vec![(2, 2)]);
    assert_eq!(
        grid.take_synchronous_navigation_cells(),
        vec![(2, 2)],
        "direct removal publishes its Recalc before cleanup completes"
    );
    assert!(
        grid.retained_neighbor_counts()
            .iter()
            .all(|&count| count == 0)
    );
}

#[test]
fn retained_wall_plane_placement_reaches_fixed_stride_alias() {
    let registry = OverlayTypeRegistry::from_ini(
        &IniFile::from_str("[OverlayTypes]\n0=GASAND\n[GASAND]\nWall=yes\nStrength=100\n"),
        None,
    );
    let mut sim = placement_sim();
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(512, 2, |_| {}));
    sim.overlay_grid = Some(OverlayGrid::new(512, 2));
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(511, 0, 0, 0);
    let owner = sim.interner.intern("WallOwner");
    assert!(super::wall_placement::stamp_wall(
        &mut sim, &registry, 0, 1, 0, owner
    ));
    let alias_grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(
        alias_grid.cell(0, 1).overlay_data & 0x0F,
        0x08,
        "anchor connects west through fixed-stride alias"
    );
    assert_eq!(
        alias_grid.cell(511, 0).overlay_data & 0x0F,
        0x02,
        "aliased real wall connects east back to anchor"
    );
    assert_eq!(
        alias_grid.retained_neighbor_counts()[511],
        1,
        "west fixed-stride alias resolves to real slot 511"
    );
    assert_eq!(
        alias_grid
            .retained_neighbor_counts()
            .iter()
            .map(|&count| u32::from(count))
            .sum::<u32>(),
        5,
        "three true-dummy neighbors produce no retained output"
    );
}

/// A Slave Miner refinery placed from production takes the hand-off its
/// Unlimbo runs (`0x006B0D60`, `BuildingClass::ExitObject @ 0x004452FA`):
/// the manager its factory constructed idle (state 0) waits out the build-up
/// in state 4 (frame MAX), as a deployed one does, every slave inside.
#[test]
fn a_placed_slave_refinery_waits_out_its_build_up_in_the_deployed_state() {
    use crate::sim::slave_manager::ManagerState;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n0=SLAV\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GACNST\n1=YAREFN\n\
         [GACNST]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=1000\nFactory=BuildingType\n\
         [SLAV]\nStrength=125\nSpeed=3\nSlaved=yes\nStorage=4\n\
         [YAREFN]\nOwner=Americans,Alliance,Russians,Soviet\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=5\nFoundation=2x2\n[Clear]\nBuildable=yes\n",
    ))
    .expect("slave refinery rules");
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 14, 20);
    let refinery = ready_and_place(&mut sim, &rules, "Americans", "YAREFN", 16, 20);
    let entity = sim.substrate.entities.get(refinery).unwrap();
    assert!(entity.building_up(), "placed buildings build up");
    let manager = entity
        .slave_manager
        .as_ref()
        .expect("Enslaves= builds the manager");
    assert_eq!(manager.state(), ManagerState::Deployed);
    assert_eq!(manager.frame(), i32::MAX);
    let slaves: Vec<u64> = manager.slaves().collect();
    assert_eq!(slaves.len(), 5);
    for slave in slaves {
        assert!(
            sim.substrate
                .entities
                .get(slave)
                .unwrap()
                .lifecycle
                .in_limbo
        );
    }
}

/// Original P2 blocked-preferred output160/269 requests radar6 at the
/// selected GAPILE's GetCoords cell15,15, although the admitted GI is in14,15.
/// House4FB5F2..4FB5FB calls producer vt48 -> Building447AC0; the actual
/// original return is3968,3840,0 from Location3712,3712,0/Foundation3x2.
/// Source: basic-factory-output-prerequisites-research/
/// blocked-preferred-attempt-2.json SHA6bd726fb29c2398b093baeedbbdd4d0f80ccf10d688a11e8bc2cc29fcb4ac9c4,
/// linked by tools/spatial_oracle/_factory_infantry_output/meta.json.
/// Paid-ready/frame159 are supplied priors; this is not full charge/RNG parity.
#[test]
fn stock_infantry_fallback_unit_ready_uses_producer_getcoords() {
    use crate::sim::movement::ground_pose;
    use crate::sim::radar::RadarEventType;
    use crate::sim::world::SimSoundEvent;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let mut sim = Simulation::with_seed(2);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let owner = sim.interner.intern("Americans");
    let mut house =
        crate::sim::house_state::HouseState::new(owner, 0, Some(owner), true, 10_000, 10);
    house.difficulty = crate::sim::house_state::HouseDifficulty::Hard;
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    let producer = sim
        .spawn_object_at_height_with_overlay_registry(
            "GAPILE",
            "Americans",
            14,
            14,
            64,
            0,
            &rules,
            &registry,
        )
        .expect("stock GAPILE constructor and admission");
    sim.spawn_object_at_height_with_overlay_registry(
        "GAPOWR",
        "Americans",
        15,
        16,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("original P2 blocker enters through Building admission");
    let producer_entity = sim.substrate.entities.get(producer).unwrap();
    let location = ground_pose::position_world_coord(&producer_entity.position);
    assert_eq!([location.x, location.y, location.z], [3712, 3712, 0]);
    let center = ground_pose::object_get_coords(producer_entity, sim.resolved_terrain.as_ref());
    assert_eq!([center.x, center.y, center.z], [3968, 3840, 0]);

    assert!(super::enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
    let category = ProductionCategory::Infantry;
    let held = sim
        .production
        .factory_shadow
        .view(owner, category)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    let cost = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    assert_eq!(
        sim.houses.get_mut(&owner).unwrap().economy.spend(cost),
        cost
    );
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, category)
    );
    // The focused publisher helper increments both clocks before applying
    // due PLACE, so159 ->160 reaches the measured native fallback call frame.
    // No actor/House turns or native completion cadence are asserted here.
    sim.session.binary_frame = 159;
    sim.session.tick = 159;
    let event_start = sim.sound_events.len();
    assert!(super::dispatch_production_changes_for_tests(
        &mut sim,
        &rules,
        Some(&registry),
    ));
    assert_eq!(sim.session.binary_frame, 160);
    assert_eq!(sim.session.tick, 160);
    assert!(sim.pending_command_snapshot().is_empty());
    let product = sim.substrate.entities.get(held).unwrap();
    let position = ground_pose::position_world_coord(&product.position);
    assert_eq!([position.x, position.y, position.z], [3712, 3968, 0]);
    assert_eq!((product.position.rx, product.position.ry), (14, 15));
    assert_eq!(product.radio_contacts.slot(0), Some(producer));
    assert_eq!(product.dock_entered_with, Some(producer));
    let producer_entity = sim.substrate.entities.get(producer).unwrap();
    assert_eq!(producer_entity.radio_contacts.slot(0), Some(held));
    assert_eq!(producer_entity.dock_entered_with, Some(held));
    let notifications: Vec<_> = sim.sound_events[event_start..]
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::UnitComplete { owner, radar } => {
                Some((*owner, radar.event_type, radar.rx, radar.ry))
            }
            _ => None,
        })
        .collect();
    assert_eq!(notifications, [(owner, RadarEventType::UnitReady, 15, 15)]);
}

fn building_placed_owners(sim: &Simulation) -> Vec<crate::sim::intern::InternedId> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::SimSoundEvent::BuildingPlaced { owner } => Some(*owner),
            _ => None,
        })
        .collect()
}

/// `HouseClass @ 0x004FB0E0`: BuildingSlam (`0x004FB2FD..0x004FB314`) follows
/// only a successful Unlimbo (`0x004FB236 JE` skips it), for a wall too
/// (its Unlimbo returns 1 at `0x00440865`).
#[test]
fn successful_placement_requests_building_slam_once() {
    let rules = build_catalog_rules();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, 1, "Americans", "GACNST", 18, 18);
    super::credits_entry_for_owner(&mut sim, "Americans");
    ready_building(&mut sim, &rules, "Americans", "GACNST");
    let americans = sim.interner.intern("Americans");

    let far = ProductionPlacement::Building {
        type_id: "GACNST",
        cell: (60, 60),
    };
    assert!(!place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        far,
        None
    ));
    assert!(building_placed_owners(&sim).is_empty());

    let near = ProductionPlacement::Building {
        type_id: "GACNST",
        cell: (20, 20),
    };
    assert!(place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        near,
        None
    ));
    assert_eq!(building_placed_owners(&sim), vec![americans]);

    let (wall_rules, registry) = gsi_04_07_wall_placement_contract();
    let mut wall_sim = placement_sim();
    spawn_structure(&mut wall_sim, 1, "Americans", "GACNST", 10, 10);
    wall_sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut wall_sim, &wall_rules, "Americans", "GAWALL");
    let wall = ProductionPlacement::Building {
        type_id: "GAWALL",
        cell: (12, 10),
    };
    assert!(place_production_with_overlays(
        &mut wall_sim,
        &wall_rules,
        "Americans",
        wall,
        Some(&registry)
    ));
    let wall_owner = wall_sim.interner.get("Americans").expect("owner");
    assert_eq!(building_placed_owners(&wall_sim), vec![wall_owner]);
}
