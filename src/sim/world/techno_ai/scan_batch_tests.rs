//! Live target acquisition and compatibility with the retired scan request.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{ScanMission, TargetKind};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{FrameEffects, ObjectAiCtx};

/// A production-admitted tank whose next due mission visit is Hunt.
fn hunting_tank_scene(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 626);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().short_game = false;
    let hunter = sim
        .spawn_object("MTNK", "Americans", 12, 12, 0, rules)
        .unwrap();
    sim.session.set_binary_frame_for_test(100);
    sim.mission_queue_exact(
        hunter,
        MissionId::from_known(MissionType::Hunt),
        1,
        100,
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(hunter)
            .unwrap()
            .mission
            .current(),
        MissionId::from_known(MissionType::Hunt)
    );
    sim.refresh_object_sight(hunter, rules, 0, FrameEffects::default());
    (sim, hunter)
}

fn run_mission(sim: &mut Simulation, rules: &RuleSet, hunter: u64) -> Option<TargetKind> {
    super::super::mission_handlers::dispatch_foot_mission(
        sim,
        hunter,
        rules,
        ObjectAiCtx::default(),
    );
    let actor = sim.substrate.entities.get(hunter).unwrap();
    assert!(actor.mission.dispatch_timer().delay() > 0, "Hunt ran");
    actor.attack_target.target()
}

/// Exercise the former real-match opt-in, which now keeps scans live.
#[allow(deprecated)]
fn request_legacy_scan_mode(sim: &mut Simulation) {
    sim.batch_target_scans();
}

/// Foot::Mission_Hunt calls Retaliate_And_Scan synchronously at 0x004D5392.
/// An earlier factory/Unlimbo can therefore admit a hostile before this
/// hunter's turn, even when the pass began without a target.
#[test]
fn hunt_sees_a_hostile_admitted_after_the_pass_begins() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut serial, hunter) = hunting_tank_scene(rules);
    let (mut requested, requested_hunter) = hunting_tank_scene(rules);
    assert_eq!(hunter, requested_hunter);
    assert!(
        serial
            .threat_scan(rules, None, hunter, ScanMission::Hunt, None)
            .target()
            .is_none()
    );

    request_legacy_scan_mode(&mut requested);
    let hostile = serial
        .spawn_object("HTNK", "Russians", 14, 12, 0, rules)
        .unwrap();
    assert_eq!(
        requested
            .spawn_object("HTNK", "Russians", 14, 12, 0, rules)
            .unwrap(),
        hostile
    );
    let live = run_mission(&mut serial, rules, hunter);
    assert_eq!(live, Some(TargetKind::Entity(hostile)));
    assert_eq!(run_mission(&mut requested, rules, hunter), live);
    assert_eq!(
        requested.scenario_rng.logical_state(),
        serial.scenario_rng.logical_state()
    );
}

/// An old pick remaining legal does not preserve its rank after a more
/// valuable target enters. Both worlds use the real retail selector.
#[test]
fn hunt_reranks_new_candidates_even_when_the_old_pick_remains_legal() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut serial, hunter) = hunting_tank_scene(rules);
    let (mut requested, _) = hunting_tank_scene(rules);
    let previous = serial
        .spawn_object("E1", "Russians", 14, 12, 0, rules)
        .unwrap();
    assert_eq!(
        requested
            .spawn_object("E1", "Russians", 14, 12, 0, rules)
            .unwrap(),
        previous
    );
    assert_eq!(
        serial
            .threat_scan(rules, None, hunter, ScanMission::Hunt, None)
            .target(),
        Some(previous)
    );
    request_legacy_scan_mode(&mut requested);
    let hostile = serial
        .spawn_object("HTNK", "Russians", 12, 14, 0, rules)
        .unwrap();
    assert_eq!(
        requested
            .spawn_object("HTNK", "Russians", 12, 14, 0, rules)
            .unwrap(),
        hostile
    );
    let live = run_mission(&mut serial, rules, hunter);
    assert_eq!(live, Some(TargetKind::Entity(hostile)));
    assert_eq!(run_mission(&mut requested, rules, hunter), live);
    assert!(
        requested
            .substrate
            .entities
            .get(previous)
            .unwrap()
            .is_active()
    );
    assert_eq!(
        requested.scenario_rng.logical_state(),
        serial.scenario_rng.logical_state()
    );
}

/// The old payload contained no serialized fields, only the Option tag.
/// A restored request must not re-enable frame-start acquisition.
#[test]
fn a_saved_legacy_scan_request_resumes_with_live_acquisition() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut saved, hunter) = hunting_tank_scene(rules);
    saved.scan_batch = Some(ScanBatch::default());
    assert_eq!(
        crate::util::save_codec::serialize(&saved.scan_batch).unwrap(),
        vec![1]
    );
    let terrain = saved.resolved_terrain.as_ref().unwrap().clone();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&saved, 0, 0, "hunt.map", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert!(
        restored.scan_batch.is_some(),
        "the saved Option tag decoded"
    );
    restored.restore_after_snapshot_load().unwrap();
    assert!(
        restored.scan_batch.is_none(),
        "load retires the legacy request before any object turn"
    );
    restored.rebuild_caches_after_load(terrain, Default::default(), rules);
    assert!(restored.rebuild_dynamic_navigation(rules));
    let hostile = restored
        .spawn_object("HTNK", "Russians", 14, 12, 0, rules)
        .unwrap();
    assert_eq!(
        run_mission(&mut restored, rules, hunter),
        Some(TargetKind::Entity(hostile))
    );
}

/// Whole frames on a worker pool: per-call candidate scoring may run in
/// parallel, but each object consumes the world at its own visit.
fn frames_on_workers(
    rules: &RuleSet,
    size: u16,
    threads: usize,
    frames: usize,
    setup: impl Fn(&mut Simulation) + Sync,
) -> Vec<u64> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| {
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 626, size);
        sim.session.set_game_mode(ScenarioGameMode::Skirmish);
        sim.session.game_options_mut_for_test().short_game = false;
        let grid = sim
            .path_grid_snapshot()
            .map(|grid| (*grid).clone())
            .unwrap();
        setup(&mut sim);
        (0..frames)
            .map(|_| {
                let outcome = sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
                assert!(
                    outcome.frame_committed,
                    "the requested frame actually advances"
                );
                sim.state_hash()
            })
            .collect()
    })
}

#[test]
fn whole_frames_do_not_depend_on_the_thread_count() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let squads = |sim: &mut Simulation| {
        for i in 0..8u16 {
            for (house, x) in [("Americans", 10), ("Russians", 15)] {
                sim.spawn_object("E1", house, x, 8 + 2 * i, 0, rules)
                    .unwrap();
            }
        }
    };
    let one = frames_on_workers(rules, 32, 1, 90, squads);
    assert_eq!(frames_on_workers(rules, 32, 4, 90, squads), one);
}

/// Two hunting companies still exercise parallel scoring within one scan.
#[test]
fn nested_side_by_side_hunt_scans_do_not_depend_on_the_thread_count() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let companies = |sim: &mut Simulation| {
        for i in 0..15u16 {
            for j in 0..10u16 {
                for (house, x) in [("Americans", 4), ("Russians", 30)] {
                    let id = sim
                        .spawn_object("E1", house, x + j, 4 + 2 * i, 0, rules)
                        .unwrap();
                    sim.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .mission
                        .set_current_for_test(MissionId::from_known(MissionType::Hunt), 0);
                }
            }
        }
    };
    let one = frames_on_workers(rules, 48, 1, 10, companies);
    assert_eq!(frames_on_workers(rules, 48, 4, 10, companies), one);
}
