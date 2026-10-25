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

/// Aircraft41A940 continues into Foot4D6AA0 after Idle has commenced Enter.
/// Foot4D7025 reads that live mission's Rate: stock Enter14*2 + jitter1..5.
#[test]
fn an_empty_harrier_given_move_then_guard_uses_enter_cadence_after_idle() {
    use crate::sim::command::Command;
    use crate::sim::movement::air_movement::current_fly_height;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    let player = sim.intern("Americans");
    sim.session.set_current_house_for_test(Some(player));
    sim.spawn_object("GAPOWR", "Americans", 5, 10, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 26, 26, 0, &rules)
        .unwrap();
    let dock = sim
        .spawn_object("AMRADR", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let aircraft = sim
        .spawn_object("ORCA", "Americans", 11, 10, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .occupancy
            .first_building_on_layer(11, 10, MovementLayer::Ground),
        Some(dock)
    );
    // Supplied consumed-ammo prefix. Real Guard, Enter and docking reach
    // the service mission before the player interrupts it with Move/G.
    sim.substrate
        .entities
        .get_mut(aircraft)
        .unwrap()
        .ammo_mut()
        .unwrap()
        .set_current_for_test(0);
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        if sim
            .substrate
            .entities
            .get(aircraft)
            .unwrap()
            .mission
            .current()
            .known()
            == Some(MissionType::Sleep)
        {
            break;
        }
    }
    let entity = sim.substrate.entities.get(aircraft).unwrap();
    assert_eq!(entity.mission.current().known(), Some(MissionType::Sleep));
    assert_eq!(entity.ammo().unwrap().current(), 0);
    assert_eq!(entity.radio_contacts.slot(0), Some(dock));
    assert!(sim.mission_ready_to_commence(aircraft, &rules));
    assert!(sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: aircraft,
            target_rx: 25,
            target_ry: 10
        },
        Some(&rules)
    ));
    assert!(
        sim.substrate
            .entities
            .get(aircraft)
            .unwrap()
            .radio_contacts
            .is_empty()
    );
    assert!(
        !sim.substrate
            .entities
            .get(dock)
            .unwrap()
            .radio_contacts
            .contains(aircraft)
    );
    for _ in 0..8 {
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        let entity = sim.substrate.entities.get(aircraft).unwrap();
        if entity.mission.current().known() == Some(MissionType::Move)
            && current_fly_height(entity, sim.resolved_terrain.as_ref()) > 0
        {
            break;
        }
    }
    let entity = sim.substrate.entities.get(aircraft).unwrap();
    assert_eq!(entity.mission.current().known(), Some(MissionType::Move));
    assert!((1..1500).contains(&current_fly_height(entity, sim.resolved_terrain.as_ref())));
    let guard = sim.area_guard_key_command(aircraft, &rules).unwrap();
    assert!(sim.apply_command("Americans", &guard, Some(&rules)));
    let mut saw_area_guard = false;
    for _ in 0..20 {
        saw_area_guard |= sim
            .substrate
            .entities
            .get(aircraft)
            .unwrap()
            .mission
            .current()
            .known()
            == Some(MissionType::AreaGuard);
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        let entity = sim.substrate.entities.get(aircraft).unwrap();
        if saw_area_guard && entity.mission.current().known() == Some(MissionType::Enter) {
            assert_eq!(
                entity.navigation.nav_com(),
                Some(NavTargetRef::Object { id: dock })
            );
            let delay = entity.mission.dispatch_timer().delay();
            assert!(
                (29..=33).contains(&delay),
                "Enter cadence after AreaGuard Idle, got {delay}"
            );
            return;
        }
    }
    panic!("Move/G did not reach the AreaGuard-to-Enter handoff");
}
