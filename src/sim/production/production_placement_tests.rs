//! Building placement tests — verifies foundation overlap detection, placement validity,
//! and per-owner placement pool management for the production system.

use crate::rules::foundation::foundation_dimensions;

use super::{
    BuildingPlacementError, ProductionCategory, ProductionPlacement, credits_for_owner,
    cycle_active_producer_for_owner_category, place_production_with_overlays,
    placement_preview_for_owner_with_overlays, producer_candidates_for_owner_category,
    publish_production_changes, ready_building_entity_for_owner, ready_buildings_for_owner,
};
use crate::map::entities::EntityCategory;
use crate::map::overlay_mark::OverlayMarkOutcome;
use crate::map::resolved_terrain::{
    RampDirection, ResolvedTerrainCell, ResolvedTerrainGrid, zone_class,
};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
use crate::sim::combat::{Health, TargetKind};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::BuildingUp;
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
         SpeedType=Track\n\
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
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art_ini));
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
    assert!(place_building(sim, rules, owner, type_id, (rx, ry), None));
    sim.substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner() == owner_id
                && entity.type_ref() == type_ref
                && entity.position.rx() == rx
                && entity.position.ry() == ry
                && entity.category() == EntityCategory::Structure
        })
        .map(|entity| entity.stable_id())
        .expect("placed building should exist")
}

fn set_ticks_until_completion(sim: &mut Simulation, stable_id: u64, ticks: u16) {
    let now = sim.session.binary_frame() as i32;
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
    sim.fog.set_width_for_test(64);
    sim.fog.set_height_for_test(64);
    let owner = sim.interner.intern("Americans");
    let viewer = sim.interner.intern("Soviet");
    for house in [owner, viewer] {
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house, 0, None, true, 10_000, 10,
            ));
    }
    sim.fog.reveal_all_for_owner(owner);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
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
            .is_operational()
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
            entity.category() == EntityCategory::Unit
                && sim
                    .interner
                    .resolve(entity.owner())
                    .eq_ignore_ascii_case(owner)
                && sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case(type_id)
        })
        .map(|entity| entity.stable_id())
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
/// indexes, not merely a subset of it.
///
/// Not parity: the search takes the arguments OnConstructionComplete pushes
/// (instruction reading) through the shared search port, but no native run of
/// this scene has derived the landing cell. These tests ratchet the mechanism
/// (seed, ring order, occupancy, frame-counter selection), not gamemd's cell.
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
    let (sim, refinery_id, spawned) =
        complete_stock_allied_refinery_on(start_frame, extra_blockers, |_| {});
    assert!(
        spawned,
        "frame {start_frame}: refinery completion should report the free unit"
    );
    (sim, refinery_id)
}

/// [`complete_stock_allied_refinery`] on clear ground that `override_cell`
/// edits; also returns whether the completion tick reported a spawn.
fn complete_stock_allied_refinery_on(
    start_frame: u32,
    extra_blockers: &[(u16, u16)],
    override_cell: impl FnMut(&mut ResolvedTerrainCell),
) -> (Simulation, u64, bool) {
    let mut sim = placement_sim();
    sim.session.set_binary_frame_for_test(start_frame);
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    block_building_foundation(&mut grid, &rules, "GAREFN", 20, 20);
    for &(rx, ry) in extra_blockers {
        sim.spawn_object("BLOCKER", "Russians", rx, ry, 0, &rules)
            .expect("fixture blocker should spawn");
    }
    install_refinery_test_terrain_on(&mut sim, override_cell);
    set_ticks_until_completion(&mut sim, refinery_id, 1);

    let completion = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    (sim, refinery_id, completion.spawned_entities)
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

/// Install the MapClass inputs that active UnitClass::Unlimbo and the
/// FreeUnit searches always see: the playfield, the Map Size, the cells and
/// their zones. Blocker fixtures are inserted before this call so they model
/// objects already occupying the bay rather than a second constructor
/// placement.
fn install_refinery_test_terrain(sim: &mut Simulation) {
    install_refinery_test_terrain_on(sim, |_| {});
}

fn install_refinery_test_terrain_on(
    sim: &mut Simulation,
    override_cell: impl FnMut(&mut ResolvedTerrainCell),
) {
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    }));
    sim.set_playfield_size_height_for_test(Some(64));
    let terrain = resolved_clear_grid_with_override(64, 64, override_cell);
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_terrain(
            &PathGrid::from_resolved_terrain(&terrain),
            &terrain,
            &[],
            64,
            64,
        ),
    );
    sim.resolved_terrain = Some(terrain);
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
         TechLevel=1\n\
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
        "[Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Soviet\n[InfantryTypes]\n\
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
         Owner=Americans,Alliance,Russians,Soviet\n\
         Wall=yes\n\
         Armor=concrete\n\
         Strength=300\n\
         Cost=100\n\
         TechLevel=1\n\
         Foundation=1x1\n\
         Adjacent=8\n\
         GuardRange=5\n\
         [WALLKIT]\n\
         Owner=Americans,Alliance,Russians,Soviet\n\
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
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art_ini));
    (rules, OverlayTypeRegistry::from_ini(&ini, None))
}

/// A 2x2 GACNST yard and a modded MODPROC refinery of `foundation` docking
/// MODHARV, with `free_unit` as its `FreeUnit=` when set.
fn modded_refinery_rules(foundation: &str, free_unit: Option<&str>) -> RuleSet {
    let free_unit = free_unit.map_or(String::new(), |unit| format!("FreeUnit={unit}\n"));
    RuleSet::from_ini(&IniFile::from_str(&format!(
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
         {free_unit}\
         Foundation={foundation}\n\
         [MODHARV]\n\
         Strength=600\n\
         Harvester=yes\n\
         Dock=MODPROC\n\
         Speed=4\n[Clear]\nBuildable=yes\n",
    )))
    .expect("modded refinery rules should parse")
}

fn mark_allied(sim: &mut Simulation, a: &str, b: &str) {
    let a = a.to_ascii_uppercase();
    let b = b.to_ascii_uppercase();
    sim.house_alliances_mut_for_test()
        .entry(a.clone())
        .or_default()
        .insert(b.clone());
    sim.house_alliances_mut_for_test()
        .entry(b)
        .or_default()
        .insert(a);
}

fn ready_building(sim: &mut Simulation, rules: &RuleSet, owner: &str, type_id: &str) {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern(type_id);
    super::complete_held_building_for_test(sim, rules, owner_id, type_id);
}

/// Place `owner`'s ready `type_id` at `cell` through Place_Production.
fn place_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    cell: (u16, u16),
    overlays: Option<&OverlayTypeRegistry>,
) -> bool {
    place_production_with_overlays(
        sim,
        rules,
        owner,
        ProductionPlacement::Building { type_id, cell },
        overlays,
        crate::sim::world::FrameEffects::default(),
    )
}

/// The placement cursor's preview of `owner`'s `type_id` at `cell`.
fn preview_at(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    cell: (u16, u16),
    overlays: Option<&OverlayTypeRegistry>,
) -> super::BuildingPlacementPreview {
    placement_preview_for_owner_with_overlays(sim, rules, owner, type_id, cell.0, cell.1, overlays)
        .expect("a known building type has a preview")
}

/// A PLACE event for `owner`'s ready `type_id` at `cell`, executing on `tick`.
fn place_event(
    owner: crate::sim::intern::InternedId,
    type_id: crate::sim::intern::InternedId,
    (rx, ry): (u16, u16),
    tick: u64,
) -> CommandEnvelope {
    CommandEnvelope::new(owner, tick, Command::PlaceReadyBuilding { type_id, rx, ry })
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
         TechLevel=1\n\
         BuildCat=Power\n\
         Strength=750\n\
         Armor=wood\n\
         Adjacent=2\n\
         Power=200\n\
         [AMRADR]\nOwner=Americans,Alliance,Russians,Soviet\n\
         TechLevel=1\n\
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
fn a_completed_building_waits_in_its_factory_for_placement() {
    let mut sim = placement_sim();
    let rules = build_catalog_rules();

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gacnst = sim.interner.intern("GACNST");
    super::house_for_test(&mut sim, "Americans")
        .economy_mut()
        .set_credits_for_test(50_000);
    let built_before = sim.houses[&americans].stats().built();
    // Arm the Building build in the registry and complete it; the Strip's
    // publish then announces the held building once.
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
            .factories_mut()
            .test_arm_ready(americans, ProductionCategory::Building)
    );

    publish_production_changes(&mut sim, &rules);
    let held = sim
        .production
        .factories()
        .view(americans, ProductionCategory::Building)
        .expect("completed building remains held by its Factory");
    assert!(held.complete_object().is_some());
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
    assert_eq!(sim.houses[&americans].stats().built(), built_before);
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building),
        held_id
    );
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans")
            .into_iter()
            .map(|item| item.type_id)
            .collect::<Vec<_>>(),
        vec![gacnst]
    );
    assert_eq!(sim.sound_events.iter().filter(|event| matches!(event, crate::sim::world::SimSoundEvent::BuildingComplete { owner } if *owner == americans)).count(), 1);
    assert_eq!(sim.scenario_rng.logical_state(), rng);
}

/// HousePlace4FB22E continues completion/C/abandon/score even when ordinary
/// Building Unlimbo4408DF installs and UnInit's its temporary product.
#[test]
fn installed_upgrade_consumes_factory_product_but_keeps_host_and_placement_tail() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[Countries]\n0=Americans\n1=Soviet\n\
             [BuildingTypes]\n0=GACNST\n1=POWER\n2=UPGRADE\n\
             [GACNST]\nOwner=Americans,Soviet\nFactory=BuildingType\nStrength=1000\n\
             [POWER]\nOwner=Americans,Soviet\nStrength=1000\nUpgrades=1\nPower=100\nBaseNormal=no\n\
             [UPGRADE]\nOwner=Americans,Soviet\nStrength=100\nCost=600\nPower=50\nPowersUpBuilding=POWER\n\
             [Clear]\nBuildable=yes\n",
        ),
        &IniFile::from_str(
            "[GACNST]\nFoundation=1x1\n[POWER]\nFoundation=1x1\n[UPGRADE]\nFoundation=1x1\n",
        ),
    )
    .unwrap();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    // A remote non-BaseNormal host tests the special Display+1180 gate,
    // independently of the ordinary Cell clear-to-build result.
    spawn_structure(&mut sim, &rules, 2, "Americans", "POWER", 40, 40);
    spawn_structure(&mut sim, &rules, 3, "Soviet", "POWER", 42, 40);
    sim.substrate.entities.get_mut(2).unwrap().health.set(200);
    let owner = sim.interner.get("Americans").unwrap();
    let upgrade_type = sim.interner.intern("UPGRADE");
    ready_building(&mut sim, &rules, "Americans", "UPGRADE");
    let held = super::lifecycle_tests::held_id(&sim, owner, ProductionCategory::Building);
    let rng = sim.scenario_rng.logical_state();
    let built = sim.houses[&owner].stats().built();
    assert!(!place_building(
        &mut sim,
        &rules,
        "Americans",
        "UPGRADE",
        (42, 40),
        None,
    ));
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, owner, ProductionCategory::Building),
        held,
        "a refused host keeps the same paid factory object for retry"
    );
    let preview = preview_at(&sim, &rules, "Americans", "UPGRADE", (40, 40), None);
    assert!(preview.valid && preview.in_build_area);
    assert!(
        preview.cells[0].valid,
        "the per-cell upgrade preview admits its host"
    );
    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    let object = rules.object("UPGRADE").unwrap();
    assert!(
        !crate::sim::build_site::is_clear_to_build(
            &sim,
            &rules,
            None,
            cells.lookup((40, 40)),
            crate::sim::build_site::building_speed_type(object),
            Some(object),
            Some(owner),
            Some(&cells),
            None,
        ),
        "the independent ordinary cell query still rejects occupied ground"
    );
    let no_host = preview_at(&sim, &rules, "Americans", "UPGRADE", (12, 10), None);
    assert!(
        no_host.in_build_area && no_host.valid,
        "Display keeps its ordinary proximity gate"
    );
    assert!(
        !no_host.cells[0].valid,
        "an upgrade cannot paint clear ground as an admitted host"
    );
    assert!(place_building(
        &mut sim,
        &rules,
        "Americans",
        "UPGRADE",
        (40, 40),
        None,
    ));
    let host = sim.substrate.entities.get(2).unwrap();
    assert!(host.is_object_alive() && !host.lifecycle.in_limbo());
    assert_eq!(host.health.current(), 1000);
    assert_eq!(host.building_upgrades().count(), 1);
    assert_eq!(
        host.building_upgrades().types(),
        &[Some(upgrade_type), None, None]
    );
    let consumed = sim.substrate.entities.get(held).unwrap();
    assert!(!consumed.is_object_alive() && consumed.lifecycle.in_limbo());
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
    assert_eq!(sim.houses[&owner].stats().built(), built + 1);
    assert_eq!(building_placed_owners(&sim), vec![owner]);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        rng,
        "installation has no additional constructor draw"
    );
}

/// Place_Production admits only what `FactoryClass::IsComplete @ 0x004CA130`
/// answers for (`0x004FB17D`): a building still in production is refused, at
/// the PLACE and in its preview, and stays held. Once complete it places,
/// whether or not the Strip has published the completion.
#[test]
fn an_unfinished_building_is_refused_until_its_factory_completes() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    super::house_for_test(&mut sim, "Americans");
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        ProductionCategory::Building,
        1,
    );
    let held = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building);
    let place =
        |sim: &mut Simulation| place_building(sim, &rules, "Americans", "GAPOWR", (12, 10), None);

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert_eq!(preview.reason, Some(BuildingPlacementError::NotReady));
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
    assert!(!place(&mut sim));
    assert_eq!(
        super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building),
        held
    );
    assert!(
        sim.substrate
            .entities
            .get(held)
            .unwrap()
            .lifecycle
            .in_limbo()
    );

    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(americans, ProductionCategory::Building)
    );
    assert!(
        sim.production
            .factories()
            .view(americans, ProductionCategory::Building)
            .and_then(|factory| factory.object)
            .is_some_and(|object| !object.completion_accounted),
        "complete, not yet published"
    );
    assert!(place(&mut sim));
    let placed = sim.substrate.entities.get(held).unwrap();
    assert_eq!((placed.position.rx(), placed.position.ry()), (12, 10));
    assert!(!placed.lifecycle.in_limbo());
}

/// `Main_Tick` steps the factories (`LogicClass::PerTickUpdate` at
/// `0x0055DC9E`, which runs each `FactoryClass::AI`) before it executes the
/// frame's events (`Process_Command_Queues_For_Frame` at `0x0055DE40`), and
/// Place_Production asks only IsComplete: a PLACE executing in the frame its
/// building completes places it, before the Strip publishes the completion in
/// the next frame's prefix.
#[test]
fn a_place_executing_in_the_completion_frame_is_admitted() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    let grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");
    super::house_for_test(&mut sim, "Americans");
    arm_build_via(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        ProductionCategory::Building,
        1,
    );
    let held = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building);
    // The last step, paid off, is due in the next frame.
    let frame = sim.session.binary_frame() as i32;
    let factory = sim
        .production
        .factories_mut()
        .test_factory_mut(americans, ProductionCategory::Building)
        .expect("armed Buildings factory");
    factory.set_progress_for_test(super::PRODUCTION_STEPS - 1);
    factory.set_balance_for_test(0);
    factory.set_step_rate_frames_for_test(1);
    factory.set_step_timer_for_test(crate::sim::timer::CdTimer::started(frame, 0));
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());

    let place = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
    let tick = sim.advance_tick(&[place], Some(&rules), Some(&grid), None, 67);

    assert_eq!(tick.executed_commands, 1);
    assert!(tick.spawned_entities);
    let placed = sim.substrate.entities.get(held).unwrap();
    assert_eq!((placed.position.rx(), placed.position.ry()), (12, 10));
    assert!(!placed.lifecycle.in_limbo());
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
}

#[test]
fn place_ready_building_spawns_and_consumes_ready_item() {
    let mut sim = placement_sim();
    let rules = build_catalog_rules();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 18, 18);

    let americans = sim.interner.intern("Americans");
    // The house whose Record_Last_Built counts the placement.
    super::house_for_test(&mut sim, "Americans");
    ready_building(&mut sim, &rules, "Americans", "GACNST");
    let built_before = sim.houses[&americans].stats().built();
    let held_id = sim
        .production
        .factories()
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

    let admitted = place_building(&mut sim, &rules, "Americans", "GACNST", (20, 20), None);
    assert!(admitted);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
    assert_eq!(sim.houses[&americans].stats().built(), built_before + 1);

    let structures = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.owner())
                .eq_ignore_ascii_case("Americans")
                && sim
                    .interner
                    .resolve(e.type_ref())
                    .eq_ignore_ascii_case("GACNST")
                && e.position.rx() == 20
                && e.position.ry() == 20
                && e.category() == crate::map::entities::EntityCategory::Structure
        })
        .count();
    assert_eq!(structures, 1);
    let placed = sim
        .substrate
        .entities
        .get(held_id)
        .expect("same held identity placed");
    assert_eq!((placed.position.rx(), placed.position.ry()), (20, 20));
    assert!(!placed.lifecycle.in_limbo());
    let successor = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Building);
    assert!(successor > held_id);
    assert_eq!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .techno_ctor_random_word(),
        successor_word
    );
    assert!(
        sim.substrate
            .entities
            .get(successor)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(
        sim.production
            .factories()
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
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            americans, 0, None, human, 10_000, 10,
        ));
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    let gapowr = sim.interner.intern("GAPOWR");
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    let place = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
    let placed_frame = sim.session.binary_frame();
    let tick = sim.advance_tick(&[place], Some(&rules), Some(&grid), None, 67);
    assert_eq!(tick.executed_commands, 1);
    let placed = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.type_ref() == gapowr)
        .map(|entity| entity.stable_id())
        .expect("the command places GAPOWR");

    let mut completed = None;
    for _ in 0..80 {
        let frame = sim.session.binary_frame();
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

/// A human player's placement follows the native `player` route rows
/// (`tools/spatial_oracle/building_construction.json`) frame by frame: idle
/// (BState 1) and unopened for a frame, then Construction. GetImage
/// (`0x004513D0`) hands DrawBody the Buildup in every frame before the
/// opening, the idle one included.
#[test]
fn a_human_placement_draws_its_buildup_until_it_opens() {
    use crate::sim::building_art::{BuildingImage, building_image};
    use crate::sim::building_construction::native_route;
    let rows = native_route::rows("player");
    assert!(!rows.is_empty());
    for row in &rows {
        let name = row["input"]["name"].as_str().unwrap();
        let mut sim = placement_sim();
        // The native rows run at GameSpeed 0.
        sim.session.game_options_mut_for_test().game_speed = 0;
        let mut rules = stock_power_contract_rules();
        rules.set_buildup_control_for_test("GAPOWR", native_route::control(row));
        let grid = PathGrid::new(64, 64);
        let americans = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, true, 10_000, 10,
            ));
        spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
        let gapowr = sim.interner.intern("GAPOWR");
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        ready_building(&mut sim, &rules, "Americans", "GAPOWR");
        let place = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
        let tick = sim.advance_tick(&[place], Some(&rules), Some(&grid), None, 67);
        assert_eq!(tick.executed_commands, 1, "{name}");
        let placed = sim
            .substrate
            .entities
            .values()
            .find(|entity| entity.type_ref() == gapowr)
            .map(|entity| entity.stable_id())
            .expect("the command places GAPOWR");

        for frame in row["frames"].as_array().unwrap() {
            sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let building = sim.substrate.entities.get(placed).unwrap();
            let context = format!("{name} frame {}", frame["frame"]);
            let native = if frame["bstate"] != 0 && frame["grand_opening"] == true {
                BuildingImage::Body
            } else {
                BuildingImage::Buildup
            };
            assert_eq!(building_image(building, true), native, "{context}");
            if native_route::assert_frame(building, frame, &context) {
                break;
            }
        }
    }
}

#[test]
fn stock_gapowr_placement_restores_power_and_radar_during_buildup() {
    let mut sim = placement_sim();
    let rules = stock_power_contract_rules();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    spawn_structure(&mut sim, &rules, 2, "Americans", "AMRADR", 10, 14);

    let americans = sim.interner.intern("Americans");
    let gapowr = sim.interner.intern("GAPOWR");

    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    let outage = sim
        .power_states
        .get(&americans)
        .expect("stock radar house should have derived power state");
    assert_eq!(outage.total_output(), 0);
    assert_eq!(outage.total_drain(), 50);
    assert!(outage.is_low_power());
    assert!(
        !has_active_radar(sim.power_states.as_map(), americans),
        "stock American radar must be offline during house low power"
    );

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    let place = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);

    let tick = sim.advance_tick(&[place], Some(&rules), Some(&grid), None, 67);
    assert_eq!(tick.executed_commands, 1);

    let placed = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.owner() == americans
                && entity.type_ref() == gapowr
                && entity.position.rx() == 12
                && entity.position.ry() == 10
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
            entity.owner() == americans && entity.type_ref() == gapowr && entity.building_up()
        }),
        "GAPOWR must still be in its visible buildup during reassessment"
    );

    let recovered = sim
        .power_states
        .get(&americans)
        .expect("post-placement tick should reassess stock power");
    assert_eq!(recovered.total_output(), 200);
    assert_eq!(recovered.total_drain(), 50);
    assert!(!recovered.is_low_power());
    assert!(
        has_active_radar(sim.power_states.as_map(), americans),
        "existing stock American radar should recover while GAPOWR is building up"
    );
}

#[test]
fn place_ready_building_accepts_clear_mixed_height_footprint() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);

    for (cell, z) in [((12, 10), 0), ((13, 10), 1), ((12, 11), 2), ((13, 11), 3)] {
        sim.resolved_terrain
            .as_mut()
            .expect("placement fixture terrain")
            .cell_mut_for_test(cell.0, cell.1)
            .expect("footprint cell")
            .level = z;
    }

    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(
        preview.valid,
        "mixed clear heights should not reject placement"
    );
    assert!(
        preview.cells.iter().all(|cell| cell.valid),
        "all otherwise-clear mixed-height cells should be individually valid"
    );

    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(placed);

    assert!(sim.substrate.entities.values().any(|e| {
        sim.interner
            .resolve(e.type_ref())
            .eq_ignore_ascii_case("GAPOWR")
            && e.position.rx() == 12
            && e.position.ry() == 10
    }));
}

#[test]
fn stock_refinery_free_unit_spawns_on_building_up_completion_once() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 14, 20);
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
        sim.session.binary_frame() - 1,
        building.mission.current(),
        building.mission.queued(),
        building.mission.handler_state(),
        building.building_ready_latch(),
        building.building_body_state(),
        building.native_stage(),
        building.building_art.actually_placed(),
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
        let cell = (miner.position.rx(), miner.position.ry());

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
                miner.stable_id()
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
                .contains_entity(cell.0, cell.1, miner.stable_id()),
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

/// OnConstructionComplete's searches push SpeedType 2, Wheel (`0x00446CC0`,
/// `0x00446E03`), not the free unit's own. Ground a Wheel unit crosses but a
/// Track unit cannot therefore stays a candidate: when the frame modulo
/// picks it, the Track CMIN's own Can_Enter_Cell refuses it on both
/// searches and the house is refunded (`0x00446E71..0x00446E8F`); on every
/// other frame the CMIN lands on the stock pool as before.
#[test]
fn free_unit_search_asks_wheel_ground_whatever_the_free_units_speed_type() {
    const WHEEL_ONLY: (u16, u16) = (19, 19);
    for frame in 0..(STOCK_4X3_FALLBACK_POOL.len() as u32 * 2) {
        let (sim, _, spawned) = complete_stock_allied_refinery_on(frame, &[], |cell| {
            if (cell.rx, cell.ry) == WHEEL_ONLY {
                cell.speed_costs.track = Some(0);
                cell.base_speed_costs.track = Some(0);
            }
        });
        let pick = STOCK_4X3_FALLBACK_POOL[(frame as usize) % STOCK_4X3_FALLBACK_POOL.len()];
        let cmin_ids = unit_ids(&sim, "Americans", "CMIN");
        if pick == WHEEL_ONLY {
            assert!(
                !spawned && cmin_ids.is_empty(),
                "frame {frame}: the Wheel-only pick is refused and refunded"
            );
            continue;
        }
        assert!(spawned, "frame {frame}");
        assert_eq!(cmin_ids.len(), 1, "frame {frame}: the FreeUnit is placed");
        let miner = sim.substrate.entities.get(cmin_ids[0]).unwrap();
        assert_eq!(
            (miner.position.rx(), miner.position.ry()),
            pick,
            "frame {frame}"
        );
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
    let rules = modded_refinery_rules("1x1", Some("MODHARV"));
    let mut sim = placement_sim();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 18, 18);
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
        (free_unit.position.rx(), free_unit.position.ry()),
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
            .contains_entity(20, 21, free_unit.stable_id())
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
        let cell = (miner.position.rx(), miner.position.ry());

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
                miner.stable_id()
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 14, 20);
    let refinery_id = ready_and_place(&mut sim, &rules, "Americans", "GAREFN", 20, 20);
    spawn_standing_blocker(&mut sim, &rules, (22, 22));
    for ry in 0..64 {
        for rx in 0..64 {
            grid.set_blocked(rx, ry, true);
        }
    }
    super::house_for_test(&mut sim, "Americans")
        .economy_mut()
        .set_credits_for_test(100);
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
                .and_then(|terrain| terrain.cell_mut_for_test(rx, ry))
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
            .resolve(entity.type_ref())
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
fn simultaneous_refinery_completions_preserve_stable_id_order() {
    let mut sim = placement_sim();
    let rules = stock_refinery_completion_rules();
    let mut grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 14, 20);
    spawn_structure(&mut sim, &rules, 2, "Russians", "NACNST", 14, 35);
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
fn refinery_without_free_unit_spawns_nothing_on_completion() {
    let rules = modded_refinery_rules("3x3", None);
    let mut sim = placement_sim();
    let grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 18, 18);
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
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if (cell.rx, cell.ry) == (31, 31) {
            cell.land_type = LandType::Rock.as_index();
        }
    }));
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 30, 30);
    spawn_structure(&mut sim, &rules, 2, "Americans", "GACNST", 40, 40);
    ready_building(&mut sim, &rules, "Americans", "GACNST");
    let place =
        |sim: &mut Simulation, cell| place_building(sim, &rules, "Americans", "GACNST", cell, None);

    // Rock under the foundation, then the other yard's cell.
    assert!(!place(&mut sim, (31, 31)));
    assert!(!place(&mut sim, (40, 40)));
    assert_eq!(
        ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        1,
        "invalid placement must not consume the ready building"
    );
    assert!(place(&mut sim, (32, 32)), "a clear cell takes it");
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
}

#[test]
fn placement_command_rejects_marked_ground_mobiles_until_they_are_unmarked() {
    let mut rules = ground_occupant_placement_rules();
    // A retained death has real ART and a native sequence clock. An absent
    // death record retires on the first class visit, before this command.
    let death_art = IniFile::from_str("[E1]\nSequence=E1Sequence\n[E1Sequence]\nDie1=56,15,0\n");
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&death_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&death_art),
    );
    let grid = PathGrid::new(64, 64);

    for blocker_type in ["MTNK", "E1"] {
        let mut sim = placement_sim();
        spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
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
                .set_for_test(0);
            sim.begin_infantry_death_sequence(
                blocker_id,
                crate::sim::world::InfantryDeathSequence::Die1,
                &rules,
                crate::sim::world::FrameEffects::default(),
            );
        }

        ready_building(&mut sim, &rules, "Americans", "GAPOWR");
        let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
        assert!(!preview.valid, "{blocker_type} must reject the preview");
        assert_eq!(
            preview
                .cells
                .iter()
                .map(|cell| cell.valid)
                .collect::<Vec<_>>(),
            vec![true, true, true, false],
            "only the non-origin foundation cell occupied by {blocker_type} should reject"
        );

        let americans = sim.interner.get("Americans").expect("owner interned");
        let gapowr = sim.interner.get("GAPOWR").expect("type interned");
        let entities_before = sim.substrate.entities.len();
        let next_id_before = sim.next_stable_object_id();
        let occupancy_generation_before = sim.substrate.occupancy.generation();
        let rejected = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
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
        assert_eq!(sim.next_stable_object_id(), next_id_before);
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
        let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
        assert!(
            preview.valid,
            "the same foundation must become legal after {blocker_type} is unmarked"
        );

        let accepted = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
        let tick = sim.advance_tick(&[accepted], Some(&rules), Some(&grid), None, 67);
        assert!(tick.spawned_entities);
        assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
        assert!(sim.substrate.entities.values().any(|entity| {
            entity.type_ref() == gapowr
                && entity.position.rx() == 12
                && entity.position.ry() == 10
                && entity.building_up()
        }));
    }
}

#[test]
fn placement_command_rejects_nonblocking_overlay_and_preserves_ready_building() {
    let mut sim = placement_sim();
    let rules = ground_occupant_placement_rules();
    let grid = PathGrid::new(64, 64);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);

    let mut overlay_grid = OverlayGrid::new(64, 64);
    overlay_grid.place_overlay(13, 11, 7, 4);
    sim.overlay_grid = Some(overlay_grid);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(!preview.valid, "any ordinary nonempty overlay must reject");
    assert_eq!(
        preview
            .cells
            .iter()
            .map(|cell| cell.valid)
            .collect::<Vec<_>>(),
        vec![true, true, true, false]
    );

    let americans = sim.interner.get("Americans").expect("owner interned");
    let gapowr = sim.interner.get("GAPOWR").expect("type interned");
    let entities_before = sim.substrate.entities.len();
    let next_id_before = sim.next_stable_object_id();
    let occupancy_generation_before = sim.substrate.occupancy.generation();
    let rejected = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
    let tick = sim.advance_tick(&[rejected], Some(&rules), Some(&grid), None, 67);

    assert_eq!(tick.executed_commands, 1);
    assert!(!tick.spawned_entities);
    assert_eq!(sim.substrate.entities.len(), entities_before);
    assert_eq!(sim.next_stable_object_id(), next_id_before);
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
    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(
        preview.valid,
        "the same foundation must become legal after the overlay is cleared"
    );

    let accepted = place_event(americans, gapowr, (12, 10), sim.session.tick() + 1);
    let tick = sim.advance_tick(&[accepted], Some(&rules), Some(&grid), None, 67);
    assert!(tick.spawned_entities);
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
}

#[test]
fn empty_cell_wall_placement_still_works_but_wall_on_overlay_rejects() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();

    let mut clear_sim = placement_sim();
    spawn_structure(&mut clear_sim, &rules, 1, "Americans", "GACNST", 10, 10);
    clear_sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut clear_sim, &rules, "Americans", "GAWALL");
    let preview = preview_at(
        &clear_sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(
        preview.valid,
        "the ordinary empty-cell wall preview must remain accepted: {:?}",
        preview.reason
    );
    let placed = place_building(
        &mut clear_sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(
        placed,
        "the ordinary empty-cell wall commit must remain accepted"
    );

    let mut overlay_sim = placement_sim();
    spawn_structure(&mut overlay_sim, &rules, 1, "Americans", "GACNST", 10, 10);
    overlay_sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
    let mut overlay_grid = OverlayGrid::new(64, 64);
    overlay_grid.place_overlay(12, 10, 7, 4);
    overlay_sim.overlay_grid = Some(overlay_grid);
    ready_building(&mut overlay_sim, &rules, "Americans", "GAWALL");
    let placement_credits = 1_337;
    super::house_for_test(&mut overlay_sim, "Americans")
        .economy_mut()
        .set_credits_for_test(placement_credits);
    assert!(overlay_sim.rebuild_dynamic_navigation(&rules));
    let owner = overlay_sim.interner.get("Americans").expect("owner");
    let wall = rules.object("GAWALL").expect("wall rules");
    assert_eq!(wall.cost, 100, "fixture must exercise a nonzero wall cost");
    let category = super::production_tech::production_category_for_object(wall);
    let factory_before = {
        let view = overlay_sim
            .production
            .factories()
            .view(owner, category)
            .expect("completed wall factory");
        (
            view.progress,
            view.on_hold,
            view.suspended,
            view.object.cloned(),
            view.queue.clone(),
        )
    };
    let held_id = factory_before
        .3
        .as_ref()
        .and_then(|object| object.entity_id)
        .expect("completed wall retains Factory+0x58 identity");
    let ready_types = |sim: &Simulation| {
        ready_buildings_for_owner(sim, &rules, "Americans")
            .into_iter()
            .map(|item| item.type_id)
            .collect::<Vec<_>>()
    };
    let ready_before = ready_types(&overlay_sim);
    assert_eq!(
        ready_before.len(),
        1,
        "the completed wall waits for placement"
    );
    let overlay_before = *overlay_sim
        .overlay_grid
        .as_ref()
        .expect("overlay grid")
        .cell(12, 10);
    let navigation_before = overlay_sim
        .path_grid_snapshot()
        .expect("published navigation before rejected placement");
    let preview = preview_at(
        &overlay_sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(!preview.valid, "an ordinary wall must not replace ore");
    let placed = place_building(
        &mut overlay_sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(!placed, "the occupied primary wall commit must be rejected");
    assert_eq!(
        ready_types(&overlay_sim),
        ready_before,
        "rejection must preserve the completed wall product"
    );
    let factory_after = {
        let view = overlay_sim
            .production
            .factories()
            .view(owner, category)
            .expect("rejected placement retains completed wall factory");
        (
            view.progress,
            view.on_hold,
            view.suspended,
            view.object.cloned(),
            view.queue.clone(),
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
    let (mut rules, registry) = gsi_04_07_wall_placement_contract();
    // A registered custom cue exercises RecordLastBuilt after Rust consumes
    // the temporary wall. Original440774 ->UnInit5F65F0 defers deletion;
    // Record4FB7DC reads its retained constructor Location, never the cell
    // being stamped. Only this test's sound input extends the wall fixture.
    let sound_ini = IniFile::from_str(
        "[SoundList]\n0=FixtureWallCreated\n[FixtureWallCreated]\nSounds=wall\n\
         [GAWALL]\nCreateSound=FixtureWallCreated\n",
    );
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&sound_ini);
    rules.bind_type_sound_references(&sound_ini, &sounds);
    let path_grid = PathGrid::new(64, 64);
    let mut sim = placement_sim();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    super::house_for_test(&mut sim, "Americans")
        .economy_mut()
        .set_credits_for_test(50_000);
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
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
            .factories_mut()
            .test_enqueue_kernel(owner, category, type_id, 1, cost)
    );
    // This fixture deliberately leaves BuildCat at native default0. After
    // Completed clears held, HouseAbandon removes the matching queued copy
    // first and retains the empty Factory; no successor Begin runs.
    let wall = rules.object("GAWALL").unwrap();
    assert!(super::can_build::find_factory(&sim, &rules, owner, wall, true, false, true).is_some());
    assert!(super::production_tech::strip_keeps(
        &sim, &rules, owner, wall
    ));
    let expected = sim.scenario_rng.clone();
    let entities_before = sim.substrate.entities.len();

    let tick = sim.advance_tick(
        &[place_event(
            owner,
            type_id,
            (12, 10),
            sim.session.tick() + 1,
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
        entities_before - 1,
        "BuildCat0 consumes the held wall without a successor constructor"
    );
    assert!(ready_buildings_for_owner(&sim, &rules, "Americans").is_empty());
    let cell = sim.overlay_grid.as_ref().unwrap().cell(12, 10);
    assert_eq!(cell.overlay_id, Some(2));
    assert_eq!(cell.overlay_data, 0);
    assert_eq!(cell.wall_owner, Some(owner));
    assert!(!sim.substrate.entities.values().any(|entity| {
        entity.type_ref() == type_id && (entity.position.rx(), entity.position.ry()) == (12, 10)
    }));
    assert_eq!(tick.state_hash, Some(sim.state_hash()));
    assert!(!sim.substrate.entities.contains(held));
    let completion_order = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::SimSoundEvent::BuildingPlaced { owner: event_owner }
                if *event_owner == owner =>
            {
                Some("BuildingSlam")
            }
            crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
                if sound_id == "FixtureWallCreated" =>
            {
                Some("CreateSound")
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        completion_order,
        ["BuildingSlam", "CreateSound"],
        "HousePlace4FB314 requests BuildingSlam before RecordLastBuilt4FB4B7"
    );
    let cues = sim
        .sound_events
        .iter()
        .filter_map(|event| {
            let crate::sim::world::SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } = event
            else {
                return None;
            };
            (sound_id == "FixtureWallCreated").then_some((
                *audible_to,
                *rx,
                *ry,
                *sub_x,
                *sub_y,
                *world_z_leptons,
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cues,
        [(
            None,
            0,
            0,
            crate::util::fixed_math::SimFixed::ZERO,
            crate::util::fixed_math::SimFixed::ZERO,
            0
        )],
        "consumed original wall still owns one raw-zero cue"
    );
    let retained = sim.production.factories().view(owner, category).unwrap();
    assert!(retained.object.is_none() && retained.queue.is_empty());
    assert!(!retained.is_running());
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(
        sim.production
            .factories()
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |_| {}));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let placement_credits = 1_337;
    super::house_for_test(&mut sim, "Americans")
        .economy_mut()
        .set_credits_for_test(placement_credits);
    let owner = sim.interner.get("Americans").expect("owner");
    let wall_type = sim.interner.get("GAWALL").expect("wall type");
    let wall = rules.object("GAWALL").expect("wall rules");
    assert_eq!(wall.cost, 100, "fixture must exercise a nonzero wall cost");
    let category = super::production_tech::production_category_for_object(wall);
    let held_id = sim
        .production
        .factories()
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
    let expected_rng = sim.scenario_rng.clone();
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
    let preview = preview_at(&sim, &rules, "Americans", "GAWALL", origin, Some(&registry));
    assert!(preview.valid, "primary wall cell should be legal");
    assert_eq!(
        preview
            .wall_autofill_cells
            .iter()
            .map(|cell| cell.coord)
            .collect::<Vec<_>>(),
        expected
    );

    sim.dirty_cells = Default::default();

    let placed = place_building(
        &mut sim,
        &rules,
        "Americans",
        "GAWALL",
        origin,
        Some(&registry),
    );
    assert!(placed);
    assert!(
        ready_buildings_for_owner(&sim, &rules, "Americans").is_empty(),
        "the primary plus all fillers consume the one ready product"
    );
    let retained = sim.production.factories().view(owner, category).unwrap();
    assert!(retained.object.is_none() && retained.queue.is_empty());
    assert_eq!(retained.progress, 0);
    assert!(!retained.is_running());
    // Each filler now reaches the shared constructor's Scenario draw. The
    // consumed original-code corpus compares full streams and request order;
    // this larger cardinal fixture checks that placement no longer skips them.
    assert!(sim.scenario_rng.logical_state() != expected_rng.logical_state());
    let retained = sim.substrate.entities.get(held_id).unwrap();
    assert!(!retained.lifecycle.object_alive() && retained.lifecycle.in_limbo());
    let pending: Vec<_> = sim.substrate.pending_delete.object_tickets().collect();
    assert_eq!(pending.len(), 1 + expected.len());
    assert_eq!(pending[0], held_id, "origin queues before the fillers");
    for id in pending {
        let object = sim.substrate.entities.get(id).unwrap();
        assert_eq!(object.type_ref(), wall_type);
        assert!(!object.lifecycle.object_alive() && object.lifecycle.in_limbo());
    }
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
            (rx, ry), // Object5F64B0 base Mark dirties before the wall arm.
            (rx, ry - 1),
            (rx + 1, ry),
            (rx, ry + 1),
            (rx - 1, ry),
            (rx, ry),
        ]);
    }
    assert_eq!(
        sim.dirty_cells.tactical_for_test(),
        expected_tactical,
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
    spawn_structure(&mut out_of_range, &rules, 1, "Americans", "GACNST", 10, 10);
    out_of_range.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut out_of_range, &rules, "Americans", "GAWALL");
    let owner = out_of_range.interner.get("Americans").expect("owner");
    out_of_range
        .overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(24, 18, overlay_id, 0x20, owner);
    let preview = preview_at(
        &out_of_range,
        &rules,
        "Americans",
        "GAWALL",
        (18, 18),
        Some(&registry),
    );
    assert!(
        preview.wall_autofill_cells.is_empty(),
        "GuardRange=5 must not close an endpoint six cells away"
    );

    let mut foreign_blocker = placement_sim();
    spawn_structure(
        &mut foreign_blocker,
        &rules,
        1,
        "Americans",
        "GACNST",
        10,
        10,
    );
    foreign_blocker.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut foreign_blocker, &rules, "Americans", "GAWALL");
    let owner = foreign_blocker.interner.get("Americans").expect("owner");
    let enemy = foreign_blocker.interner.intern("Russians");
    let grid = foreign_blocker.overlay_grid.as_mut().expect("overlay grid");
    grid.place_owned_wall(20, 18, overlay_id, 0x20, enemy);
    grid.place_owned_wall(23, 18, overlay_id, 0x20, owner);
    let preview = preview_at(
        &foreign_blocker,
        &rules,
        "Americans",
        "GAWALL",
        (18, 18),
        Some(&registry),
    );
    assert!(
        preview.wall_autofill_cells.is_empty(),
        "a foreign wall must block, not terminate, the direction"
    );
    let placed = place_building(
        &mut foreign_blocker,
        &rules,
        "Americans",
        "GAWALL",
        (18, 18),
        Some(&registry),
    );
    assert!(placed);
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "WALLKIT");
    assert!(registry.id_for_name("WALLKIT").is_none());

    let placed = place_building(
        &mut sim,
        &rules,
        "Americans",
        "WALLKIT",
        (12, 10),
        Some(&registry),
    );
    assert!(placed);
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").expect("owner");
    let overlay_id = registry.id_for_name("GAWALL").expect("wall overlay");
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(23, 18, overlay_id, 0x20, owner);
    let preview = preview_at(
        &sim,
        &rules,
        "Americans",
        "GAWALL",
        (18, 18),
        Some(&registry),
    );
    assert_eq!(
        preview
            .wall_autofill_cells
            .iter()
            .map(|cell| cell.coord)
            .collect::<Vec<_>>(),
        vec![(19, 18), (20, 18), (21, 18), (22, 18)]
    );

    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_overlay(20, 18, 7, 4);
    let placed = place_building(
        &mut sim,
        &rules,
        "Americans",
        "GAWALL",
        (18, 18),
        Some(&registry),
    );
    assert!(placed);
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
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
        .attack_target
        .set_for_test(Some(TargetKind::Cell(13, 10)));
    sim.dirty_cells = Default::default();

    let placed = place_building(
        &mut sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(placed);
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
            .target()
            .is_none(),
        "cleanup removal must broadcast CellClass pointer expiry before returning"
    );
    let expected_dirty = vec![(12, 9), (13, 10), (12, 11), (11, 10), (12, 10)];
    // Object base Mark publishes the anchor before wall connectivity's
    // existing N/E/S/W/self visits; radar belongs to connectivity only.
    let mut expected_tactical = vec![(12, 10)];
    expected_tactical.extend_from_slice(&expected_dirty);
    assert_eq!(sim.dirty_cells.tactical_for_test(), expected_tactical);
    assert_eq!(sim.dirty_cells.radar(), expected_dirty);
    assert_eq!(
        sim.dirty_cells.radar_generation(),
        5,
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
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    spawn_structure(&mut sim, &rules, 1, "Russians", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Russians", "GAWALL");
    let event_owner = sim.interner.intern("Americans");
    let wall_type = sim.interner.get("GAWALL").expect("wall type");

    let tick = sim.advance_tick(
        &[place_event(
            event_owner,
            wall_type,
            (12, 10),
            sim.session.tick() + 1,
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
    assert_eq!(tick.state_hash, Some(sim.state_hash()));
}

#[test]
fn gsi_04_07_wall_replacement_requires_damaged_same_type_and_owner_and_stays_local() {
    let (rules, registry) = gsi_04_07_wall_placement_contract();
    let mut sim = placement_sim();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").expect("owner");
    let enemy = sim.interner.intern("Russians");

    let preview = |sim: &Simulation| {
        preview_at(
            sim,
            &rules,
            "Americans",
            "GAWALL",
            (12, 10),
            Some(&registry),
        )
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
    let placed = place_building(
        &mut sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(placed);

    let overlay_grid = sim.overlay_grid.as_ref().unwrap();
    // House4FCFFB cleans the old anchor before replacement Mark. Removing
    //that connection isolates the stage-two neighbor;48089A..4808B8 removes
    //GAWALL data20 and preserves its stored owner. New Mark has no east link.
    assert_eq!(overlay_grid.cell(12, 10).overlay_data, 0);
    assert_eq!(overlay_grid.cell(12, 10).wall_owner, Some(owner));
    assert_eq!(overlay_grid.cell(13, 10).overlay_id, None);
    assert_eq!(overlay_grid.cell(13, 10).overlay_data, 0);
    assert_eq!(overlay_grid.cell(13, 10).wall_owner, Some(owner));
    assert_eq!(
        overlay_grid.cell(30, 30).overlay_data,
        0x1B,
        "placement does not globally rewrite wall frames"
    );
}

#[test]
fn owned_wall_replacement_rechecks_building_admission_after_removal() {
    let (mut rules, registry) = gsi_04_07_wall_placement_contract();
    // A scenario/forced-placement wall can cover nonbuildable ground. The
    // old owned damaged overlay admits the first Building test; once sold,
    // Building4407A0 must reject it even though Overlay's Track test accepts.
    rules.terrain_rules =
        RuleSet::from_ini(&IniFile::from_str("[Clear]\nBuildable=no\nTrack=100%\n"))
            .unwrap()
            .terrain_rules;
    let mut sim = placement_sim();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut sim, &rules, "Americans", "GAWALL");
    let owner = sim.interner.get("Americans").unwrap();
    let wall_type = sim.interner.get("GAWALL").unwrap();
    let product = ready_building_entity_for_owner(&sim, &rules, owner, wall_type).unwrap();
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(12, 10, 2, 0x20, owner);
    assert!(
        preview_at(
            &sim,
            &rules,
            "Americans",
            "GAWALL",
            (12, 10),
            Some(&registry)
        )
        .valid
    );
    assert!(!place_building(
        &mut sim,
        &rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry)
    ));
    let old_cell = sim.overlay_grid.as_ref().unwrap().cell(12, 10);
    assert_eq!(
        old_cell.overlay_id, None,
        "old wall sale is not rolled back"
    );
    assert_eq!(old_cell.wall_owner, None);
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    assert!(
        crate::sim::build_site::is_clear_to_build(
            &sim,
            &rules,
            Some(&registry),
            cells.lookup((12, 10)),
            Some(crate::rules::locomotor_type::SpeedType::Track),
            None,
            None,
            Some(&cells),
            None,
        ),
        "Overlay Track admission alone would still accept the exposed ground"
    );
    assert_eq!(
        ready_building_entity_for_owner(&sim, &rules, owner, wall_type),
        Some(product)
    );
    let held = sim.entities().get(product).unwrap();
    assert!(held.lifecycle.object_alive() && held.lifecycle.in_limbo());
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, crate::sim::world::SimSoundEvent::WallSold { .. }))
    );
}

#[test]
fn place_ready_building_requires_base_normal_provider_within_adjacent_range() {
    let rules = placement_radius_rules();

    let mut sim = placement_sim();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(placed);

    let mut far_sim = placement_sim();
    spawn_structure(&mut far_sim, &rules, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut far_sim, &rules, "Americans", "GAPOWR");
    // GACNST has Adjacent=6 (default), foundation 2x2 at (10,10).
    // Expanded zone: max_x = 10+2-1+7 = 18, so (20,10) is out of range.
    let placed = place_building(&mut far_sim, &rules, "Americans", "GAPOWR", (20, 10), None);
    assert!(!placed);
}

#[test]
fn base_normal_false_structures_do_not_extend_build_area() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAGAP", 10, 10);
    // PLACE needs this house's yard; keep it far from the gap generator, so
    // only GAGAP could cover (12,10).
    spawn_structure(&mut sim, &rules, 2, "Americans", "GACNST", 50, 50);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert_eq!(preview.reason, Some(BuildingPlacementError::OutOfBuildArea));
    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(!placed);
    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (52, 50), None);
    assert!(placed, "the yard's own area admits the same building");
}

/// BuildOffAlly lends an allied structure's build area only while the game
/// option is on and the structure is `EligibileForAllyBuilding=`.
#[test]
fn build_off_ally_lends_an_eligible_allied_provider_only_while_enabled() {
    let rules = build_off_ally_rules();
    for (build_off_ally, provider, admitted) in [
        (true, "GACNST", true),
        (false, "GACNST", false),
        (true, "GAPOWR", false),
    ] {
        let mut sim = placement_sim();
        sim.session.game_options_mut_for_test().build_off_ally = build_off_ally;
        spawn_structure(&mut sim, &rules, 1, "Alliance", provider, 10, 10);
        // PLACE still needs this house's producer; only its radius comes from
        // the nearby ally. Keep the own yard outside this placement's build area.
        spawn_structure(&mut sim, &rules, 2, "Americans", "GACNST", 50, 50);
        mark_allied(&mut sim, "Americans", "Alliance");
        ready_building(&mut sim, &rules, "Americans", "GAPOWR");

        let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
        assert_eq!(
            placed, admitted,
            "BuildOffAlly={build_off_ally}, {provider}"
        );
    }
}

/// BuildOffAlly lends the area only when the provider's house is allied
/// with the placer's (`HouseClass::IsAlliedWith @ 0x004F9A50`, asked by the
/// provider's house at `0x004A900E`): the placer's own one-way alliance with
/// the provider lends nothing.
#[test]
fn build_off_ally_asks_the_providers_house_about_the_placer() {
    let rules = build_off_ally_rules();
    for (asker, other, admitted) in [
        ("Alliance", "Americans", true),
        ("Americans", "Alliance", false),
    ] {
        let mut sim = placement_sim();
        sim.session.game_options_mut_for_test().build_off_ally = true;
        spawn_structure(&mut sim, &rules, 1, "Alliance", "GACNST", 10, 10);
        spawn_structure(&mut sim, &rules, 2, "Americans", "GACNST", 50, 50);
        sim.house_alliances_mut_for_test()
            .entry(asker.to_ascii_uppercase())
            .or_default()
            .insert(other.to_ascii_uppercase());
        ready_building(&mut sim, &rules, "Americans", "GAPOWR");

        let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
        assert_eq!(placed, admitted, "{asker} allies {other}");
    }
}

#[test]
fn placement_preview_keeps_shared_dummy_unchanged_across_unallocated_foundation_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 60, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let dummy = terrain.shared_cell_dummy();
    dummy.stamp_coord(-17, 92);
    dummy.set_level_slope(-128, 7);
    let before = dummy.snapshot();

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (63, 10), None);
    assert!(!preview.valid);
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
    assert_eq!(
        preview
            .cells
            .iter()
            .filter(|cell| cell.coord.0 == 64)
            .map(|cell| (cell.coord, cell.valid, cell.level, cell.slope))
            .collect::<Vec<_>>(),
        vec![((64, 10), false, -128, 7), ((64, 11), false, -128, 7)],
    );
    assert_eq!(
        dummy.snapshot(),
        before,
        "cursor admission must not stamp the live Dummy"
    );

    // Simulation admission still uses the canonical native lookup and retains
    // its final failed coordinate for subsequent gameplay consumers.
    assert!(!crate::sim::build_site::can_place_building_at(
        &sim,
        &rules,
        None,
        rules.object("GAPOWR").unwrap(),
        (63, 10),
        sim.interner.get("Americans"),
        None,
        None,
    ));
    assert_eq!(dummy.snapshot().coord, (64, 11));
}

#[test]
fn place_ready_building_rejects_bridge_deck_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 12 && cell.ry == 10 {
            cell.has_bridge_deck = true;
            cell.bridge_walkable = true;
            cell.bridge_transition = true;
            cell.bridge_deck_level = 3;
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
    }));

    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(!placed);

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
}

#[test]
fn place_ready_building_rejects_native_gap_restamp_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    let americans = sim.interner.intern("Americans");
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    sim.install_resolved_terrain_for_new_map(resolved_clear_grid_with_override(64, 64, |_| {}));
    assert!(crate::sim::build_site::can_place_building_at(
        &sim,
        &rules,
        None,
        rules.object("GAPOWR").unwrap(),
        (12, 10),
        Some(americans),
        None,
        None,
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

    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(!placed);

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
    assert!(
        !preview.cells[0].valid,
        "binary CellClass+0x140 bit 0x400 blocks placement even without live bridge deck flags"
    );
}

#[test]
fn place_ready_building_rejects_canonical_ramp_cells() {
    let mut sim = placement_sim();
    let rules = placement_radius_rules();

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 12 && cell.ry == 10 {
            cell.has_ramp = true;
            cell.canonical_ramp = Some(RampDirection::West);
            cell.slope_type = 1;
            cell.ground_walk_blocked = false;
        }
    }));

    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(!placed);

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
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

    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
    ready_building(&mut sim, &rules, "Americans", "GAPOWR");
    let resolved = resolved_clear_grid_with_override(64, 64, |cell| {
        if cell.rx == 12 && cell.ry == 10 {
            cell.ground_walk_blocked = true;
            cell.base_build_blocked = true;
            cell.has_bridge_deck = true;
            cell.bridge_walkable = true;
            cell.bridge_transition = true;
            cell.bridge_deck_level = 3;
            // A legacy deck whose +44 bridge identity 47E040 has destroyed:
            // state 0, bit0x100 clear, bit0x400 set.
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_DESTROYED_OR_RAMP;
        }
    });
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(&resolved, true, 5),
    );
    sim.resolved_terrain = Some(resolved);
    let deck = sim.overlay_cell_mut_for_test(12, 10);
    deck.overlay_id = Some(0x18);
    deck.overlay_data = 0;

    let preview = preview_at(&sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 10), None);
    assert!(!placed);
    let placed = place_building(&mut sim, &rules, "Americans", "GAPOWR", (12, 12), None);
    assert!(
        placed,
        "clear ground beside the destroyed deck admits the same building"
    );
}

#[test]
fn gsi_04_04_water_bound_building_accepts_water_but_not_beach_zone() {
    let rules = naval_yard_placement_rules();
    for (land_type, zone_type, admitted) in [
        (LandType::Beach, zone_class::BEACH, false),
        (LandType::Water, zone_class::WATER, true),
    ] {
        let mut sim = placement_sim();
        spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
        ready_building(&mut sim, &rules, "Americans", "GAYARD");
        sim.resolved_terrain = Some(resolved_clear_grid_with_override(64, 64, |cell| {
            if cell.rx == 20 && cell.ry == 20 {
                cell.land_type = land_type.as_index();
                cell.zone_type = zone_type;
                cell.base_build_blocked = true;
            }
        }));

        let placed = place_building(&mut sim, &rules, "Americans", "GAYARD", (20, 20), None);
        assert_eq!(placed, admitted, "{land_type:?}");
        if !admitted {
            let preview = preview_at(&sim, &rules, "Americans", "GAYARD", (20, 20), None);
            assert_eq!(preview.reason, Some(BuildingPlacementError::BlockedTerrain));
        }
    }
}

#[test]
fn producer_candidates_are_sorted_by_stable_id() {
    let mut sim = placement_sim();
    let rules = factory_rules();

    spawn_structure(&mut sim, &rules, 9, "Americans", "GAWEAP", 20, 20);
    spawn_structure(&mut sim, &rules, 3, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, &rules, 5, "Americans", "GAWEAP", 15, 15);

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

    spawn_structure(&mut sim, &rules, 3, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, &rules, 5, "Americans", "GAWEAP", 15, 15);
    spawn_structure(&mut sim, &rules, 9, "Americans", "GAWEAP", 20, 20);

    assert!(cycle_active_producer_for_owner_category(
        &mut sim,
        &rules,
        "Americans",
        ProductionCategory::Vehicle,
    ));
    assert_eq!(
        sim.production
            .primary_factory_ids(
                sim.interner.intern("Americans"),
                ProductionCategory::Vehicle
            )
            .next(),
        Some(3)
    );
    assert!(cycle_active_producer_for_owner_category(
        &mut sim,
        &rules,
        "Americans",
        ProductionCategory::Vehicle,
    ));
    assert_eq!(
        sim.production
            .primary_factory_ids(
                sim.interner.intern("Americans"),
                ProductionCategory::Vehicle
            )
            .next(),
        Some(5)
    );
}

#[test]
fn blocked_active_war_factory_does_not_spawn_from_second_factory() {
    let mut sim = placement_sim();
    let rules = factory_rules();
    let mut grid = PathGrid::new(64, 64);

    spawn_structure(&mut sim, &rules, 1, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, &rules, 2, "Americans", "GAWEAP", 30, 30);
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
    let held = super::lifecycle_tests::held_id(&sim, americans, ProductionCategory::Vehicle);
    assert!(
        sim.production
            .factories_mut()
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
        crate::sim::world::FrameEffects::default(),
    ));
    let produced = sim.substrate.entities.get(held).unwrap();

    assert_eq!(
        (produced.position.rx(), produced.position.ry()),
        (12, 11),
        "native444565 retains the selected producer's coordinate despite PathGrid blockage"
    );
    assert!(produced.has_live_contact_with(1));
    assert!(!produced.has_live_contact_with(2));
}

#[test]
fn spawn_routing_prefers_active_producer_when_available() {
    let mut sim = placement_sim();
    let rules = factory_rules();
    let grid = PathGrid::new(64, 64);
    sim.install_fixture_path_grid(Some(&grid));

    spawn_structure(&mut sim, &rules, 3, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, &rules, 5, "Americans", "GAWEAP", 30, 30);
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
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
        super::house_for_test(&mut sim, "Americans")
            .economy_mut()
            .set_credits_for_test(1000);
        spawn_structure(&mut sim, &rules, 1, "Americans", "GAPOWR", 20, 20);
        if let Some(ge) = sim.substrate.entities.get_mut(1) {
            ge.health = Health::new(health);
            ge.mark_live_contact_with(99);
        }
        let mut peer = GameEntity::test_default(99, "MTNK", "Americans", 22, 20);
        peer.set_owner_for_test(owner);
        peer.set_type_ref_for_test(sim.interner.intern("MTNK"));
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
    use crate::sim::c4_plant::PendingC4Detonation;
    use crate::sim::world::SimSoundEvent;
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n1=GAFWLL\n\
         [GAPOWR]\nOwner=Americans,Alliance,Russians,Soviet\nCost=800\nStrength=750\n\
         [GAFWLL]\nCost=100\nStrength=100\nFirestormWall=yes\n",
    ))
    .expect("sale admission rules should parse");
    let scene = |rules: &RuleSet, type_id: &str| {
        let mut sim = placement_sim();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
        super::house_for_test(&mut sim, "Americans")
            .economy_mut()
            .set_credits_for_test(1000);
        spawn_structure(&mut sim, rules, 1, "Americans", type_id, 20, 20);
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
        let mut sim = scene(&rules, "GAPOWR");
        assert!(!can_sell_building(&sim, &rules, 1));
        assert!(
            !sell_back(
                &mut sim,
                &rules,
                1,
                order,
                None,
                crate::sim::world::FrameEffects::default()
            ),
            "{order:?}"
        );
        assert!(!selling(&sim));
        assert_eq!(clicks(&sim), 0);

        let mut sim = scene(&rules, "GAFWLL");
        assert!(can_sell_building(&sim, &rules, 1));
        assert!(
            sell_back(
                &mut sim,
                &rules,
                1,
                order,
                None,
                crate::sim::world::FrameEffects::default()
            ),
            "{order:?}"
        );
        sim.flush_pending_delete();
        assert!(!sim.substrate.entities.contains(1), "{order:?}: removed");
        assert_eq!(clicks(&sim), 0);
        assert_eq!(credits_for_owner(&sim, "Americans"), 1000, "unpaid");
    }

    rules.set_buildup_control_for_test("GAPOWR", [0, 25, 2]);
    let mut sim = scene(&rules, "GAPOWR");
    assert!(can_sell_building(&sim, &rules, 1));
    assert!(sell_back(
        &mut sim,
        &rules,
        1,
        SellOrder::Player,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(selling(&sim));
    assert!(!can_sell_building(&sim, &rules, 1));
    let sale = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .mission
        .dispatch_timer();
    assert!(sell_back(
        &mut sim,
        &rules,
        1,
        SellOrder::Player,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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
    assert!(!sell_back(
        &mut sim,
        &rules,
        1,
        SellOrder::Computer,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert_eq!(clicks(&sim), 2);

    let mut sim = scene(&rules, "GAPOWR");
    assert!(sell_back(
        &mut sim,
        &rules,
        1,
        SellOrder::Computer,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(selling(&sim));
    assert_eq!(clicks(&sim), 1);

    let mut sim = scene(&rules, "GAPOWR");
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .pending_c4_detonation
        .set_for_test(Some(PendingC4Detonation::new(
            crate::sim::timer::CdTimer::started(0, 100),
            None,
        )));
    assert!(!sell_back(
        &mut sim,
        &rules,
        1,
        SellOrder::Computer,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(!selling(&sim));
    assert_eq!(clicks(&sim), 0);
    assert!(can_sell_building(&sim, &rules, 1));
    assert!(sell_back(
        &mut sim,
        &rules,
        1,
        SellOrder::Player,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(selling(&sim));
    assert_eq!(clicks(&sim), 1);
}

#[test]
fn sell_player_built_garrisoned_building_demolishes_and_ejects_alive() {
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let mut sim = placement_sim();
    let rules = sell_rules();
    super::house_for_test(&mut sim, "Americans")
        .economy_mut()
        .set_credits_for_test(0);

    // Spawn a CanBeOccupied building OWNED by Americans with NO original_owner
    // (player-built, not captured). NABNKR in sell_rules has Cost=0 so the
    // refund is 0 — this test pins the demolition path (entities.remove fired)
    // and the alive-eject of the occupant, not the refund magnitude.
    spawn_structure(&mut sim, &rules, 30, "Americans", "NABNKR", 40, 40);
    // The legacy structure helper writes active/occupation state directly.
    // Admit this integration fixture through complete Techno Unlimbo before
    // adding its occupant, rather than inventing the retained +508 value.
    sim.remove_entity_occupancy(30);
    sim.substrate
        .entities
        .get_mut(30)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
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
        t.passenger_role = PassengerRole::transport(PassengerCargo::new(5, 1.0));
    }
    let mut pax = crate::sim::game_entity::GameEntity::test_default(31, "E1", "Americans", 39, 40);
    pax.set_owner_for_test(amer_id);
    pax.set_type_ref_for_test(e1_id);
    pax.passenger_role = PassengerRole::inside_for_test(30, false);
    sim.substrate.entities.insert(pax);
    if let Some(t) = sim.substrate.entities.get_mut(30)
        && let Some(c) = t.passenger_role.cargo_mut()
    {
        c.board_forced_chain(&[(31, 1.0)]);
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
    assert!(!pax.death.is_dying(), "occupant should not be dying");
    assert!(pax.health.current() > 0, "occupant should be alive");
    assert!(pax.passenger_role.is_none(), "occupant role should be None");
}

#[test]
fn wall_mark_rejects_buildable_ground_that_track_cannot_enter() {
    // Overlay5FC6F6 passes SpeedType1 (Track), not Foot or BuildingType's
    // Buildable-only choice, into the shared Cell47C620 decision.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Clear]\nBuildable=yes\nFoot=100%\nTrack=0%\n",
    ))
    .unwrap();
    let registry = OverlayTypeRegistry::from_ini(
        &IniFile::from_str("[OverlayTypes]\n0=GASAND\n[GASAND]\nWall=yes\n"),
        None,
    );
    let mut sim = placement_sim();
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    let before = sim.state_hash();
    assert_eq!(
        sim.mark_overlay_for_test(&rules, &registry, (12, 12), 0),
        OverlayMarkOutcome::WallRefused
    );
    assert_eq!(
        sim.state_hash(),
        before,
        "refused Mark publishes no wall state"
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
    let rules = RuleSet::from_ini(&IniFile::from_str("[Clear]\nTrack=100%\n")).unwrap();
    let mut sim = placement_sim();
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(5, 5, |_| {}));
    sim.overlay_grid = Some(OverlayGrid::new(5, 5));
    assert_eq!(
        sim.mark_overlay_for_test(&rules, &registry, (2, 2), 0),
        OverlayMarkOutcome::Marked
    );
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
    let rules = RuleSet::from_ini(&IniFile::from_str("[Clear]\nTrack=100%\n")).unwrap();
    let mut sim = placement_sim();
    sim.resolved_terrain = Some(resolved_clear_grid_with_override(512, 2, |_| {}));
    sim.overlay_grid = Some(OverlayGrid::new(512, 2));
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(511, 0, 0, 0);
    assert_eq!(
        sim.mark_overlay_for_test(&rules, &registry, (0, 1), 0),
        OverlayMarkOutcome::Marked
    );
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 14, 20);
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
                .in_limbo()
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
    house.set_difficulty_for_test(crate::sim::house_state::HouseDifficulty::Hard);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
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

    assert!(super::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let category = ProductionCategory::Infantry;
    let held = super::lifecycle_tests::held_id(&sim, owner, category);
    let cost = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    let economy = sim.houses.get_mut(&owner).unwrap().economy_mut();
    assert!(
        economy.available_money() >= cost,
        "the wallet covers the build"
    );
    economy.spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );
    // The focused publisher helper increments both clocks before applying
    // due PLACE, so159 ->160 reaches the measured native fallback call frame.
    // No actor/House turns or native completion cadence are asserted here.
    sim.session.set_binary_frame_for_test(159);
    sim.session.set_tick_for_test(159);
    let event_start = sim.sound_events.len();
    assert!(super::dispatch_production_changes_for_tests(
        &mut sim,
        &rules,
        Some(&registry),
    ));
    assert_eq!(sim.session.binary_frame(), 160);
    assert_eq!(sim.session.tick(), 160);
    assert!(sim.pending_command_snapshot().is_empty());
    let product = sim.substrate.entities.get(held).unwrap();
    let position = ground_pose::position_world_coord(&product.position);
    assert_eq!([position.x, position.y, position.z], [3712, 3968, 0]);
    assert_eq!((product.position.rx(), product.position.ry()), (14, 15));
    assert_eq!(product.radio_contacts.slot(0), Some(producer));
    assert_eq!(product.tether.partner(), Some(producer));
    let producer_entity = sim.substrate.entities.get(producer).unwrap();
    assert_eq!(producer_entity.radio_contacts.slot(0), Some(held));
    assert_eq!(producer_entity.tether.partner(), Some(held));
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

/// `BuildingClass::Exit_Object_Main @ 0x00443C60` reaches its Cloning roster
/// loop in the InfantryType arm after the produced infantry's GetDock and
/// before its Unlimbo (`0x004449AF`, `0x004449D9..0x00444A53`, then
/// `0x00444C95`). This exercises the
/// stock Yuri barracks through the ordinary player PLACE publication/command
/// path; each same-house `[NACLON] Cloning=yes` vat should add one E1 for its
/// owner, even while that house is in low power.
fn stock_yuri_cloning_world() -> Option<(
    Simulation,
    RuleSet,
    OverlayTypeRegistry,
    crate::sim::intern::InternedId,
)> {
    stock_yuri_cloning_world_with_scenario(IniFile::empty())
}

fn stock_yuri_cloning_world_with_scenario(
    scenario: IniFile,
) -> Option<(
    Simulation,
    RuleSet,
    OverlayTypeRegistry,
    crate::sim::intern::InternedId,
)> {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    // These production-reader layers cover stock Battle type data without
    // depending on an installed map archive. The simulation below supplies
    // its own flat delivery map and empty scenario Rules pass; the relevant
    // type fields come through retail RULESMD, optional LANGRULE, and the
    // selected MPBattleMD layer.
    let rules_ini = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
    let fixed_art = crate::rules::retail_ini_fixture::retail_ini("artmd.ini")?;
    let mode_ini = crate::rules::retail_ini_fixture::retail_ini("MPBattleMD.ini")?;
    let mut layers = RulesLayerStack::new(rules_ini);
    let ini_dir = crate::test_fixture::checkout_root().join("ini");
    let langrule_name = std::fs::read_dir(&ini_dir)
        .expect("retail INI directory")
        .filter_map(Result::ok)
        .find_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("langrule.ini")
                .then(|| entry.file_name())
        });
    if let Some(langrule_name) = langrule_name {
        let langrule =
            crate::rules::retail_ini_fixture::retail_ini(&langrule_name.to_string_lossy())
                .expect("present LANGRULE.INI parses");
        layers.push(RulesLayerKind::LangRule, langrule);
    }
    layers.push(RulesLayerKind::GameMode, mode_ini);
    layers.push(RulesLayerKind::Scenario, scenario);
    let processed = layers
        .process_with_fixed_art(&fixed_art)
        .expect("stock Yuri Battle rules layers");
    let registry = OverlayTypeRegistry::from_ini(processed.ini(), Some(&fixed_art));
    let mut rules = RuleSet::from_processed_rules(&processed).expect("stock Yuri Battle rules");
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&fixed_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
    let mut sim = Simulation::with_seed(2);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);

    let owner = sim.interner.intern("YuriCountry");
    let mut house =
        crate::sim::house_state::HouseState::new(owner, 0, Some(owner), true, 10_000, 10);
    house.set_difficulty_for_test(crate::sim::house_state::HouseDifficulty::Hard);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().game_speed = 3;
    Some((sim, rules, registry, owner))
}

#[test]
fn stock_yuri_cloning_vat_duplicates_a_successfully_placed_infantry() {
    let Some((mut sim, rules, registry, owner)) = stock_yuri_cloning_world() else {
        return;
    };

    let other_owner = sim.interner.intern("Americans");
    let mut other_house = crate::sim::house_state::HouseState::new(
        other_owner,
        1,
        Some(other_owner),
        true,
        10_000,
        10,
    );
    other_house.project_country_mults(&rules, &sim.interner);
    sim.houses.register(other_house);

    sim.spawn_object_at_height_with_overlay_registry(
        "YABRCK",
        "YuriCountry",
        14,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("stock Yuri barracks enters through its constructor and Unlimbo");
    sim.spawn_object_at_height_with_overlay_registry(
        "NACLON",
        "YuriCountry",
        22,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("stock Yuri Cloning Vats enters through its constructor and Unlimbo");
    sim.spawn_object_at_height_with_overlay_registry(
        "NACLON",
        "YuriCountry",
        30,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("a second same-house vat preserves its own native roster entry");
    sim.spawn_object_at_height_with_overlay_registry(
        "NACLON",
        "Americans",
        46,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("the other house's vat is registered to that house only");

    assert!(super::enqueue_by_type(
        &mut sim,
        &rules,
        "YuriCountry",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let category = ProductionCategory::Infantry;
    let held = super::lifecycle_tests::held_id(&sim, owner, category);
    let cost = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    assert!(sim.houses.get(&owner).unwrap().economy().available_money() >= cost);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );
    let mut low_power = crate::sim::power_system::PowerState::default();
    low_power.set_is_low_power_for_test(true);
    sim.power_states.map_mut_for_test().insert(owner, low_power);

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim,
        &rules,
        Some(&registry),
    ));

    assert!(
        sim.substrate
            .entities
            .get(held)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
    let e1 = sim.interner.get("E1").unwrap();
    let infantry_count = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            entity.category() == EntityCategory::Infantry
                && entity.owner() == owner
                && entity.type_ref() == e1
        })
        .count();
    assert_eq!(
        infantry_count, 3,
        "the infantry producer's house should receive the original plus one copy per same-house vat"
    );
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .filter(|entity| {
                entity.category() == EntityCategory::Infantry
                    && entity.owner() == other_owner
                    && entity.type_ref() == e1
            })
            .count(),
        0,
        "a vat outside the producer house's native Cloning roster receives no copy"
    );
}

/// The native clone caller ignores each vat's Exit_Object return. The scenario
/// pass also makes the Cloning=yes vat an InfantryType factory, exercising the
/// nested-exit guard from its type flag. The shared `FreeRadio @ 0x0065ADC0`
/// admission rejects the full vat before candidate selection, but the created
/// infantry remains in limbo rather than being deleted or refunded.
#[test]
fn cloning_vat_keeps_a_refused_copy_in_limbo() {
    let scenario = IniFile::from_str("[NACLON]\nFactory=InfantryType\n");
    let Some((mut sim, rules, registry, owner)) = stock_yuri_cloning_world_with_scenario(scenario)
    else {
        return;
    };
    let barracks = sim
        .spawn_object_at_height_with_overlay_registry(
            "YABRCK",
            "YuriCountry",
            14,
            14,
            64,
            0,
            &rules,
            &registry,
        )
        .expect("stock Yuri barracks");
    let vat = sim
        .spawn_object_at_height_with_overlay_registry(
            "NACLON",
            "YuriCountry",
            22,
            14,
            64,
            0,
            &rules,
            &registry,
        )
        .expect("stock Yuri Cloning Vats");
    sim.substrate
        .entities
        .get_mut(vat)
        .unwrap()
        .radio_contacts
        .set_slot(0, barracks);

    assert!(super::enqueue_by_type(
        &mut sim,
        &rules,
        "YuriCountry",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let category = ProductionCategory::Infantry;
    let held = super::lifecycle_tests::held_id(&sim, owner, category);
    let cost = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim,
        &rules,
        Some(&registry),
    ));

    assert!(
        sim.substrate
            .entities
            .get(held)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
    let e1 = sim.interner.get("E1").unwrap();
    let copies: Vec<_> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            entity.category() == EntityCategory::Infantry
                && entity.owner() == owner
                && entity.type_ref() == e1
                && entity.stable_id() != held
        })
        .collect();
    assert_eq!(copies.len(), 1, "the native vat attempt creates one copy");
    assert!(
        !copies[0].lifecycle.cell_marked(),
        "a rejected clone remains present in limbo because the native caller ignores the exit result"
    );
}

/// The native clone-list helper is reached from the Infantry output arm;
/// stock `YTNK` follows the vehicle output arm and is not duplicated.
#[test]
fn cloning_vats_do_not_duplicate_yuri_vehicle_factory_output() {
    let Some((mut sim, rules, registry, owner)) = stock_yuri_cloning_world() else {
        return;
    };
    sim.spawn_object_at_height_with_overlay_registry(
        "YAWEAP",
        "YuriCountry",
        14,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("stock Yuri war factory");
    sim.spawn_object_at_height_with_overlay_registry(
        "NACLON",
        "YuriCountry",
        22,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("stock Yuri Cloning Vats");

    assert!(super::enqueue_by_type(
        &mut sim,
        &rules,
        "YuriCountry",
        "YTNK",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let category = ProductionCategory::Vehicle;
    let cost = sim.cost_of(owner, rules.object("YTNK").unwrap(), &rules);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );

    assert!(super::dispatch_production_changes_for_tests(
        &mut sim,
        &rules,
        Some(&registry),
    ));

    let ytnk = sim.interner.get("YTNK").unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .values()
            .filter(|entity| entity.owner() == owner && entity.type_ref() == ytnk)
            .count(),
        1,
        "only the original vehicle is produced"
    );
}

/// The clone loop (`0x004449D9`) follows the source's GetDock (`0x004449AF`):
/// an empty dock Cell takes the refusal at `0x00445696` first. With the source
/// moved outside Map::In_Bounds, no candidate Cell admits it, so no vat makes
/// a copy.
#[test]
fn cloning_vat_copies_wait_for_the_source_dock_cell() {
    let Some((mut sim, rules, registry, owner)) = stock_yuri_cloning_world() else {
        return;
    };
    let barracks = sim
        .spawn_object_at_height_with_overlay_registry(
            "YABRCK",
            "YuriCountry",
            14,
            14,
            64,
            0,
            &rules,
            &registry,
        )
        .expect("stock Yuri barracks");
    sim.spawn_object_at_height_with_overlay_registry(
        "NACLON",
        "YuriCountry",
        22,
        14,
        64,
        0,
        &rules,
        &registry,
    )
    .expect("stock Yuri Cloning Vats");

    assert!(super::enqueue_by_type(
        &mut sim,
        &rules,
        "YuriCountry",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let category = ProductionCategory::Infantry;
    let held = super::lifecycle_tests::held_id(&sim, owner, category);
    let cost = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );

    // Keep the producer's registration and contacts intact while making all
    // cells around its stored Location fail the existing Map::In_Bounds gate.
    let source = sim.substrate.entities.get_mut(barracks).unwrap();
    source.position.set_rx_for_test(0);
    source.position.set_ry_for_test(0);

    assert!(
        !super::dispatch_production_changes_for_tests(&mut sim, &rules, Some(&registry),),
        "the source Infantry cannot be delivered from outside the map diamond"
    );

    let e1 = sim.interner.get("E1").unwrap();
    let copies = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            entity.category() == EntityCategory::Infantry
                && entity.owner() == owner
                && entity.type_ref() == e1
                && entity.stable_id() != held
        })
        .count();
    assert_eq!(
        copies, 0,
        "GetDock refuses the source before the clone loop runs"
    );
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
    spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 18, 18);
    super::house_for_test(&mut sim, "Americans");
    ready_building(&mut sim, &rules, "Americans", "GACNST");
    let americans = sim.interner.intern("Americans");

    let far = place_building(&mut sim, &rules, "Americans", "GACNST", (60, 60), None);
    assert!(!far);
    assert!(building_placed_owners(&sim).is_empty());

    let near = place_building(&mut sim, &rules, "Americans", "GACNST", (20, 20), None);
    assert!(near);
    assert_eq!(building_placed_owners(&sim), vec![americans]);

    let (wall_rules, registry) = gsi_04_07_wall_placement_contract();
    let mut wall_sim = placement_sim();
    spawn_structure(&mut wall_sim, &rules, 1, "Americans", "GACNST", 10, 10);
    wall_sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    ready_building(&mut wall_sim, &wall_rules, "Americans", "GAWALL");
    let wall = place_building(
        &mut wall_sim,
        &wall_rules,
        "Americans",
        "GAWALL",
        (12, 10),
        Some(&registry),
    );
    assert!(wall);
    let wall_owner = wall_sim.interner.get("Americans").expect("owner");
    assert_eq!(building_placed_owners(&wall_sim), vec![wall_owner]);
}

/// ExitObject's aircraft arm (`BuildingClass::ExitObject_Main @ 0x00443CB4`):
/// the airfield unlimboes each aircraft it builds at the dock coordinate for
/// it, a contact it does not hold yet answering the centre (`0x00447B20`);
/// HELLO links the next free slot (TETHER leaves an `AirportBound=` aircraft
/// untethered, `0x006F4B4B`), and the aircraft steps onto that slot's
/// `DockingOffset` and takes the airfield as its dock (`+0x6CC`).
#[test]
fn an_airfield_puts_each_aircraft_it_builds_on_its_own_pad() {
    use crate::sim::movement::ground_pose;
    let mut rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n0=ORCA\n\
         [BuildingTypes]\n0=GAAIRC\n\
         [ORCA]\nCost=1200\nStrength=150\nSpeed=14\nTechLevel=1\nOwner=Americans\nAmmo=1\n\
         Landable=yes\nAirportBound=yes\nFighter=yes\nPrimary=TestGun\nDock=GAAIRC\n\
         Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
         [GAAIRC]\nOwner=Americans\nStrength=1000\nFactory=AircraftType\nHelipad=yes\n\
         UnitReload=yes\nNumberOfDocks=4\n\
         [TestGun]\nDamage=10\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
         [TestShot]\nROT=0\n[TestWH]\nVerses=100%\n",
    ))
    .expect("airfield rules");
    rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
        "[GAAIRC]\nFoundation=3x2\nDockingOffset0=0,-128,0\nDockingOffset1=0,128,0\n\
         DockingOffset2=256,-128,0\nDockingOffset3=256,128,0\n",
    )));
    let mut sim = placement_sim();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 50_000, 10,
        ));
    let airfield = sim
        .spawn_object("GAAIRC", "Americans", 20, 20, 0, &rules)
        .expect("airfield");
    let centre = ground_pose::object_get_coords(
        sim.substrate.entities.get(airfield).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    for (slot, [dx, dy]) in [[0, -128], [0, 128]].into_iter().enumerate() {
        assert!(super::enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            "ORCA",
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let built = super::lifecycle_tests::held_id(&sim, owner, ProductionCategory::Aircraft);
        let cost = sim.cost_of(owner, rules.object("ORCA").unwrap(), &rules);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut()
            .spend_money(cost);
        assert!(
            sim.production
                .factories_mut()
                .test_arm_ready(owner, ProductionCategory::Aircraft)
        );
        assert!(super::dispatch_production_changes_for_tests(
            &mut sim, &rules, None
        ));
        let plane = sim.substrate.entities.get(built).unwrap();
        assert!(!plane.lifecycle.in_limbo(), "slot {slot}: placed");
        let at = ground_pose::position_world_coord(&plane.position);
        assert_eq!(
            [at.x, at.y],
            [centre.x + dx, centre.y + dy],
            "slot {slot}: on its pad"
        );
        assert_eq!(plane.radio_contacts.slot(0), Some(airfield));
        assert_eq!(plane.tether.partner(), None, "TETHER");
        assert_eq!(
            plane.ammo().and_then(|ammo| ammo.dock()),
            Some(airfield),
            "+0x6CC"
        );
        assert_eq!(plane.mission.current().known(), Some(MissionType::Guard));
        let pad = sim.substrate.entities.get(airfield).unwrap();
        assert_eq!(pad.radio_contacts.slot(slot), Some(built));
    }
}
