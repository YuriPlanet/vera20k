//! `BuildingClass::Unlimbo @ 0x00440580`'s arms that remove what the new
//! building stands on (`0x004405BC..0x00440770`, `0x00440A6D..0x00440B01`),
//! on the flat two-house arena. The rules text is this fixture's own.

use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::world::Simulation;

const RULES: &str = "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
     [BuildingTypes]\n0=TOWER\n1=GAWALL\n2=GATE\n3=POST\n4=FENCE\n\
     [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
     [General]\nWallTower=TOWER\nGDIGateOne=GATE\n\
     [TOWER]\nStrength=500\n\
     [GATE]\nStrength=500\nGate=yes\n\
     [POST]\nStrength=500\nLaserFencePost=yes\n\
     [FENCE]\nStrength=300\nLaserFence=yes\n\
     [GASAND]\nWall=yes\n[CYCL]\n[GAWALL]\nStrength=300\nWall=yes\n\
     [Clear]\nBuildable=yes\nFloat=0%\n";

const ART: &str = "[TOWER]\nFoundation=1x1\n[GATE]\nFoundation=1x1\n[POST]\nFoundation=1x1\n\
     [FENCE]\nFoundation=1x1\n[GAWALL]\nFoundation=1x1\nToOverlay=GAWALL\n";

const GAWALL: u8 = 2;

fn arena() -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(RULES);
    let art = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("fixture rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    (sim, rules, registry)
}

/// Places `ty` for the human house on its own GAWALL at (10, 10), beside a
/// second own wall at (11, 10), and returns the two cells' overlays.
fn overlays_after_placing_on_own_wall(ty: &str) -> [Option<u8>; 2] {
    let (mut sim, rules, registry) = arena();
    let owner = sim.interner.get("Americans").unwrap();
    let grid = sim.overlay_grid.as_mut().unwrap();
    grid.place_owned_wall(10, 10, GAWALL, 0, owner);
    grid.place_owned_wall(11, 10, GAWALL, 0, owner);
    sim.spawn_object_with_overlay_registry(ty, "Americans", 10, 10, 0, &rules, &registry)
        .expect("the building stands on its owner's wall");
    let grid = sim.overlay_grid.as_ref().unwrap();
    [grid.cell(10, 10).overlay_id, grid.cell(11, 10).overlay_id]
}

#[test]
fn the_wall_tower_sells_its_owners_wall_under_it() {
    assert_eq!(
        overlays_after_placing_on_own_wall("TOWER"),
        [None, Some(GAWALL)]
    );
}

#[test]
fn a_gate_sells_its_owners_walls_under_its_foundation() {
    assert_eq!(
        overlays_after_placing_on_own_wall("GATE"),
        [None, Some(GAWALL)]
    );
}

#[test]
fn a_laser_fence_post_removes_its_owners_fence_building_in_its_cell() {
    let (mut sim, rules, registry) = arena();
    let spawn = |sim: &mut Simulation, ty: &str| {
        sim.spawn_object_with_overlay_registry(ty, "Americans", 10, 10, 0, &rules, &registry)
    };
    spawn(&mut sim, "FENCE").expect("fence stands");
    let post = spawn(&mut sim, "POST").expect("the post shares its owner's fence cell");
    let buildings: Vec<_> = sim
        .cell_objects((10, 10), MovementLayer::Ground)
        .filter(|member| match *member {
            CellObjectMember::Entity(id) => sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure),
            CellObjectMember::Terrain(_) => false,
        })
        .collect();
    assert_eq!(buildings, [CellObjectMember::Entity(post)]);
}
