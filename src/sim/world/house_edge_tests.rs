//! `HouseClass::DetermineEdge @ 0x0050DB00` through its production callers,
//! Unlimbo's and ChangeOwner's SelectPrimaryFactory (`0x00448070`) and the
//! manual TogglePrimaryFactory (`0x00448160`), on retail NewHghts.mmx bounds.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::sim::command::Command;
use crate::sim::house_state::determine_waypoint_edge;

const NORTH: u8 = 0;
const EAST: u8 = 1;
const SOUTH: u8 = 2;
const WEST: u8 = 3;

struct NewHeights {
    rules: RuleSet,
    bounds: PlayfieldBounds,
    /// Start waypoints 0 and 1.
    starts: [(u16, u16); 2],
}

/// Retail Battle rules for NewHghts.mmx and that map's own Size/LocalSize and
/// start waypoints, through the production readers.
fn new_heights() -> Option<NewHeights> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("NewHghts.mmx")?;
    let (root, assets) = crate::rules::retail_ini_fixture::retail_scenario_assets()?;
    let map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "NewHghts.mmx", &assets)
            .expect("load retail NewHghts.mmx")
            .map;
    let start = |index| {
        let waypoint = map.waypoints[&index];
        (waypoint.rx, waypoint.ry)
    };
    Some(NewHeights {
        rules: retail.rules,
        bounds: PlayfieldBounds::from_map_header(&map.header),
        starts: [start(0), start(1)],
    })
}

/// A clear arena holding both starts' yards (`Americans` human, `Russians`
/// computer) with the map's playfield bounds.
fn arena(fixture: &NewHeights) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&fixture.rules, 0x50DB00, 96);
    sim.intern_rule_type_ids(&fixture.rules);
    sim.resolve_type_handles(&fixture.rules);
    sim.set_playfield_bounds_for_test(Some(fixture.bounds));
    sim
}

/// The GACNST an MCV standing on `start` deploys, through Unlimbo.
fn yard_at(sim: &mut Simulation, rules: &RuleSet, owner: &str, start: (u16, u16)) -> u64 {
    let foundation = &rules.object("GACNST").unwrap().foundation;
    let (x, y) =
        crate::sim::world::world_spawn::deploy_origin_from_unit_cell(start.0, start.1, foundation);
    sim.spawn_object("GACNST", owner, x as u16, y as u16, 0, rules)
        .expect("GACNST unlimbos")
}

fn edge(sim: &Simulation, owner: &str) -> u8 {
    sim.houses[&sim.interner.get(owner).unwrap()].waypoint_edge()
}

fn toggle(sim: &mut Simulation, rules: &RuleSet, entity_id: u64) {
    assert!(sim.apply_command(
        "Americans",
        &Command::TogglePrimaryFactory { entity_id },
        Some(rules),
    ));
}

/// NewHghts.mmx start 0: the deployed yard's north-west cell is nearest the
/// North reference, but its foundation centre, two cells further south-east,
/// is nearest West. The House starts North (constructor) and takes West.
#[test]
fn a_deployed_yard_takes_the_edge_nearest_its_foundation_centre() {
    let Some(fixture) = new_heights() else {
        return;
    };
    let rules = &fixture.rules;
    let mut sim = arena(&fixture);
    let start = fixture.starts[0];
    let facing = rules.object("GACNST").unwrap().deploy_facing;
    let mcv = sim
        .spawn_object("AMCV", "Americans", start.0, start.1, facing, rules)
        .unwrap();
    assert_eq!(edge(&sim, "Americans"), NORTH);

    assert!(sim.deploy_mcv(mcv, rules, None, FrameEffects::default()));

    let yard = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "GACNST")
        .expect("the MCV deployed")
        .stable_id();
    let entity = sim.substrate.entities.get(yard).unwrap();
    let north_west = (entity.position.rx(), entity.position.ry());
    assert_eq!(north_west, (start.0 - 1, start.1 - 1));
    assert_eq!(
        determine_waypoint_edge(north_west, fixture.bounds),
        NORTH,
        "the north-west cell must measure differently for this to discriminate"
    );
    assert!(sim.production.is_primary_factory(yard));
    assert_eq!(edge(&sim, "Americans"), WEST);
}

/// A second yard leaves the edge with the primary yard: SelectPrimaryFactory
/// keeps the first yard's flag, and the House+68 walk stops at that yard.
/// Making the second yard primary moves the edge to it; clearing its flag
/// does not call DetermineEdge (`0x00448189` jumps to `0x00448246`).
#[test]
fn a_second_yard_moves_the_edge_only_while_made_primary() {
    let Some(fixture) = new_heights() else {
        return;
    };
    let rules = &fixture.rules;
    let mut sim = arena(&fixture);
    let first = yard_at(&mut sim, rules, "Americans", fixture.starts[0]);
    assert_eq!(edge(&sim, "Americans"), WEST);

    let second = yard_at(&mut sim, rules, "Americans", fixture.starts[1]);
    assert!(sim.production.is_primary_factory(first));
    assert!(!sim.production.is_primary_factory(second));
    assert_eq!(edge(&sim, "Americans"), WEST);

    toggle(&mut sim, rules, second);
    assert!(!sim.production.is_primary_factory(first));
    assert!(sim.production.is_primary_factory(second));
    assert_eq!(edge(&sim, "Americans"), SOUTH);

    toggle(&mut sim, rules, second);
    assert!(!sim.production.is_primary_factory(second));
    assert_eq!(edge(&sim, "Americans"), SOUTH);

    toggle(&mut sim, rules, first);
    assert_eq!(edge(&sim, "Americans"), WEST);
}

/// ChangeOwner's SelectPrimaryFactory runs for the new owner only. A House
/// that already has a primary yard keeps it and its edge; a House without a
/// yard makes the captured yard primary and measures from its centre. The old
/// owner's edge stays as it was.
#[test]
fn a_captured_yard_moves_only_a_yardless_captors_edge() {
    let Some(fixture) = new_heights() else {
        return;
    };
    let rules = &fixture.rules;
    let mut sim = arena(&fixture);
    let americans = sim.interner.get("Americans").unwrap();
    let russians = sim.interner.get("Russians").unwrap();
    let american_yard = yard_at(&mut sim, rules, "Americans", fixture.starts[0]);
    let russian_yard = yard_at(&mut sim, rules, "Russians", fixture.starts[1]);
    assert_eq!(edge(&sim, "Americans"), WEST);
    assert_eq!(edge(&sim, "Russians"), SOUTH);

    sim.change_owner_with_rules(
        russian_yard,
        americans,
        rules,
        None,
        FrameEffects::default(),
    );
    assert!(sim.production.is_primary_factory(american_yard));
    assert!(!sim.production.is_primary_factory(russian_yard));
    assert_eq!(edge(&sim, "Americans"), WEST);
    assert_eq!(edge(&sim, "Russians"), SOUTH);

    sim.change_owner_with_rules(
        american_yard,
        russians,
        rules,
        None,
        FrameEffects::default(),
    );
    assert!(sim.production.is_primary_factory(american_yard));
    assert_eq!(edge(&sim, "Russians"), WEST);
    assert_eq!(edge(&sim, "Americans"), WEST);
}

/// The premise of [`Simulation::determine_house_edge`]'s list-order residual:
/// every stock `Factory=BuildingType` type is a `ConstructionYard=` type, so
/// a primary one stops the House+68 walk.
#[test]
fn every_stock_building_factory_is_a_construction_yard() {
    let Some(fixture) = new_heights() else {
        return;
    };
    let factories: Vec<_> = fixture
        .rules
        .all_objects()
        .filter(|object| {
            object.factory_type() == Some(crate::rules::object_type::FactoryType::BuildingType)
        })
        .collect();
    assert!(!factories.is_empty());
    for object in factories {
        assert!(object.construction_yard, "{}", object.id);
    }
}

/// The walk's other arms (`0x0050DB73..0x0050DC34`), which retail callers do
/// not reach: with no primary yard it keeps the last live yard; with no live
/// yard, the first other live Building; with neither, the edge stays.
#[test]
fn the_house_walk_falls_back_to_the_last_yard_then_the_first_building() {
    let Some(fixture) = new_heights() else {
        return;
    };
    let rules = &fixture.rules;
    let mut sim = arena(&fixture);
    let owner = sim.interner.get("Americans").unwrap();
    // The 2x2 power plant's and the 4x4 yards' centre cells (65,35), (35,33)
    // and (65,65) lie nearest East, North and South.
    let plant = sim
        .spawn_object("GAPOWR", "Americans", 64, 34, 0, rules)
        .unwrap();
    let north = sim
        .spawn_object("GACNST", "Americans", 33, 31, 0, rules)
        .unwrap();
    let south = sim
        .spawn_object("GACNST", "Americans", 63, 63, 0, rules)
        .unwrap();
    assert!(!sim.production.is_primary_factory(south));
    assert_eq!(edge(&sim, "Americans"), NORTH);

    sim.production.clear_primary_factory(north);
    sim.determine_house_edge(owner, south, rules);
    assert_eq!(edge(&sim, "Americans"), SOUTH);

    sim.techno_limbo_with_rules(north, rules, None);
    sim.techno_limbo_with_rules(south, rules, None);
    sim.determine_house_edge(owner, south, rules);
    assert_eq!(edge(&sim, "Americans"), EAST);

    sim.techno_limbo_with_rules(plant, rules, None);
    sim.determine_house_edge(owner, south, rules);
    assert_eq!(edge(&sim, "Americans"), EAST);
}
