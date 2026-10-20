//! A stock computer House's automatic yard deploy sends its starting force
//! to the base (`HouseClass::MoveGroundForcesToBase @ 0x0050C920`, called
//! from `UnitClass::Deploy` at `0x00739926`): one shared cell destination, a
//! cleared archive target and a queued Move for every ground actor.

use std::collections::{BTreeMap, BTreeSet};

use crate::headless_scenario::{battle_session, load_session};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

#[test]
#[ignore = "slow; real stock startup, automatic AI MCV deployment and ground-force travel"]
fn actual_computer_yard_retasks_existing_starting_force_through_move() {
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut launch = battle_session(
        "XMP03T4.MAP",
        &[(LaunchCountry::Russia, AiDifficulty::Easy)],
    );
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(1);
    launch.opponents[0].start_position = LaunchStartPosition::Position(2);
    launch.options.starting_credits = 5000;
    launch.options.unit_count = 10;
    launch.options.bases = true;
    launch.options.tech_level = 10;
    launch.options.short_game = true;
    launch.options.crates = false;
    launch.options.shroud = false;
    let mut scene = load_session(&root, "XMP03T4.MAP", 0x1234_5678, launch)
        .expect("unchanged stock Battle startup");
    // The client's cadence; headless_scenario's convenience cadence is 66 ms.
    const CLIENT_TICK_MS: u32 = 1000 / crate::util::fixed_math::SIM_TICK_HZ;
    let sim = scene.sim();
    let owner = sim.interner.get("Computer1").unwrap();
    let rules = &scene.runtime.resources.rules;
    let tank_type = rules.object("HTNK").unwrap();
    assert!(tank_type.allowed_to_start_in_multiplayer);
    assert!(tank_type.tech_level <= sim.houses[&owner].tech_level());
    assert!(tank_type.deploys_into.is_none());
    // Armed Unit and Infantry idle (`0x00738B67..0x00738C98`,
    // `0x0051CCB9..0x0051CD75`) select AreaGuard for this House's IQ.
    assert!(sim.houses[&owner].current_iq() >= rules.general.iq_guard_area);
    let yard_type = rules.object("SMCV").unwrap().deploys_into.clone().unwrap();
    assert!(rules.object(&yard_type).unwrap().construction_yard);
    let mcv = sim
        .entities()
        .iter_sorted()
        .find(|(_, entity)| {
            entity.owner() == owner && sim.interner.resolve(entity.type_ref()) == "SMCV"
        })
        .expect("normal launch must supply the computer MCV")
        .0;
    let army: Vec<_> = sim
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| {
            entity.owner() == owner
                && matches!(sim.interner.resolve(entity.type_ref()), "HTNK" | "E2")
        })
        .map(|(id, entity)| (id, (entity.position.rx(), entity.position.ry())))
        .collect();
    assert!(army.iter().any(|(id, _)| {
        sim.interner
            .resolve(sim.entities().get(*id).unwrap().type_ref())
            == "HTNK"
    }));
    let mut converted = false;
    for _ in 0..60 {
        let sim = scene.sim();
        let prior_archives = army
            .iter()
            .all(|(id, _)| sim.entities().get(*id).unwrap().archive_target().is_some());
        scene
            .runtime
            .advance_frame(
                &[],
                CLIENT_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let sim = scene.sim();
        let yard = sim.entities().iter_sorted().any(|(_, entity)| {
            entity.owner() == owner && sim.interner.resolve(entity.type_ref()) == yard_type
        });
        if yard {
            assert!(
                !sim.entities().contains(mcv),
                "normal conversion retires its source"
            );
            assert!(
                prior_archives,
                "AreaGuard gives every actor an archive target before the deploy"
            );
            converted = true;
            break;
        }
    }
    assert!(converted, "the computer deploys its MCV without a command");
    let sim = scene.sim();
    assert_ne!(sim.houses[&owner].base_origin(), (0, 0));
    let move_mission = MissionId::from_known(MissionType::Move);
    let shared = sim.entities().get(army[0].0).unwrap().navigation.nav_com();
    assert!(shared.is_some(), "the deploy gives a destination");
    for (id, _) in &army {
        let entity = sim.entities().get(*id).unwrap();
        assert_eq!(
            entity.archive_target(),
            None,
            "the deploy clears {id}'s archive target"
        );
        assert_eq!(
            entity.navigation.nav_com(),
            shared,
            "the deploy gives {id} the shared destination"
        );
        // An actor that updates after the MCV in the deploy frame has
        // already commenced the queued Move.
        assert!(
            entity.mission.queued() == move_mission || entity.mission.current() == move_mission,
            "the deploy queues Move for {id}"
        );
    }
    let mut moved = BTreeSet::new();
    let mut completed = BTreeMap::new();
    for _ in 0..600 {
        scene
            .runtime
            .advance_frame(
                &[],
                CLIENT_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let sim = scene.sim();
        for &(id, initial_cell) in &army {
            let entity = sim.entities().get(id).expect("starting force remains live");
            let cell = (entity.position.rx(), entity.position.ry());
            if cell != initial_cell {
                moved.insert(id);
            }
            if moved.contains(&id)
                && entity.navigation.nav_com().is_none()
                && entity.mission.current() == MissionId::from_known(MissionType::AreaGuard)
            {
                completed
                    .entry(id)
                    .or_insert((sim.session.binary_frame(), cell));
            }
        }
        if completed.len() == army.len() {
            break;
        }
    }
    assert_eq!(
        completed.len(),
        army.len(),
        "every actor travels, releases its destination and returns to AreaGuard; moved={moved:?}, completed={completed:?}"
    );
}
