//! Rust regression tests for the urgency-0 code-2 chain of
//! `AStar_compute_edge_cost @ 0x00429830` over live cell lists
//! ([`Simulation::astar_code2_chain_jams`]). Expected behaviour comes from
//! instruction reading of `0x00429886..0x00429984`, except where a test
//! replays the original executable's Hills controls.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::{FootPathQueue, NavTargetRef};
use crate::sim::movement::FacingClass;
use crate::sim::pathfinding::code2_multiplier;
use crate::util::fixed_math::SIM_ONE;

const NORTH: u8 = 0;
const NORTH_EAST: u8 = 1;
const EAST: u8 = 2;

/// A tank and a one-cell building.
fn rules() -> RuleSet {
    // This synthetic scene needs native buildable Clear ground and the
    // explicitly intended one-cell HUT foundation in the fixed ART reader.
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=HUT\n\
         [TANK]\nStrength=300\nSpeed=6\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [HUT]\nStrength=100\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str("[HUT]\nFoundation=1x1\n"))
        .expect("chain rules")
}

fn arena(rules: &RuleSet) -> Simulation {
    crate::sim::arena_fixture::rival_arena(rules, 0x429830)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, type_name: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object_at_height(type_name, "Americans", cell.0, cell.1, 0, 0, rules)
        .expect("spawns")
}

/// A stopped tank (speed fraction 0) whose path head word steps `head`,
/// facing north.
fn stopped(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16), head: Option<u8>) -> u64 {
    let id = spawn(sim, rules, "TANK", cell);
    let tank = sim.substrate.entities.get_mut(id).unwrap();
    tank.foot_speed.set_speed_fraction(SIM_ZERO);
    tank.body_facing = FacingClass::new(0, 0);
    tank.navigation.path_replay = FootPathQueue::for_test(
        head.into_iter().collect(),
        0,
        Some((cell.0 as i16, cell.1 as i16)),
    );
    id
}

/// A moving tank (full speed fraction) facing `facing`, whose head word
/// steps north.
fn moving(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16), facing: u16) -> u64 {
    let id = stopped(sim, rules, cell, Some(NORTH));
    let tank = sim.substrate.entities.get_mut(id).unwrap();
    tank.foot_speed.set_speed_fraction(SIM_ONE);
    tank.body_facing = FacingClass::new(facing, 0);
    id
}

fn jams(sim: &Simulation, cell: (u16, u16)) -> bool {
    sim.astar_code2_chain_jams(cell, MovementLayer::Ground, None)
        .expect("the arena has map cells")
}

fn set_head(sim: &mut Simulation, id: u64, head: Option<u8>) {
    let queue = &mut sim
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .navigation
        .path_replay;
    queue.set_directions_for_test(head.into_iter().collect());
    queue.set_cursor(0);
}

/// A cell whose first object is no Foot jams (`0x0042989A`); an empty one
/// clears (`0x00429888`).
#[test]
fn a_building_jams_and_an_empty_cell_clears() {
    let rules = rules();
    let mut sim = arena(&rules);
    let hut = spawn(&mut sim, &rules, "HUT", (11, 10));
    assert_eq!(
        sim.cell_objects((11, 10), MovementLayer::Ground).next(),
        Some(CellObjectMember::Entity(hut))
    );

    assert!(jams(&sim, (11, 10)));
    assert!(!jams(&sim, (12, 10)));
}

/// A stopped Foot heads for its path's head word (`0x004298B3`), whatever
/// its facing, and one without a head clears the chain.
#[test]
fn a_stopped_foot_heads_for_its_path_head() {
    let rules = rules();
    let mut sim = arena(&rules);
    spawn(&mut sim, &rules, "HUT", (11, 10));
    let tank = stopped(&mut sim, &rules, (10, 10), Some(EAST));
    sim.substrate.entities.get_mut(tank).unwrap().body_facing = FacingClass::new(0x4000, 0);

    assert!(jams(&sim, (10, 10)), "the head steps east onto the hut");
    set_head(&mut sim, tank, Some(NORTH));
    assert!(!jams(&sim, (10, 10)), "the head steps north onto nothing");
    set_head(&mut sim, tank, None);
    assert!(!jams(&sim, (10, 10)), "no head clears, facing east or not");
}

/// The head word is masked to a direction (`0x004298F1`): a tube word (8)
/// steps north.
#[test]
fn a_tube_head_word_steps_north() {
    let rules = rules();
    let mut sim = arena(&rules);
    spawn(&mut sim, &rules, "HUT", (10, 9));
    stopped(&mut sim, &rules, (10, 10), Some(8));

    assert!(jams(&sim, (10, 10)));
}

/// A moving Foot heads along its PrimaryFacing rounded to eight ways
/// (`0x004298C4..0x004298DC`), not its head word: 0x3000 rounds east,
/// 0x2FFF north-east.
#[test]
fn a_moving_foot_heads_along_its_facing() {
    let rules = rules();
    let mut sim = arena(&rules);
    spawn(&mut sim, &rules, "HUT", (11, 10));
    let tank = moving(&mut sim, &rules, (10, 10), 0x4000);

    assert!(jams(&sim, (10, 10)), "facing east, onto the hut");
    for (facing, onto_hut) in [(0x3000, true), (0x2FFF, false), (0, false)] {
        sim.substrate.entities.get_mut(tank).unwrap().body_facing = FacingClass::new(facing, 0);
        assert_eq!(jams(&sim, (10, 10)), onto_hut, "facing {facing:#06x}");
    }
    assert_eq!(dir_from_facing16(0x2FFF), NORTH_EAST);
}

/// Ten Foots in a row jam whatever the last one heads for; nine clear when
/// the cell after them lists nothing (`0x00429978..0x00429984`).
#[test]
fn the_chain_jams_after_ten_foots() {
    let rules = rules();
    let mut sim = arena(&rules);
    for x in 5..=13 {
        stopped(&mut sim, &rules, (x, 10), Some(EAST));
    }
    assert!(!jams(&sim, (5, 10)), "nine Foots, then an empty cell");

    stopped(&mut sim, &rules, (14, 10), Some(EAST));
    assert!(jams(&sim, (5, 10)), "ten Foots");
}

/// The next list is the bridge list of a cell with a bridge
/// (`+0x140 & 0x100`) when the Foot is on a bridge or its own cell stands
/// more than two levels higher (`0x00429932..0x0042996E`); otherwise the
/// ground list, where the hut stands.
#[test]
fn a_bridge_cell_lists_a_bridge_occupant_on_its_deck() {
    let rules = rules();
    let mut sim = arena(&rules);
    spawn(&mut sim, &rules, "HUT", (11, 10));
    let tank = stopped(&mut sim, &rules, (10, 10), Some(EAST));
    let set_on_bridge = |sim: &mut Simulation, on: bool| {
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .set_on_bridge(on);
    };
    let set_level = |sim: &mut Simulation, cell: (u16, u16), level: u8| {
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.cell_mut(cell.0, cell.1).unwrap().level = level;
    };

    set_on_bridge(&mut sim, true);
    assert!(jams(&sim, (10, 10)), "no bridge: the ground list");

    let terrain = sim.resolved_terrain.as_mut().unwrap();
    terrain.cell_mut(11, 10).unwrap().bridge_facts.raw_flags |= 0x100;
    assert!(!jams(&sim, (10, 10)), "on a bridge: the empty deck list");

    set_on_bridge(&mut sim, false);
    assert!(jams(&sim, (10, 10)), "level with the cell: the ground list");
    set_level(&mut sim, (10, 10), 3);
    assert!(!jams(&sim, (10, 10)), "three levels up: the deck list");
    set_level(&mut sim, (10, 10), 2);
    assert!(jams(&sim, (10, 10)), "two levels up: the ground list");
}

/// The search asks the chain about the Cell it enters (`0x00429F8A` passes
/// the neighbour). A friendly tank stopped between two huts with a NavCom
/// makes the searching tank's class answer 2 for its Cell. When it heads east
/// onto nothing, the chain clears (x1) and the two-step route runs through it.
/// When it heads north onto a hut, the chain jams (x4) and the four-step
/// detour round the huts is cheaper. Finishing admits only class 0, so it
/// straightens neither.
#[test]
fn a_jammed_chain_detours_the_route_and_a_clear_one_runs_through() {
    let rules = rules();
    for (head, through) in [(EAST, true), (NORTH, false)] {
        let mut sim = arena(&rules);
        spawn(&mut sim, &rules, "HUT", (10, 9));
        spawn(&mut sim, &rules, "HUT", (10, 11));
        let blocker = stopped(&mut sim, &rules, (10, 10), Some(head));
        sim.substrate
            .entities
            .get_mut(blocker)
            .unwrap()
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::cell(10, 5)));
        let tank = spawn(&mut sim, &rules, "TANK", (9, 10));
        let request = FootPathRequest::track(
            &sim.substrate.entities,
            tank,
            DriveCoord::cell(11, 10, 0),
            0,
            sim.playfield_bounds,
        )
        .unwrap();

        assert_eq!(
            sim.foot_find_path(&request, &rules, None, FrameEffects::default()),
            Ok(FindPathResult::Route)
        );
        let route = sim
            .substrate
            .entities
            .get(tank)
            .unwrap()
            .navigation
            .path_replay
            .route_cells();
        assert_eq!(route.contains(&(10, 10)), through, "head {head}: {route:?}");
        assert_eq!(route.last(), Some(&(11, 10)));
    }
}

/// The original executable's controls (`tools/spatial_oracle/
/// astar_hills_markers.json`): a same-House MTNK on its ground list with
/// the queue words `0, 0`, facing north (its `+0x388` bytes are zero), at
/// speed fraction 0 and 1.0, priced from the cell it stands in at urgency
/// 0, 1 and 2. The cell north of it lists nothing. This arena is VERA's, so
/// the check is a regression test of those prices; the Hills map row
/// (`hills_native_integration_tests`) is the comparison with native.
#[test]
fn the_native_hills_controls_price_as_the_live_chain() {
    let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_hills_markers.json",
    ))
    .unwrap();
    assert_eq!(packet["code_unchanged"], true);
    let native = &packet["blocker_prestate"];
    assert_eq!(native["on_bridge"], false);
    assert_eq!(native["supplied_queue"], serde_json::json!([0, 0, -1]));
    let rules = rules();
    let mut sim = arena(&rules);
    let tank = stopped(&mut sim, &rules, (10, 10), Some(NORTH));
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .navigation
        .path_replay
        .set_directions_for_test(vec![NORTH, NORTH]);
    let controls = packet["costs"].as_array().unwrap();
    assert_eq!(controls.len(), 6);
    for control in controls {
        assert_eq!(control["concrete_class"], 2);
        assert_eq!(control["rng_before"], control["rng_after"]);
        let speed = match control["supplied_speed_f64_bits"].as_str().unwrap() {
            "0000000000000000" => SIM_ZERO,
            "3ff0000000000000" => SIM_ONE,
            value => panic!("unrepresented native speed control {value}"),
        };
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .foot_speed
            .set_speed_fraction(speed);
        let urgency = control["urgency"].as_u64().unwrap() as u8;
        let multiplier = code2_multiplier(urgency, || {
            sim.astar_code2_chain_jams((10, 10), MovementLayer::Ground, None)
        })
        .unwrap();
        assert_eq!(
            f64::from(multiplier),
            control["cost"].as_f64().unwrap(),
            "native Foot+578={} urgency={urgency}",
            control["supplied_speed_f64_bits"]
        );
    }
}
