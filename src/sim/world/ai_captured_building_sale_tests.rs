//! Stock Engineer capture followed by the computer's Building4492D7 sale.
//!
//! Physical rules, ART, map and launch houses feed ordinary command/frame owners.
//! The finished plant and Engineer are constructed at supplied clear sites; the
//! computer's unrelated base-building/strategy work is held inactive. The AI
//! Capture order supplies both Target and Destination, as the Engineer Hunt arm
//! (4D53BA..4D53F4/4D548F..4D54B9) does. Autonomous Team/Hunt generation and a
//! whole original-engine match are outside this Rust composition regression.

use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseAiActivationLatches;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::timer::CdTimer;
use crate::sim::world::SnapshotProcessState;
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

const MAP: &str = "XMP03T4.MAP";
const PLANT_CELL: (u16, u16) = (31, 89);
const ENGINEER_CELL: (u16, u16) = (27, 89);
const FRAME_LIMIT: usize = 1200;

fn stock_scene() -> (HeadlessScenario, InternedId, InternedId) {
    let (root, _) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");
    let mut launch = battle_session(MAP, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.unit_count = 0;
    launch.options.bases = true;
    launch.options.crates = false;
    launch.options.short_game = false;
    launch.options.multi_engineer = false;
    let mut scene = load_session(&root, MAP, 0x4492_D7, launch).expect("retail Battle loads");
    let sim = &mut scene.runtime.simulation;
    sim.hash_state_every(std::num::NonZeroU64::new(10_000).unwrap());
    let human = sim.session.current_house().unwrap();
    assert!(sim.houses[&human].is_human());
    let computers: Vec<_> = sim
        .houses
        .iter()
        .filter_map(|(&id, house)| {
            (!house.is_human() && !house.player_control() && !house.multiplay_passive())
                .then_some(id)
        })
        .collect();
    assert_eq!(computers.len(), 1, "one genuine launch computer House");
    let computer = computers[0];
    let country = sim.houses[&computer].country().unwrap();
    assert_eq!(sim.interner.resolve(country), "Russians");
    assert!(!sim.houses[&computer].is_controlled_by_human(true));
    // This explicit AI prior prevents an unrelated base rollout or no-factory
    // fire sale. It changes none of the human/player/passive capture-sale gates.
    // Both actual starting MCVs remain alive, so neither House is eliminated.
    assert!(scene.runtime.resources.rules.general.iq_production > 0);
    let frame = sim.session.binary_frame() as i32;
    let house = sim.houses.get_mut(&computer).unwrap();
    house.set_current_iq_for_test(0);
    house.set_ai_activation_for_test(HouseAiActivationLatches::default());
    house.set_strategy_timer_for_test(CdTimer::started(frame, 20_000));
    for owner in [human, computer] {
        let starters: Vec<_> = sim
            .entities()
            .iter_sorted()
            .filter(|(_, entity)| entity.owner() == owner)
            .collect();
        assert_eq!(starters.len(), 1, "only the lobby's MCV is supplied");
        assert!(matches!(
            sim.interner.resolve(starters[0].1.type_ref()),
            "AMCV" | "SMCV"
        ));
    }
    let rules = &scene.runtime.resources.rules;
    let plant = rules.object("GAPOWR").unwrap();
    assert_eq!((plant.strength, plant.cost, plant.soylent), (750, 800, 0));
    assert!(plant.capturable && rules.has_buildup("GAPOWR"));
    assert!(rules.object("SENGINEER").unwrap().engineer);
    (scene, human, computer)
}

fn place_plant(scene: &mut HeadlessScenario, owner: InternedId) -> u64 {
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    assert!(crate::sim::build_site::can_place_building_at(
        &runtime.simulation,
        rules,
        Some(&runtime.resources.overlay_registry),
        rules.object("GAPOWR").unwrap(),
        (PLANT_CELL.0 as i16, PLANT_CELL.1 as i16),
        Some(owner),
        None,
        None,
    ));
    let name = runtime.simulation.interner.resolve(owner).to_owned();
    let plant = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "GAPOWR",
            &name,
            PLANT_CELL.0,
            PLANT_CELL.1,
            0,
            rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary constructor/Unlimbo at a clear stock map site");
    let entity = runtime.simulation.entities().get(plant).unwrap();
    assert_eq!(entity.initial_owner(), owner);
    assert_eq!(entity.health.current(), 750);
    plant
}

fn advance(scene: &mut HeadlessScenario) {
    let due = scene.runtime.simulation.take_due_commands();
    scene.runtime.advance_frame_for_tooling(&due, 22).unwrap();
}

fn capture_by_walking(scene: &mut HeadlessScenario, owner: InternedId, plant: u64) {
    let runtime = &mut scene.runtime;
    let human = runtime.simulation.houses[&owner].is_human();
    let name = runtime.simulation.interner.resolve(owner).to_owned();
    let engineer = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            if human { "ENGINEER" } else { "SENGINEER" },
            &name,
            ENGINEER_CELL.0,
            ENGINEER_CELL.1,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("stock Engineer constructor and Walk Unlimbo");
    let command = if human {
        Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: plant,
        }
    } else {
        // The human click wrapper supplies Target=NULL. AI Mission_Capture
        // interprets that as no target and queues Hunt (4D4C14..4D4C6B), so use
        // the actual two-token AI order boundary, without writing mission/NavCom.
        Command::MegaMission {
            entity_id: engineer,
            mission: MissionType::Capture,
            target: Some(TargetKind::Entity(plant)),
            destination: Some(TargetKind::Entity(plant)),
        }
    };
    runtime.simulation.queue_command(CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick(),
        command,
    ));
    let mut walked = false;
    for _ in 0..FRAME_LIMIT {
        advance(scene);
        let sim = scene.sim();
        if let Some(entity) = sim.entities().get(engineer) {
            walked |= (entity.position.rx(), entity.position.ry()) != ENGINEER_CELL;
        }
        if sim
            .entities()
            .get(plant)
            .is_some_and(|entity| entity.owner() == owner)
        {
            assert!(walked, "capture must follow observed Walk displacement");
            assert!(
                !sim.entities().contains(engineer),
                "spent Engineer retires in the frame"
            );
            return;
        }
    }
    let engineer_state = scene.sim().entities().get(engineer).map(|entity| {
        (
            (entity.position.rx(), entity.position.ry()),
            entity.mission.current(),
            entity.mission.queued(),
            entity.navigation.nav_com(),
            entity.attack_target.target(),
        )
    });
    panic!("stock Engineer did not capture after {FRAME_LIMIT} frames: {engineer_state:?}");
}

#[test]
#[ignore = "physical retail Battle, stock Engineer Walk capture and complete building sale"]
fn stock_computer_capture_walks_then_sells_and_refunds_the_new_owner() {
    let (mut scene, human, computer) = stock_scene();
    let plant = place_plant(&mut scene, human);
    let credits_before =
        [human, computer].map(|owner| scene.sim().houses[&owner].economy().credits());
    capture_by_walking(&mut scene, computer, plant);
    let captured = scene.sim().entities().get(plant).unwrap();
    assert_eq!(captured.initial_owner(), human);
    assert_eq!(
        captured.mission.effective().known(),
        Some(MissionType::Selling)
    );
    let mut visited_selling = captured.mission.current().known() == Some(MissionType::Selling);
    for _ in 0..FRAME_LIMIT {
        advance(&mut scene);
        let Some(entity) = scene.sim().entities().get(plant) else {
            break;
        };
        visited_selling |= entity.mission.current().known() == Some(MissionType::Selling);
    }
    assert!(
        visited_selling,
        "the ordinary Building Selling handler must run"
    );
    assert!(
        !scene.sim().entities().contains(plant),
        "build-down must finish and remove the plant"
    );
    assert_eq!(
        scene.sim().houses[&human].economy().credits(),
        credits_before[0]
    );
    // Computer GetRefund does not apply the human RefundPercent. GAPOWR has
    // Cost=800, Soylent=0, and this Russia House has no Industrial Plant.
    assert_eq!(
        scene.sim().houses[&computer].economy().credits(),
        credits_before[1] + 800
    );
    for _ in 0..30 {
        advance(&mut scene);
    }
    assert_eq!(
        scene.sim().houses[&computer].economy().credits(),
        credits_before[1] + 800,
        "sale settlement is not repeated after retirement"
    );
}

#[test]
#[ignore = "physical retail capture history, validated save/load and computer recapture"]
fn stock_computer_recapture_after_load_keeps_its_original_building() {
    let (mut scene, human, computer) = stock_scene();
    let restore_terrain = scene.sim().resolved_terrain.as_ref().unwrap().clone();
    let plant = place_plant(&mut scene, computer);
    let credits_before =
        [human, computer].map(|owner| scene.sim().houses[&owner].economy().credits());
    capture_by_walking(&mut scene, human, plant);
    let captured = scene.sim().entities().get(plant).unwrap();
    assert_eq!(
        (captured.owner(), captured.initial_owner()),
        (human, computer)
    );
    assert_ne!(
        captured.mission.effective().known(),
        Some(MissionType::Selling)
    );

    // Retain the real live-process/map authorities and validate normal restore.
    // This checks retained ownership history, not native SAV bytes or equality
    // with uninterrupted Scenario RNG (native load deliberately reseeds it).
    let map_hash = scene.map.ini.content_hash();
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let bytes = GameSnapshot::save_validated(
        &runtime.simulation,
        map_hash,
        rules.simulation_config_hash(),
        "captured plant",
        0,
    );
    let mut restored = GameSnapshot::load_validated(
        &bytes,
        map_hash,
        rules.simulation_config_hash(),
        runtime.simulation.session.map_name(),
    )
    .unwrap()
    .sim;
    restored.retain_process_state_for_load(SnapshotProcessState::Live(&runtime.simulation));
    restored.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    restored.reconstruct_cellclass_dummy_for_map_resize();
    restored.restore_after_snapshot_load().unwrap();
    crate::sim::production::validate_restored_factory_state(&restored, rules).unwrap();
    restored.rebuild_caches_after_load(
        restore_terrain,
        runtime.simulation.terrain_speed_config().clone(),
        rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(rules, &runtime.resources.overlay_registry, None)
        .unwrap();
    restored.intern_rule_type_ids(rules);
    restored.resolve_type_handles(rules);
    restored.rebuild_lighting_sources_after_load(rules);
    // PreparedLoad commits the candidate's dummy fields onto the retained
    // identity, then resumes Main and publishes the scenario-started latch.
    let live_dummy = runtime.simulation.effective_shared_cell_dummy();
    live_dummy.adopt_prepared_load_state(&restored.effective_shared_cell_dummy());
    restored.bind_shared_cell_dummy(live_dummy);
    restored.install_fresh_process_main(runtime.simulation.main_rng());
    restored.publish_scenario_started();
    let plant_after_load = restored.entities().get(plant).unwrap();
    assert_eq!(
        (plant_after_load.owner(), plant_after_load.initial_owner()),
        (human, computer)
    );
    runtime.replace_simulation(restored);

    capture_by_walking(&mut scene, computer, plant);
    for _ in 0..120 {
        advance(&mut scene);
    }
    let recaptured = scene
        .sim()
        .entities()
        .get(plant)
        .expect("constructor House keeps its recapture");
    assert_eq!(
        (recaptured.owner(), recaptured.initial_owner()),
        (computer, computer)
    );
    assert_ne!(
        recaptured.mission.effective().known(),
        Some(MissionType::Selling)
    );
    assert_eq!(recaptured.health.current(), 750);
    assert_eq!(
        [human, computer].map(|owner| scene.sim().houses[&owner].economy().credits()),
        credits_before,
        "neither capture of the retained plant pays a sale refund"
    );
}
