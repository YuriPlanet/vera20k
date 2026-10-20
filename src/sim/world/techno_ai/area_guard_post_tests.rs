//! Area Guard's building-post arm (`0x004D6B8D..0x004D6D1A`) through the
//! Foot dispatcher, on retail rules: a computer guard whose post is a cell
//! under an own or allied building moves the post beside it.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{ObjectAiCtx, Simulation};

/// GAPOWR's 2x2 foundation, from its north corner.
const POWER_PLANT: (u16, u16) = (14, 10);
const INSIDE: (u16, u16) = (15, 11);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Own,
    Human,
    Enemy,
    Allied,
    Harvester,
    BuildingArchive,
    NoBuilding,
}

/// Retail rules on flat ground with a computer (or human) Americans House
/// and a computer Russians House, Americans allying Russians when `allied`.
fn retail_world(human: bool, allied: bool) -> Option<(Simulation, RuleSet)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for (name, side, human) in [("Americans", 0, human), ("Russians", 1, false)] {
        let id = sim.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    if allied {
        sim.house_alliances_mut_for_test()
            .entry("AMERICANS".into())
            .or_default()
            .insert("RUSSIANS".into());
    }
    Some((sim, rules))
}

fn area_guard(sim: &mut Simulation, id: u64) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::AreaGuard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

/// One Area Guard visit of a guard standing at `guard_cell` whose post is
/// [`INSIDE`] (or the plant itself): the post after it, the NavCom, the
/// RNG streams, and the plant's foundation.
struct Visit {
    post: Option<TargetKind>,
    nav_com: Option<NavTargetRef>,
    rng: String,
}

fn visit(case: Case, guard_cell: (u16, u16)) -> Option<Visit> {
    let (mut sim, rules) = retail_world(case == Case::Human, case == Case::Allied)?;
    let plant_owner = match case {
        Case::Enemy | Case::Allied => "Russians",
        _ => "Americans",
    };
    let plant = (case != Case::NoBuilding).then(|| {
        sim.spawn_object(
            "GAPOWR",
            plant_owner,
            POWER_PLANT.0,
            POWER_PLANT.1,
            0,
            &rules,
        )
        .unwrap()
    });
    assert_eq!(
        sim.substrate
            .occupancy
            .first_building_on_layer(INSIDE.0, INSIDE.1, MovementLayer::Ground),
        plant,
        "{case:?}: the post cell lies in the plant's foundation"
    );
    let guard_type = if case == Case::Harvester {
        "HARV"
    } else {
        "MTNK"
    };
    let id = sim
        .spawn_object(
            guard_type,
            "Americans",
            guard_cell.0,
            guard_cell.1,
            0,
            &rules,
        )
        .unwrap();
    area_guard(&mut sim, id);
    let post = match case {
        Case::BuildingArchive => TargetKind::Entity(plant.unwrap()),
        _ => TargetKind::Cell(INSIDE.0, INSIDE.1),
    };
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_archive_target(Some(post));
    crate::sim::world::dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());
    let entity = sim.substrate.entities.get(id).unwrap();
    Some(Visit {
        post: entity.archive_target(),
        nav_com: entity.navigation.nav_com(),
        rng: format!(
            "{:?} {:?}",
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state()
        ),
    })
}

/// The cell a moved post went to: outside the foundation, in the first
/// ring around [`INSIDE`].
fn assert_beside_the_plant(post: Option<TargetKind>, context: &str) -> (u16, u16) {
    let Some(TargetKind::Cell(x, y)) = post else {
        panic!("{context}: post {post:?} is not a cell");
    };
    let inside_foundation = (POWER_PLANT.0..POWER_PLANT.0 + 2).contains(&x)
        && (POWER_PLANT.1..POWER_PLANT.1 + 2).contains(&y);
    assert!(
        !inside_foundation,
        "{context}: post ({x}, {y}) is in the plant"
    );
    let ring = i32::from(x)
        .abs_diff(i32::from(INSIDE.0))
        .max(i32::from(y).abs_diff(i32::from(INSIDE.1)));
    assert_eq!(
        ring, 1,
        "{context}: post ({x}, {y}) is in the nearest free ring"
    );
    (x, y)
}

/// A computer tank guarding a cell under its own or an allied power plant
/// has the post moved beside the plant, drawing nothing more than a guard
/// whose post is free. A human's, an enemy plant's, a harvester's, a
/// building archive and a free cell keep the post.
#[test]
fn a_computer_guard_posted_under_a_friendly_building_moves_its_post_beside_it() {
    let Some(free) = visit(Case::NoBuilding, (10, 10)) else {
        return;
    };
    assert_eq!(free.post, Some(TargetKind::Cell(INSIDE.0, INSIDE.1)));
    for case in [
        Case::Own,
        Case::Human,
        Case::Enemy,
        Case::Allied,
        Case::Harvester,
        Case::BuildingArchive,
    ] {
        let result = visit(case, (10, 10)).unwrap();
        let context = format!("{case:?}");
        match case {
            Case::Own | Case::Allied => {
                assert_beside_the_plant(result.post, &context);
                assert_eq!(result.rng, free.rng, "{context}: the arm draws nothing");
            }
            Case::BuildingArchive => {
                assert!(
                    matches!(result.post, Some(TargetKind::Entity(_))),
                    "{context}: {:?}",
                    result.post
                );
            }
            _ => assert_eq!(
                result.post,
                Some(TargetKind::Cell(INSIDE.0, INSIDE.1)),
                "{context}"
            ),
        }
    }
}

/// The same visit's leash return heads for the moved post, not the cell in
/// the plant.
#[test]
fn a_leashed_guard_returns_to_the_moved_post() {
    let Some(result) = visit(Case::Own, (28, 28)) else {
        return;
    };
    let (x, y) = assert_beside_the_plant(result.post, "leashed");
    assert_eq!(result.nav_com, Some(NavTargetRef::Cell { rx: x, ry: y }));
}
