//! Walls under a new building (`BuildingClass::Unlimbo @ 0x00440580`)
//! and connections at its ends (`0x0044104D..0x0044115E`), on a flat arena.

use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::overlay_grid::{OverlayGrid, WallPointerTarget};
use crate::sim::world::{Simulation, UninitContext};

const RULES: &str = "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
     [BuildingTypes]\n0=TOWER\n1=GAWALL\n2=GATE\n3=POST\n4=FENCE\n5=MODWALL\n\
     [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n3=MODWALL\n\
     [General]\nWallTower=TOWER\nGDIGateOne=GATE\n\
     [TOWER]\nStrength=500\n\
     [GATE]\nStrength=500\nGate=yes\n\
     [POST]\nStrength=500\nLaserFencePost=yes\n\
     [FENCE]\nStrength=300\nLaserFence=yes\n\
     [GASAND]\nWall=yes\n[CYCL]\n[GAWALL]\nStrength=300\nWall=yes\n\
     [MODWALL]\nStrength=300\nWall=yes\n\
     [Clear]\nBuildable=yes\nFloat=0%\n";

const ART: &str = "[TOWER]\nFoundation=1x1\n[GATE]\nFoundation=1x1\n[POST]\nFoundation=1x1\n\
     [FENCE]\nFoundation=1x1\n[GAWALL]\nFoundation=1x1\nToOverlay=GAWALL\n\
     [MODWALL]\nFoundation=1x1\nToOverlay=MODWALL\n";

const GAWALL: u8 = 2;
const MODWALL: u8 = 3;

fn arena(rules_ini: &str) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    arena_with_art(rules_ini, ART)
}

fn arena_with_art(rules_ini: &str, art_ini: &str) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(rules_ini);
    let art = IniFile::from_str(art_ini);
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
    let (mut sim, rules, registry) = arena(RULES);
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
fn an_ares_gate_sells_its_owners_mod_wall_under_it() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules_ini = RULES.replace("[General]\n", "[General]\nDegrade.Enabled=no\n");
    let (mut sim, rules, registry) = arena(&rules_ini);
    assert!(rules.ares_form());
    let owner = sim.interner.get("Americans").unwrap();
    let grid = sim.overlay_grid.as_mut().unwrap();
    grid.place_owned_wall(10, 10, MODWALL, 0, owner);
    grid.place_owned_wall(11, 10, MODWALL, 0, owner);
    sim.spawn_object_with_overlay_registry("GATE", "Americans", 10, 10, 0, &rules, &registry)
        .expect("an Ares gate stands on its owner's wall");
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(grid.cell(10, 10).overlay_id, None);
    assert_eq!(grid.cell(10, 10).wall_owner, None);
    assert_eq!(grid.cell(11, 10).overlay_id, Some(MODWALL));
}

fn gate_connection_arena() -> (Simulation, RuleSet, OverlayTypeRegistry) {
    // Stock GAWALL and NAWALL use overlay array positions 2 and 26.
    let reserved: String = (4..26).map(|i| format!("{i}=UnusedOverlay{i}\n")).collect();
    let rules = RULES
        .replace("5=MODWALL\n", "5=MODWALL\n6=NAWALL\n7=NGATE\n")
        .replace("[General]", &format!("{reserved}26=NAWALL\n[General]"))
        .replace("GDIGateOne=GATE", "GDIGateOne=GATE\nNodGateTwo=NGATE")
        .replace(
            "[Clear]",
            "[NAWALL]\nStrength=300\nWall=yes\n[NGATE]\nStrength=500\nGate=yes\n[Clear]",
        );
    let art = ART.replace("[GATE]\nFoundation=1x1", "[GATE]\nFoundation=3x1")
        + "[NGATE]\nFoundation=1x3\n[NAWALL]\nFoundation=1x1\nToOverlay=NAWALL\n";
    arena_with_art(&rules, &art)
}

#[test]
fn placing_and_removing_a_gate_updates_existing_concrete_wall_ends() {
    let _stock = crate::rules::ares_switch::read_ares_way(false);
    let (mut sim, rules, registry) = gate_connection_arena();
    assert!(!rules.ares());
    for (x, y) in [(8, 10), (9, 10), (13, 10), (10, 9)] {
        sim.spawn_object_with_overlay_registry("GAWALL", "Americans", x, y, 0, &rules, &registry)
            .expect("place concrete wall");
    }
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_overlay_data(9, 10, 0x18);
    let gate = sim
        .spawn_object_with_overlay_registry("GATE", "Russians", 10, 10, 0, &rules, &registry)
        .expect("place east-west gate beside the other house's walls");
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(
        grid.cell(9, 10).overlay_data,
        0x1A,
        "preserve damage and wall links"
    );
    assert_eq!(
        grid.cell(13, 10).overlay_data,
        0x08,
        "connect the opposite end"
    );
    assert_eq!(
        grid.cell(10, 9).overlay_data,
        0,
        "ignore a perpendicular wall"
    );
    sim.apply_wall_damage(WallPointerTarget::Real(10, 9), -1, &registry, &rules);
    let grid = sim.overlay_grid.as_mut().unwrap();
    assert_eq!(
        grid.cell(9, 10).overlay_data,
        0x1A,
        "keep the live gate link"
    );
    let owner = grid.cell(13, 10).wall_owner;
    assert!(owner.is_some());
    grid.set_overlay_data(13, 10, 0x28);

    sim.uninit_with_context(gate, UninitContext::new(&rules));
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(
        grid.cell(9, 10).overlay_data,
        0x18,
        "drop only the gate link"
    );
    assert_eq!(
        grid.cell(13, 10).overlay_id,
        None,
        "clean up the isolated damaged wall"
    );
    assert_eq!(grid.cell(13, 10).wall_owner, owner);
}

#[test]
fn placing_a_wall_connects_to_an_existing_gate_of_the_matching_family_and_axis() {
    let _stock = crate::rules::ares_switch::read_ares_way(false);
    let (mut sim, rules, registry) = gate_connection_arena();
    sim.spawn_object_with_overlay_registry("NGATE", "Americans", 10, 10, 0, &rules, &registry)
        .expect("place north-south gate");
    for (wall, x, y) in [("NAWALL", 10, 9), ("NAWALL", 11, 10), ("GAWALL", 10, 13)] {
        sim.spawn_object_with_overlay_registry(wall, "Americans", x, y, 0, &rules, &registry)
            .expect("place wall beside gate");
    }
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(grid.cell(10, 9).overlay_id, Some(26));
    assert_eq!(
        grid.cell(10, 9).overlay_data,
        0x04,
        "connect south into Nod gate"
    );
    assert_eq!(
        grid.cell(11, 10).overlay_data,
        0,
        "ignore the gate's long side"
    );
    assert_eq!(
        grid.cell(10, 13).overlay_data,
        0,
        "ignore a different wall family"
    );
}

#[test]
fn a_laser_fence_post_removes_its_owners_fence_building_in_its_cell() {
    let (mut sim, rules, registry) = arena(RULES);
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
