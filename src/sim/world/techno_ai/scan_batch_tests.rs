//! Rust regression tests for the batched target scans ([`super`]). A batched
//! answer is checked against the object's own scan on the same world; no
//! native behavior is claimed.
use super::*;
use crate::sim::mission::MissionType;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{FrameEffects, ObjectAiCtx};

/// Put `id` on `mission`.
fn on_mission(sim: &mut Simulation, id: u64, mission: MissionType) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(mission), 0);
}

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
    sim.refresh_object_sight(hunter, rules, 0, FrameEffects::default());
    (sim, hunter)
}

/// Run `hunter`'s mission handler; answers its target.
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

/// The module's sim-scale deviation: Foot::Mission_Hunt scans at its turn
/// (`0x004D5392`), so natively a hunter sees a hostile an earlier turn of the
/// same pass admits. Its batched answer saw the pass begin, before the
/// hostile, so this run of its handler finds nothing; the answer for its next
/// run, a later pass's, sees the hostile.
#[test]
fn a_hunter_sees_a_hostile_admitted_during_the_pass_at_its_next_run() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut serial, hunter) = hunting_tank_scene(rules);
    let (mut batched, _) = hunting_tank_scene(rules);
    batched.batch_target_scans();
    batched.answer_batched_scans(rules, None);
    // The hostile sleeps, so only the hunter's scan targets it.
    let admit = |sim: &mut Simulation| {
        let id = sim
            .spawn_object("HTNK", "Russians", 14, 12, 0, rules)
            .unwrap();
        on_mission(sim, id, MissionType::Sleep);
        id
    };
    let hostile = admit(&mut serial);
    assert_eq!(admit(&mut batched), hostile);
    assert_eq!(
        run_mission(&mut serial, rules, hunter),
        Some(TargetKind::Entity(hostile))
    );
    assert_eq!(run_mission(&mut batched, rules, hunter), None);
    assert_eq!(batched.batched_scans_used(), 1, "the answer is taken");

    batched.finish_batched_scans();
    let now = batched.session.binary_frame();
    let next_run = now
        + batched
            .substrate
            .entities
            .get(hunter)
            .unwrap()
            .mission
            .dispatch_timer()
            .remaining_if_pending(now)
            .expect("Hunt runs again later") as u32;
    let grid = batched
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    // Whole frames through the handler's next run; a frame commits its
    // number only after its pass.
    while batched.session.binary_frame() <= next_run {
        let outcome = batched.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        assert!(outcome.frame_committed);
    }
    let actor = batched.substrate.entities.get(hunter).unwrap();
    assert_eq!(
        actor.attack_target.target(),
        Some(TargetKind::Entity(hostile))
    );
    assert_eq!(batched.batched_scans_used(), 2, "the next run's answer");
}

/// A save keeps the setting, in the bytes the retired empty struct wrote, but
/// not a pass's answers; a game saved without batching resumes without it.
#[test]
fn a_save_keeps_the_setting_and_drops_the_answers() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut sim, _) = hunting_tank_scene(rules);
    sim.batch_target_scans();
    sim.answer_batched_scans(rules, None);
    assert!(!sim.scan_batch.as_ref().unwrap().answers.is_empty());
    assert_eq!(
        crate::util::save_codec::serialize(&sim.scan_batch).unwrap(),
        vec![1]
    );
    let load = |sim: &Simulation| {
        let bytes = crate::sim::snapshot::GameSnapshot::save(sim, 0, 0, "scan_batch", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.restore_after_snapshot_load().unwrap();
        restored
    };
    let restored = load(&sim);
    let batch = restored.scan_batch.as_ref().expect("still batching");
    assert!(batch.answers.is_empty());
    let serial = crate::sim::arena_fixture::rival_arena(rules, 626);
    assert!(
        load(&serial).scan_batch.is_none(),
        "a serial game stays serial"
    );
}

/// `frames` whole frames, with scans batched, of a `size` arena that `setup`
/// fills, on a worker pool of `threads`: each frame's state hash, and the
/// answers scans took.
fn batched_frames(
    rules: &RuleSet,
    size: u16,
    threads: usize,
    frames: usize,
    setup: impl Fn(&mut Simulation) + Sync,
) -> (Vec<u64>, usize) {
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
        sim.batch_target_scans();
        let hashes = (0..frames)
            .map(|_| {
                let outcome = sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
                assert!(
                    outcome.frame_committed,
                    "the requested frame actually advances"
                );
                sim.state_hash()
            })
            .collect();
        (hashes, sim.batched_scans_used())
    })
}

/// Two hunting companies of 150 riflemen: each Hunt scan walks more objects
/// than `greatest_threat::flat_scores` scores one by one, so it scores them
/// side by side inside the batch's side-by-side answers, and every frame's
/// state hash is still the same on one worker thread as on four.
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
                    on_mission(sim, id, MissionType::Hunt);
                }
            }
        }
    };
    let (one, used) = batched_frames(rules, 48, 1, 10, companies);
    assert!(used > 0, "the hunters' scans take batched answers");
    assert_eq!(batched_frames(rules, 48, 4, 10, companies), (one, used));
}
