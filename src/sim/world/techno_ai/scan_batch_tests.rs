//! Rust regression tests for the batched target scans ([`super`]). A batched
//! answer is checked against the object's own scan on the same world; no
//! native behavior is claimed.
use super::*;
use crate::sim::world::{FrameEffects, ObjectAiCtx};

/// A guarding rifleman with a Soviet rifleman two cells east and another
/// three cells south, both in sight; its passive scan is due. Scans are
/// batched, but no pass has answered them yet.
fn scene(rules: &RuleSet) -> (Simulation, u64, [u64; 2]) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 626);
    let scanner = sim
        .spawn_object("E1", "Americans", 12, 12, 0, rules)
        .unwrap();
    let east = sim
        .spawn_object("E1", "Russians", 14, 12, 0, rules)
        .unwrap();
    let south = sim
        .spawn_object("E1", "Russians", 12, 15, 0, rules)
        .unwrap();
    let owner = sim.substrate.entities.get(scanner).unwrap().owner();
    sim.fog.mark_visible_for_owner(owner, 14, 12);
    sim.fog.mark_visible_for_owner(owner, 12, 15);
    sim.session.binary_frame = 100;
    sim.substrate
        .entities
        .get_mut(scanner)
        .unwrap()
        .passive_scan
        .timer_mut_for_test()
        .clear();
    sim.batch_target_scans();
    (sim, scanner, [east, south])
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

/// An attack-mover's Arrival_Target_Handler takes the answer too.
#[test]
fn an_attack_movers_handler_takes_the_answer() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let attack_move = |sim: &mut Simulation, scanner: u64| {
        sim.substrate
            .entities
            .get_mut(scanner)
            .unwrap()
            .mega_mission = crate::sim::mission::mega_mission::MegaMission::attack_move(Some(
            TargetKind::Cell(28, 12),
        ));
    };
    let (mut serial, scanner, _) = scene(rules);
    attack_move(&mut serial, scanner);
    let live = scan(&mut serial, rules, scanner);
    assert!(live.is_some(), "the attack-mover picks a rifleman");
    let (mut batched, _, _) = scene(rules);
    attack_move(&mut batched, scanner);
    batched.answer_batched_scans(Some(rules), None);
    assert_eq!(scan(&mut batched, rules, scanner), live);
    assert_eq!(batched.batched_scans_used(), 1);
    assert_eq!(batched.state_hash(), serial.state_hash());
}

/// A pick that went into limbo before the scanner's turn fails the drop
/// step's GetFireError, and the scanner scans live for the other rifleman.
#[test]
fn a_pick_gone_before_the_turn_is_scanned_live() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (first, other) = live_pick(rules);
    let (mut batched, scanner, _) = scene(rules);
    batched.answer_batched_scans(Some(rules), None);
    batched
        .substrate
        .entities
        .get_mut(first)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert_eq!(
        scan(&mut batched, rules, scanner),
        Some(TargetKind::Entity(other))
    );
    assert_eq!(batched.batched_scans_used(), 0);
}

/// A pick the pointer-expiry broadcast reached before the scanner's turn (a
/// kill's or a cloak's Detach_All) is not taken.
#[test]
fn a_released_pick_is_scanned_live() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (first, _) = live_pick(rules);
    let (mut batched, scanner, _) = scene(rules);
    batched.answer_batched_scans(Some(rules), None);
    batched.detach_all_pointer_expired(first, rules, None, FrameEffects::default());
    assert!(scan(&mut batched, rules, scanner).is_some());
    assert_eq!(batched.batched_scans_used(), 0);
}

/// A pick captured by the scanner's house before its turn is not taken; the
/// scanner scans live for the other rifleman.
#[test]
fn a_captured_pick_is_scanned_live() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (first, other) = live_pick(rules);
    let (mut batched, scanner, _) = scene(rules);
    batched.answer_batched_scans(Some(rules), None);
    let americans = batched.substrate.entities.get(scanner).unwrap().owner();
    batched.change_owner(first, americans);
    assert_eq!(
        scan(&mut batched, rules, scanner),
        Some(TargetKind::Entity(other))
    );
    assert_eq!(batched.batched_scans_used(), 0);
}

/// A scanner that no longer stands as it did when the pass began scans
/// live.
#[test]
fn a_changed_scanner_scans_live() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let (mut batched, scanner, _) = scene(rules);
    batched.answer_batched_scans(Some(rules), None);
    let entity = batched.substrate.entities.get_mut(scanner).unwrap();
    let facing = entity.body_facing.current(100).wrapping_add(0x2000);
    entity.body_facing.snap(facing, 100);
    assert!(scan(&mut batched, rules, scanner).is_some());
    assert_eq!(batched.batched_scans_used(), 0);
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

/// Whole frames do not depend on the thread count: two rifle squads face
/// each other across the arena, and every frame's state hash is the same on
/// one worker thread as on four.
#[test]
fn whole_frames_do_not_depend_on_the_thread_count() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a worker pool");
        pool.install(|| {
            let mut sim = crate::sim::arena_fixture::rival_arena(rules, 626);
            let grid = sim
                .path_grid_snapshot()
                .map(|grid| (*grid).clone())
                .expect("arena grid");
            for i in 0..8u16 {
                for (house, x) in [("Americans", 10), ("Russians", 15)] {
                    sim.spawn_object("E1", house, x, 8 + 2 * i, 0, rules)
                        .unwrap();
                }
            }
            sim.batch_target_scans();
            let hashes: Vec<u64> = (0..90)
                .map(|_| {
                    sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
                    sim.state_hash()
                })
                .collect();
            (hashes, sim.batched_scans_used())
        })
    };
    let (one, used) = run(1);
    assert!(used > 0, "the squads' scans take batched answers");
    assert_eq!(run(4), (one, used));
}
