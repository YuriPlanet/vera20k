//! Rust regression tests for the batched target scans ([`super`]). A batched
//! answer is checked against the object's own scan on the same world; no
//! native behavior is claimed.
use super::*;
use crate::sim::world::{FrameEffects, ObjectAiCtx};

/// A guarding rifleman with a Soviet rifleman two cells east and another
/// three cells south, both in sight; its passive scan is due. Scans are
/// batched, but no pass has answered them yet.
fn scene(rules: &RuleSet) -> (Simulation, u64, [u64; 2]) {
    let (sim, scanner, riflemen) = scene_with(rules, &[(14, 12), (12, 15)]);
    (sim, scanner, [riflemen[0], riflemen[1]])
}

/// [`scene`] with Soviet riflemen, in sight, at `cells`.
fn scene_with(rules: &RuleSet, cells: &[(u16, u16)]) -> (Simulation, u64, Vec<u64>) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 626);
    let scanner = sim
        .spawn_object("E1", "Americans", 12, 12, 0, rules)
        .unwrap();
    let riflemen = cells
        .iter()
        .map(|&(rx, ry)| {
            sim.spawn_object("E1", "Russians", rx, ry, 0, rules)
                .unwrap()
        })
        .collect();
    let owner = sim.substrate.entities.get(scanner).unwrap().owner();
    for &(rx, ry) in cells {
        sim.fog.mark_visible_for_owner(owner, rx, ry);
    }
    sim.session.set_binary_frame_for_test(100);
    sim.substrate
        .entities
        .get_mut(scanner)
        .unwrap()
        .passive_scan
        .timer_mut_for_test()
        .clear();
    sim.batch_target_scans();
    (sim, scanner, riflemen)
}

/// Put `scanner` on `mission`.
fn on_mission(sim: &mut Simulation, scanner: u64, mission: crate::sim::mission::MissionType) {
    sim.substrate
        .entities
        .get_mut(scanner)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(mission), 0);
}

/// Put `scanner` on Area Guard at `post`.
fn area_guard(sim: &mut Simulation, scanner: u64, post: (u16, u16)) {
    on_mission(sim, scanner, crate::sim::mission::MissionType::AreaGuard);
    let entity = sim.substrate.entities.get_mut(scanner).unwrap();
    entity.set_archive_target(Some(TargetKind::Cell(post.0, post.1)));
}

/// Run `scanner`'s mission handler; answers its target.
fn run_mission(sim: &mut Simulation, rules: &RuleSet, scanner: u64) -> Option<TargetKind> {
    super::super::mission_handlers::dispatch_foot_mission(
        sim,
        scanner,
        rules,
        ObjectAiCtx::default(),
    );
    sim.substrate
        .entities
        .get(scanner)
        .unwrap()
        .attack_target
        .target()
}

fn scan(sim: &mut Simulation, rules: &RuleSet, scanner: u64) -> Option<TargetKind> {
    super::super::target_scan::passive_acquire_step(
        sim,
        scanner,
        Some(rules),
        ObjectAiCtx::default(),
    );
    sim.substrate
        .entities
        .get(scanner)
        .unwrap()
        .attack_target
        .target()
}

/// The pick a live scan of the scene makes, and the other rifleman.
fn live_pick(rules: &RuleSet) -> (u64, u64) {
    let (mut serial, scanner, riflemen) = scene(rules);
    let Some(TargetKind::Entity(pick)) = scan(&mut serial, rules, scanner) else {
        panic!("the guard picks a rifleman");
    };
    (pick, riflemen.into_iter().find(|&id| id != pick).unwrap())
}

/// The answer is the object's own scan, and its scan takes it.
#[test]
fn a_batched_answer_is_the_scan_itself() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut serial, scanner, _) = scene(rules);
    let live = scan(&mut serial, rules, scanner);
    assert!(live.is_some(), "the guard picks a rifleman");
    let (mut batched, _, _) = scene(rules);
    batched.answer_batched_scans(Some(rules), None);
    assert_eq!(scan(&mut batched, rules, scanner), live);
    assert_eq!(batched.batched_scans_used(), 1);
    assert_eq!(batched.state_hash(), serial.state_hash());
}

/// An Area Guard rifleman posted at its cell, its handler due: the handler's
/// scan around the post takes the answer, and a post that moved before its
/// turn scans live.
#[test]
fn an_area_guards_handler_takes_the_answer_for_its_post() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut serial, scanner, _) = scene(rules);
    area_guard(&mut serial, scanner, (12, 12));
    let live = run_mission(&mut serial, rules, scanner);
    assert!(live.is_some(), "the area guard picks a rifleman");
    let (mut batched, _, _) = scene(rules);
    area_guard(&mut batched, scanner, (12, 12));
    batched.answer_batched_scans(Some(rules), None);
    assert_eq!(run_mission(&mut batched, rules, scanner), live);
    assert_eq!(batched.batched_scans_used(), 1);
    assert_eq!(batched.state_hash(), serial.state_hash());

    let (mut moved, _, _) = scene(rules);
    area_guard(&mut moved, scanner, (12, 12));
    moved.answer_batched_scans(Some(rules), None);
    moved
        .substrate
        .entities
        .get_mut(scanner)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(12, 13)));
    assert!(run_mission(&mut moved, rules, scanner).is_some());
    assert_eq!(moved.batched_scans_used(), 0);
}

/// An Area Guard far from its post walks back first: the leash gives it a
/// NavCom before its scan, which GetFireError reads, so it scans live.
#[test]
fn an_area_guard_sent_back_to_its_post_scans_live() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut serial, scanner, _) = scene(rules);
    area_guard(&mut serial, scanner, (1, 1));
    let live = run_mission(&mut serial, rules, scanner);
    let (mut batched, _, _) = scene(rules);
    area_guard(&mut batched, scanner, (1, 1));
    batched.answer_batched_scans(Some(rules), None);
    assert_eq!(run_mission(&mut batched, rules, scanner), live);
    assert!(
        batched
            .substrate
            .entities
            .get(scanner)
            .unwrap()
            .navigation
            .nav_com()
            .is_some(),
        "the leash sends it back"
    );
    assert_eq!(batched.batched_scans_used(), 0);
    assert_eq!(batched.state_hash(), serial.state_hash());
}

/// An answer invalidated before the scanner's turn is not taken, and the
/// scanner scans live: a pick that went into limbo fails the drop step's
/// GetFireError, a pick the pointer-expiry broadcast reached (a kill's or a
/// cloak's Detach_All) or the scanner's house captured is passed over for
/// the other rifleman, and a scanner that no longer stands as it did when the
/// pass began scans afresh.
#[test]
fn an_invalidated_answer_is_scanned_live() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (first, other) = live_pick(rules);
    for case in ["limbo", "released", "captured", "turned"] {
        let (mut batched, scanner, _) = scene(rules);
        batched.answer_batched_scans(Some(rules), None);
        match case {
            "limbo" => {
                let entity = batched.substrate.entities.get_mut(first).unwrap();
                entity.lifecycle.set_in_limbo_for_test(true);
            }
            "released" => {
                batched.detach_all_pointer_expired(first, rules, None, FrameEffects::default());
            }
            "captured" => {
                let americans = batched.substrate.entities.get(scanner).unwrap().owner();
                batched.change_owner(first, americans);
            }
            _ => {
                let entity = batched.substrate.entities.get_mut(scanner).unwrap();
                let facing = entity.body_facing.current(100).wrapping_add(0x2000);
                entity.body_facing.snap(facing, 100);
            }
        }
        let pick = scan(&mut batched, rules, scanner);
        if matches!(case, "limbo" | "captured") {
            assert_eq!(pick, Some(TargetKind::Entity(other)), "{case}");
        } else {
            assert!(pick.is_some(), "{case}");
        }
        assert_eq!(batched.batched_scans_used(), 0, "{case}");
    }
}

/// An answer whose lookups stamped its copy of the Dummy (the east
/// rifleman's cell is unallocated) is dropped, and the object's own scan
/// stamps the shared Dummy as the serial scan does.
#[test]
fn an_answer_that_stamps_the_dummy_is_dropped() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let holed = || {
        let (mut sim, scanner, _) = scene(rules);
        let allocated: Vec<_> = (0..32u16)
            .flat_map(|y| (0..32u16).map(move |x| (x, y)))
            .filter(|&cell| cell != (14, 12))
            .collect();
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.test_set_native_allocated_cells(&allocated);
        // Start from a coordinate no lookup stamps.
        terrain.shared_cell_dummy().stamp_coord(1, 1);
        (sim, scanner)
    };
    let dummy = |sim: &Simulation| {
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .snapshot()
    };
    let (mut serial, scanner) = holed();
    let live = scan(&mut serial, rules, scanner);
    assert_eq!(dummy(&serial).coord, (14, 12), "the scan stamps the Dummy");
    let (mut batched, _) = holed();
    batched.answer_batched_scans(Some(rules), None);
    assert_eq!(
        dummy(&batched).coord,
        (1, 1),
        "the answer's stamps stay private"
    );
    assert_eq!(scan(&mut batched, rules, scanner), live);
    assert_eq!(batched.batched_scans_used(), 0);
    assert_eq!(dummy(&batched), dummy(&serial));
    assert_eq!(batched.state_hash(), serial.state_hash());
}

/// A saved game keeps the batching but not a pass's answers.
#[test]
fn a_save_keeps_the_setting_and_drops_the_answers() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut sim, _, _) = scene(rules);
    sim.answer_batched_scans(Some(rules), None);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "scan_batch", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("the save loads")
        .sim;
    let batch = restored.scan_batch.as_ref().expect("still batching");
    assert!(batch.answers.is_empty());
    let serial = crate::sim::arena_fixture::rival_arena(rules, 626);
    let serial = crate::sim::snapshot::GameSnapshot::save(&serial, 0, 0, "serial", 0);
    let serial = crate::sim::snapshot::GameSnapshot::load(&serial).expect("the save loads");
    assert!(
        serial.sim.scan_batch.is_none(),
        "a serial game stays serial"
    );
}

/// `frames` frames, with scans batched, of a `size` arena that `setup`
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
        .expect("a worker pool");
    pool.install(|| {
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 626, size);
        let grid = sim
            .path_grid_snapshot()
            .map(|grid| (*grid).clone())
            .expect("arena grid");
        setup(&mut sim);
        sim.batch_target_scans();
        let hashes = (0..frames)
            .map(|_| {
                sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
                sim.state_hash()
            })
            .collect();
        (hashes, sim.batched_scans_used())
    })
}

/// Whole frames do not depend on the thread count: two rifle squads face
/// each other across the arena, and every frame's state hash is the same on
/// one worker thread as on four.
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
    let (one, used) = batched_frames(rules, 32, 1, 90, squads);
    assert!(used > 0, "the squads' scans take batched answers");
    assert_eq!(batched_frames(rules, 32, 4, 90, squads), (one, used));
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
                    on_mission(sim, id, crate::sim::mission::MissionType::Hunt);
                }
            }
        }
    };
    let (one, used) = batched_frames(rules, 48, 1, 10, companies);
    assert!(used > 0, "the hunters' scans take batched answers");
    assert_eq!(batched_frames(rules, 48, 4, 10, companies), (one, used));
}
