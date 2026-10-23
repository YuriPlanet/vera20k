//! Default app handoffs must retain the native actor-turn path-search order.

use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::track_process::{TrackFamily, TrackProgress};
use crate::sim::rng::MainRng;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, TickLane};

const SEED: u64 = 0x004B_28A3;
const MAP_HASH: u64 = 0x4B28_A3;

/// Stock rules and lifecycle admission on an explicit authored clear map.
/// Bases keep both skirmish houses alive; the full foundations and the
/// player's short Move lie inside the native Size=20x20 diamond. These are
/// initial scene objects, not a claim about paid production or map startup.
fn stock_scene(rules: RuleSet) -> (SimRuntime, u64) {
    let mut sim = Simulation::with_seed(SEED);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    for (name, human) in [("Americans", true), ("Russians", false)] {
        let owner = sim.intern(name);
        let side = rules.country_side_index(name).unwrap().0;
        let mut house = HouseState::new(owner, side, Some(owner), human, 0, 10);
        house.project_country_mults(&rules, sim.interner());
        sim.houses_mut_for_test().register(house);
    }
    let player = sim.interner().get("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(player);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    *sim.overlay_grid_mut_for_test() = Some(OverlayGrid::new(32, 32));
    sim.spawn_object("GAPOWR", "Americans", 14, 14, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 28, 26, 0, &rules)
        .unwrap();
    let mover = sim
        .spawn_object("MTNK", "Americans", 20, 20, 64, &rules)
        .unwrap();
    assert!(sim.techno_player_controllable(mover, &rules));
    assert!(sim.can_select_object(mover, &rules));
    let mut resources = SimResources::new(rules);
    resources.terrain_template = sim.resolved_terrain().cloned();
    (
        SimRuntime {
            simulation: sim,
            resources,
        },
        mover,
    )
}

#[derive(Debug, PartialEq)]
struct MoverSample {
    frame: u32,
    current: MissionId,
    queued: MissionId,
    nav: Option<NavTargetRef>,
    location: DriveCoord,
    destination: Option<DriveCoord>,
    head: Option<DriveCoord>,
    progress: Option<TrackProgress>,
    route: Vec<u8>,
}

fn sample(runtime: &SimRuntime, id: u64) -> MoverSample {
    let sim = &runtime.simulation;
    let actor = sim.entities().get(id).unwrap();
    let loco = actor.locomotor.as_ref().unwrap();
    MoverSample {
        frame: sim.session().binary_frame(),
        current: actor.mission.current(),
        queued: actor.mission.queued(),
        nav: actor.navigation.nav_com(),
        location: position_world_coord(&actor.position),
        destination: loco.track_destination(TrackFamily::Drive),
        head: loco.track_head(TrackFamily::Drive),
        progress: loco.track_progress(TrackFamily::Drive),
        route: actor
            .navigation
            .path_replay()
            .remaining_directions()
            .to_vec(),
    }
}

fn frame(runtime: &mut SimRuntime, command: Option<Command>) {
    let mut commands = runtime.simulation.take_due_commands();
    if let Some(command) = command {
        commands.push(CommandEnvelope::new(
            runtime.simulation.interner().get("Americans").unwrap(),
            runtime.simulation.session().tick() + 1,
            command,
        ));
    }
    let output = runtime
        .advance_frame(&commands, 33, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(output.tick.frame_committed);
}

/// Compare actor behavior, rather than inspecting an implementation switch.
/// The serial reference supplies the native same-turn search contract:
/// Drive4B281C's due timer -> Find_Path4B28A3 -> fresh head in this call.
fn assert_native_move(
    mut reference: SimRuntime,
    mut incoming: SimRuntime,
    mover: u64,
    target: (u16, u16),
) {
    assert_eq!(sample(&incoming, mover), sample(&reference, mover));
    for runtime in [&mut reference, &mut incoming] {
        frame(
            runtime,
            Some(Command::Move {
                entity_id: mover,
                target_rx: target.0,
                target_ry: target.1,
            }),
        );
    }
    let mut saw_first_route = false;
    for _ in 0..60 {
        let before = sample(&reference, mover);
        assert_eq!(sample(&incoming, mover), before);
        frame(&mut reference, None);
        frame(&mut incoming, None);
        let expected = sample(&reference, mover);
        let actual = sample(&incoming, mover);
        if !saw_first_route && !expected.route.is_empty() {
            assert!(before.route.is_empty() && before.head.is_none());
            assert_eq!(before.nav, Some(NavTargetRef::cell(target.0, target.1)));
            assert_eq!(expected.current.known(), Some(MissionType::Move));
            assert!(expected.head.is_some());
            assert_ne!(expected.location, before.location);
            assert_eq!(
                actual, expected,
                "first due search must move in this actor turn"
            );
            saw_first_route = true;
        }
        assert_eq!(actual, expected, "subsequent ordinary Move frames");
    }
    assert!(
        saw_first_route,
        "the stock Move reached its due path search"
    );
}

#[test]
fn fresh_stock_move_uses_native_path_order() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut reference, mover) = stock_scene(retail.rules.clone());
    reference
        .simulation
        .install_fresh_process_main(&MainRng::new(SEED));
    reference.simulation.publish_scenario_started();
    let (scene, other_mover) = stock_scene(retail.rules);
    assert_eq!(mover, other_mover);
    let incoming = crate::app::loading::transitions::adopt_fresh_simulation(
        scene.simulation,
        scene.resources,
        &MainRng::new(SEED),
    );
    assert_native_move(reference, incoming, mover, (26, 20));
}

fn save(runtime: &SimRuntime) -> Vec<u8> {
    GameSnapshot::save_validated(
        &runtime.simulation,
        MAP_HASH,
        runtime.resources.rules.simulation_config_hash(),
        "stock native path order",
        1,
    )
}

fn prepare(runtime: &SimRuntime, bytes: &[u8]) -> PreparedLoad {
    PreparedLoad::prepare_candidate(
        bytes,
        Some(&runtime.simulation),
        Some(MAP_HASH),
        Some(&runtime.resources.rules),
        runtime.resources.terrain_template.as_ref(),
        Some(&runtime.resources.overlay_registry),
    )
    .unwrap()
}

#[test]
fn saved_stock_move_uses_native_path_order_without_mutating_a_rejected_load() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut reference, mover) = stock_scene(retail.rules.clone());
    let (mut incoming, other_mover) = stock_scene(retail.rules);
    assert_eq!(mover, other_mover);
    // A legacy app save carries Some(PathBatch). Enable it before saving,
    // never after adoption, so this exercises the real saved-world policy.
    incoming.simulation.batch_path_searches();
    let bytes = save(&incoming);
    let before = incoming.simulation.state_hash();
    let rng = incoming.simulation.rng_state();
    assert!(matches!(
        PreparedLoad::prepare_candidate(
            &bytes,
            Some(&incoming.simulation),
            Some(MAP_HASH),
            Some(&incoming.resources.rules),
            incoming.resources.terrain_template.as_ref(),
            None,
        ),
        Err(PreparedLoadError::MissingOverlayRegistry)
    ));
    assert_eq!(incoming.simulation.state_hash(), before);
    assert_eq!(incoming.simulation.rng_state(), rng);
    let prepared = prepare(&incoming, &bytes);
    assert_eq!(incoming.simulation.state_hash(), before);
    prepared.commit_into(&mut incoming);
    // Both worlds pass the real load preparation and successful handoff:
    // Scenario RNG, map restoration and the started latch match too.
    prepare(&reference, &save(&reference)).commit_into(&mut reference);
    assert_native_move(reference, incoming, mover, (26, 20));
}

#[test]
fn saved_pending_search_does_not_defer_a_later_stock_order() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut reference, mover) = stock_scene(retail.rules.clone());
    let (mut incoming, other_mover) = stock_scene(retail.rules);
    assert_eq!(mover, other_mover);
    let initial = sample(&incoming, mover);
    for runtime in [&mut reference, &mut incoming] {
        runtime.simulation.batch_path_searches();
        frame(
            runtime,
            Some(Command::Move {
                entity_id: mover,
                target_rx: 26,
                target_ry: 20,
            }),
        );
        frame(runtime, None);
    }
    let pending = sample(&incoming, mover);
    assert_eq!(pending.frame, 2);
    assert_eq!(pending.current.known(), Some(MissionType::Move));
    assert_eq!(pending.nav, Some(NavTargetRef::cell(26, 20)));
    assert!(pending.destination.is_some());
    assert!(pending.head.is_none() && pending.route.is_empty());
    assert_eq!(pending.location, initial.location);
    assert_eq!(sample(&reference, mover), pending);
    let legacy_bytes = save(&incoming);

    // Both independent worlds replayed the same actual pre-save history,
    // including process Main draws (which Scenario snapshots do not save).
    // Select serial execution before the reference's own save/load, not by
    // changing the app's adopted world or any actor field.
    reference.simulation.use_native_path_search_order();
    prepare(&reference, &save(&reference)).commit_into(&mut reference);
    prepare(&incoming, &legacy_bytes).commit_into(&mut incoming);
    assert_eq!(sample(&reference, mover), pending);
    assert_eq!(sample(&incoming, mover), pending);

    // Old batching can answer this saved pending request on resume. Reach
    // the destination through ordinary frames, then exercise a new request.
    for _ in 0..100 {
        frame(&mut reference, None);
        frame(&mut incoming, None);
        assert_eq!(sample(&incoming, mover), sample(&reference, mover));
    }
    let stopped = sample(&reference, mover);
    assert!(stopped.head.is_none() && stopped.nav.is_none());
    assert_eq!(
        (stopped.location.x, stopped.location.y),
        (26 * 256 + 128, 20 * 256 + 128)
    );
    assert_native_move(reference, incoming, mover, (28, 20));
}
