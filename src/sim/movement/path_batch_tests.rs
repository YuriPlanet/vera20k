//! Rust regression tests for the batched path searches ([`super`]). A
//! batched answer is checked against Find_Path's own search on the same
//! world; no native behavior is claimed.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::NavTargetRef;
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::world::FrameEffects;
use crate::util::fixed_math::SIM_ONE;

const EAST: u16 = 0x4000;
const WEST: u16 = 0xC000;
const WEST_STEP: u8 = 6;

/// A tank, a tank that cannot move, an infantryman and a one-cell building.
fn rules() -> RuleSet {
    // This synthetic scene needs native buildable Clear ground and the
    // explicitly intended one-cell HUT foundation in the fixed ART reader.
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=TANK\n1=STUCK\n[InfantryTypes]\n0=E1\n[BuildingTypes]\n0=HUT\n\
         [TANK]\nStrength=300\nSpeed=6\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [STUCK]\nStrength=300\nSpeed=0\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [E1]\nStrength=100\nSpeed=4\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [HUT]\nStrength=100\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str("[HUT]\nFoundation=1x1\n"))
        .expect("batch rules")
}

/// The flat arena with an overlay grid, which the batch needs.
fn arena(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4D3920);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .expect("arena grid");
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, type_name: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object_at_height(type_name, "Americans", cell.0, cell.1, 0, 0, rules)
        .expect("spawns")
}

fn send(sim: &mut Simulation, rules: &RuleSet, id: u64, cell: (u16, u16)) {
    sim.assign_destination_represented(
        id,
        Some(NavTargetRef::cell(cell.0, cell.1)),
        Some(rules),
        None,
        FrameEffects::default(),
    )
    .expect("takes the destination");
}

/// A Foot rolling at full speed along `facing`.
fn rolling(sim: &mut Simulation, id: u64, facing: u16) {
    let foot = sim.substrate.entities.get_mut(id).unwrap();
    foot.foot_speed.set_speed_fraction(SIM_ONE);
    foot.body_facing = FacingClass::new(facing, 0);
}

/// A tank at (10,10) and an infantryman at (10,14), both rolling east toward
/// cells ten east, each with huts north and south of its next cell and, in
/// that cell, a friendly tank rolling west into the mover's own cell: the
/// code-2 chain from there runs back through the mover. Lifted, the mover
/// clears it and the straight route is cheapest; listed, it jams and the
/// route goes round the huts.
fn jam(rules: &RuleSet) -> (Simulation, [u64; 2]) {
    let (mut sim, _) = arena(rules);
    let mut movers = [0; 2];
    for (mover, (type_name, y)) in [("TANK", 10), ("E1", 14)].into_iter().enumerate() {
        spawn(&mut sim, rules, "HUT", (11, y - 1));
        spawn(&mut sim, rules, "HUT", (11, y + 1));
        let id = spawn(&mut sim, rules, type_name, (10, y));
        send(&mut sim, rules, id, (20, y));
        rolling(&mut sim, id, EAST);
        let blocker = spawn(&mut sim, rules, "TANK", (11, y));
        send(&mut sim, rules, blocker, (2, y));
        rolling(&mut sim, blocker, WEST);
        // A path west, replayed by the urgency-2 markers.
        sim.substrate
            .entities
            .get_mut(blocker)
            .unwrap()
            .navigation
            .path_replay
            .directions = vec![WEST_STEP; 3];
        movers[mover] = id;
    }
    (sim, movers)
}

/// The jam's chains run back through the movers: listed they jam, skipped
/// they clear.
#[test]
fn the_jam_chains_run_through_the_movers() {
    let rules = rules();
    let (sim, movers) = jam(&rules);
    for (mover, y) in movers.into_iter().zip([10, 14]) {
        let jams = |unlisted| {
            sim.astar_code2_chain_jams((11, y), MovementLayer::Ground, unlisted)
                .unwrap()
        };
        assert!(jams(None), "row {y}: the chain runs back through the mover");
        assert!(!jams(Some(mover)), "row {y}: without the mover it clears");
    }
}

/// A batched answer is the search Find_Path makes itself on the same world:
/// a no-queue request at urgency 0; a code-2 ladder at urgency 1, whose
/// markers fall back to the code-2 chain, and at 2, for the tank whose chain
/// runs through its own cell and for the infantryman.
#[test]
fn a_batched_answer_is_the_serial_search() {
    let rules = rules();
    for (kind, urgency) in [
        (WaitingRequest::NoQueue, 0),
        (WaitingRequest::Code2Ladder, 1),
        (WaitingRequest::Code2Ladder, 2),
    ] {
        for mover in 0..2 {
            let search = |batched: bool| {
                let (mut sim, movers) = jam(&rules);
                let id = movers[mover];
                if kind == WaitingRequest::Code2Ladder {
                    let frame = sim.session.binary_frame;
                    let runtime = &mut sim
                        .substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .navigation
                        .path_runtime;
                    runtime.path_blocked = urgency == 2;
                    runtime.start_blocked(frame, 0);
                }
                if batched {
                    sim.batch_path_searches();
                    assert!(sim.defer_path_search(id, kind));
                    sim.answer_batched_path_searches(Some(&rules), None);
                    assert!(!sim.defer_path_search(id, kind), "the request claims it");
                }
                let request = match kind {
                    WaitingRequest::NoQueue => sim.no_queue_request(id),
                    WaitingRequest::Code2Ladder => sim.code2_ladder_request(id),
                }
                .expect("a request");
                assert_eq!(request.urgency, urgency);
                let found = sim
                    .foot_find_path(&request, &rules, None, FrameEffects::default())
                    .expect("Find_Path runs");
                if batched {
                    let batch = sim.path_batch.as_ref().unwrap();
                    assert_eq!(batch.used[kind as usize], 1, "Find_Path used the answer");
                    assert_eq!(
                        batch.ready[&id],
                        ReadySlot {
                            claimed: true,
                            answer: None
                        }
                    );
                }
                let route = sim.substrate.entities.get(id).unwrap();
                (found, route.navigation.path_replay.directions.clone())
            };
            let serial = search(false);
            assert!(
                !serial.1.is_empty(),
                "urgency {urgency}, mover {mover} routes"
            );
            assert_eq!(search(true), serial, "urgency {urgency}, mover {mover}");
        }
    }
}

/// An answer serves only the request it was computed for: a ladder whose
/// blockage timer latched after the batch asks at urgency 2 and searches for
/// itself.
#[test]
fn an_answer_for_another_request_is_not_used() {
    let rules = rules();
    let (mut sim, [tank, _]) = jam(&rules);
    sim.batch_path_searches();
    assert!(sim.defer_path_search(tank, WaitingRequest::Code2Ladder));
    sim.answer_batched_path_searches(Some(&rules), None);
    assert!(
        !sim.defer_path_search(tank, WaitingRequest::Code2Ladder),
        "an answered Foot no longer waits"
    );
    let frame = sim.session.binary_frame;
    let runtime = &mut sim
        .substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .navigation
        .path_runtime;
    runtime.path_blocked = true;
    runtime.start_blocked(frame, 0);
    let request = sim.code2_ladder_request(tank).unwrap();
    assert_eq!(request.urgency, 2);
    sim.foot_find_path(&request, &rules, None, FrameEffects::default())
        .expect("Find_Path runs");
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.used, [0, 0]);
    assert!(
        batch.ready[&tank].answer.is_some(),
        "the unused answer is left for the frame"
    );
}

/// A request whose target redirects (a building on the destination) is left
/// to the Foot's own Find_Path, which retargets its NavCom before searching.
#[test]
fn a_redirected_request_is_left_to_find_path() {
    let rules = rules();
    let (mut sim, [tank, _]) = jam(&rules);
    spawn(&mut sim, &rules, "HUT", (20, 10));
    let destination = sim.code2_ladder_request(tank).unwrap().destination;
    let answer = sim
        .find_path_target_answer(tank, destination, &rules, None)
        .unwrap()
        .expect("the precheck admits the search");
    assert!(
        sim.find_path_redirect(tank, destination, answer, &rules)
            .unwrap()
            .is_some(),
        "the building on the destination redirects (code {answer})"
    );
    sim.batch_path_searches();
    assert!(sim.defer_path_search(tank, WaitingRequest::Code2Ladder));
    sim.answer_batched_path_searches(Some(&rules), None);
    assert_eq!(sim.path_batch.as_ref().unwrap().ready[&tank].answer, None);
    let request = sim.code2_ladder_request(tank).unwrap();
    sim.foot_find_path(&request, &rules, None, FrameEffects::default())
        .expect("Find_Path runs");
    assert_eq!(sim.path_batch.as_ref().unwrap().used, [0, 0]);
}

/// Each answer is its search's own, whatever thread ran it: the same
/// searches one at a time on this thread agree, for no-queue requests and
/// code-2 ladders. Movers along the map edge stamp the shared Dummy from
/// their finishing passes.
#[test]
fn batched_answers_do_not_depend_on_the_threads() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let mut movers = Vec::new();
    for i in 0..12u16 {
        let tank = spawn(&mut sim, &rules, "TANK", (2 + 2 * i, 1));
        send(&mut sim, &rules, tank, (29 - 2 * i, 1));
        let infantry = spawn(&mut sim, &rules, "E1", (1, 3 + 2 * i));
        send(&mut sim, &rules, infantry, (1, 29 - 2 * i));
        // Tanks repeat a no-queue request, infantrymen a code-2 ladder's.
        movers.extend([
            (tank, WaitingRequest::NoQueue),
            (infantry, WaitingRequest::Code2Ladder),
        ]);
    }
    sim.batch_path_searches();
    for &(id, request) in &movers {
        assert!(sim.defer_path_search(id, request));
    }
    sim.answer_batched_path_searches(Some(&rules), None);
    let grid = sim.path_grid_snapshot().unwrap();
    let counts = sim.movement_pass_cache.current_blocker_plane();
    let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
    let ready = &sim.path_batch.as_ref().unwrap().ready;
    let mut routes = 0;
    for &(id, request) in &movers {
        let alone = dummy.detached(|| sim.batched_search(id, request, &rules, None, &grid, counts));
        routes += usize::from(alone.as_ref().is_some_and(|answer| answer.found.is_ok()));
        assert_eq!(ready[&id].answer, alone, "mover {id}");
    }
    assert_eq!(routes, movers.len(), "every mover routes");
}

/// Whole frames do not depend on the thread count: tanks and infantrymen
/// crossing each other's rows re-path through the batch, and every frame's
/// state hash is the same on one worker thread as on four.
#[test]
fn whole_frames_do_not_depend_on_the_thread_count() {
    let rules = rules();
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a worker pool");
        pool.install(|| {
            let (mut sim, grid) = arena(&rules);
            for i in 0..12u16 {
                let tank = spawn(&mut sim, &rules, "TANK", (2 + 2 * i, 4));
                send(&mut sim, &rules, tank, (29 - 2 * i, 4));
                let infantry = spawn(&mut sim, &rules, "E1", (4, 3 + 2 * i));
                send(&mut sim, &rules, infantry, (4, 29 - 2 * i));
            }
            sim.batch_path_searches();
            let hashes: Vec<u64> = (0..150)
                .map(|_| {
                    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
                    sim.state_hash()
                })
                .collect();
            (hashes, sim.path_batch.as_ref().unwrap().used)
        })
    };
    let (one, used) = run(1);
    assert!(
        used.iter().all(|&kind| kind > 0),
        "the crossing movers route and re-path through the batch: {used:?}"
    );
    assert_eq!(run(4), (one, used));
}

/// A Foot's first no-queue request after a new destination waits one frame
/// and then takes the route its own Find_Path finds: a tank's (Drive) and an
/// infantryman's (Walk), round a wall of huts.
#[test]
fn a_waiting_no_queue_request_routes_as_find_path_would() {
    let rules = rules();
    for type_name in ["TANK", "E1"] {
        let setup = |batched: bool| {
            let (mut sim, grid) = arena(&rules);
            for y in 12..20 {
                spawn(&mut sim, &rules, "HUT", (14, y));
            }
            let mover = spawn(&mut sim, &rules, type_name, (6, 16));
            send(&mut sim, &rules, mover, (24, 16));
            if batched {
                sim.batch_path_searches();
            }
            (sim, grid, mover)
        };
        let route = |sim: &Simulation, id: u64| {
            let foot = sim.substrate.entities.get(id).unwrap();
            foot.navigation.path_replay.remaining_directions().to_vec()
        };
        let (mut serial, grid, mover) = setup(false);
        let (mut batched, _, _) = setup(true);
        let mut frames = 0;
        while route(&serial, mover).is_empty() {
            serial.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            batched.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            frames += 1;
            assert!(frames < 30, "{type_name}: the serial Foot routes");
        }
        assert!(route(&batched, mover).is_empty(), "{type_name}: waits");
        assert_eq!(
            batched.path_batch.as_ref().unwrap().pending,
            [(mover, WaitingRequest::NoQueue)],
            "{type_name}"
        );
        batched.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        assert_eq!(
            batched.path_batch.as_ref().unwrap().used,
            [1, 0],
            "{type_name}"
        );
        assert_eq!(route(&batched, mover), route(&serial, mover), "{type_name}");
    }
}

/// A tank, then an infantryman, follows a tank that cannot move down a
/// corridor and stays blocked behind it with code 2. Each pass answers
/// exactly the previous frame's registrations, a Foot that waits has its
/// timer run out, so it asks again the next frame, and its code-2 ladder
/// (Drive's, then Walk's) waits and uses the answer.
#[test]
fn a_waiting_ladder_uses_its_answer_the_next_frame() {
    let rules = rules();
    for follower in ["TANK", "E1"] {
        let (mut sim, grid) = arena(&rules);
        for x in 6..18 {
            spawn(&mut sim, &rules, "HUT", (x, 9));
            spawn(&mut sim, &rules, "HUT", (x, 11));
        }
        let mover = spawn(&mut sim, &rules, follower, (8, 10));
        let stuck = spawn(&mut sim, &rules, "STUCK", (12, 10));
        send(&mut sim, &rules, mover, (24, 10));
        send(&mut sim, &rules, stuck, (24, 10));
        sim.batch_path_searches();
        let mut waited = 0;
        for _ in 0..300 {
            let mut registered: Vec<u64> = sim
                .path_batch
                .as_ref()
                .unwrap()
                .pending
                .iter()
                .map(|&(id, _)| id)
                .collect();
            registered.sort_unstable();
            registered.dedup();
            sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let batch = sim.path_batch.as_ref().unwrap();
            assert_eq!(batch.answered, registered, "{follower}");
            let frame = sim.session.binary_frame as i32;
            for &(id, kind) in &batch.pending {
                let runtime = &sim
                    .substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .navigation
                    .path_runtime;
                assert!(
                    runtime.movement_timer.expired(frame),
                    "{follower}: a Foot that waits asks again"
                );
                waited += usize::from(kind == WaitingRequest::Code2Ladder);
            }
        }
        let used = sim.path_batch.as_ref().unwrap().used[WaitingRequest::Code2Ladder as usize];
        assert!(
            waited > 0 && used > 0,
            "{follower}: code-2 ladders waited {waited}, used {used}"
        );
    }
}

/// A saved game keeps the batching and the Foots waiting for the next
/// frame's batch; this frame's answers are not saved.
#[test]
fn registrations_survive_a_save_and_answers_do_not() {
    let rules = rules();
    let (mut sim, [tank, infantry]) = jam(&rules);
    sim.batch_path_searches();
    assert!(sim.defer_path_search(tank, WaitingRequest::Code2Ladder));
    sim.answer_batched_path_searches(Some(&rules), None);
    assert!(sim.defer_path_search(infantry, WaitingRequest::NoQueue));
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "path_batch", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("the save loads")
        .sim;
    let batch = restored.path_batch.as_ref().expect("still batching");
    assert_eq!(batch.pending, [(infantry, WaitingRequest::NoQueue)]);
    assert!(batch.ready.is_empty());
    let serial = crate::sim::snapshot::GameSnapshot::save(&arena(&rules).0, 0, 0, "serial", 0);
    let serial = crate::sim::snapshot::GameSnapshot::load(&serial).expect("the save loads");
    assert!(
        serial.sim.path_batch.is_none(),
        "a serial game stays serial"
    );
}

/// A pass's answers expire with it, so a request made between live object
/// passes waits for the next batch, as it would after a save and load.
#[test]
fn answers_expire_with_their_pass() {
    let rules = rules();
    let (mut sim, [tank, _]) = jam(&rules);
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    sim.batch_path_searches();
    assert!(sim.defer_path_search(tank, WaitingRequest::Code2Ladder));
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.answered, [tank], "the pass answered the tank");
    assert!(batch.ready.is_empty(), "its answers expired with it");
    assert!(
        sim.defer_path_search(tank, WaitingRequest::NoQueue),
        "a request between passes waits"
    );
}

/// Through its requests a Foot searches at most once a frame, and only when
/// the pass answered it: its first request claims the answer and any later
/// one that frame waits, as does every request of a Foot the pass did not
/// answer. Of a Foot's registrations in a frame, the latest is answered.
#[test]
fn a_foots_first_request_in_a_frame_claims_its_answer() {
    let rules = rules();
    let (mut sim, [tank, infantry]) = jam(&rules);
    sim.batch_path_searches();
    assert!(sim.defer_path_search(tank, WaitingRequest::NoQueue));
    sim.answer_batched_path_searches(Some(&rules), None);
    assert!(
        !sim.defer_path_search(tank, WaitingRequest::NoQueue),
        "the first request searches"
    );
    assert!(
        sim.defer_path_search(tank, WaitingRequest::Code2Ladder),
        "a later one waits"
    );
    assert!(
        sim.defer_path_search(infantry, WaitingRequest::NoQueue),
        "an unanswered Foot waits"
    );
    assert_eq!(
        sim.path_batch.as_ref().unwrap().pending,
        [
            (tank, WaitingRequest::Code2Ladder),
            (infantry, WaitingRequest::NoQueue)
        ]
    );
    assert!(sim.defer_path_search(tank, WaitingRequest::NoQueue));
    sim.answer_batched_path_searches(Some(&rules), None);
    let answer = sim.path_batch.as_ref().unwrap().ready[&tank]
        .answer
        .as_ref();
    assert_eq!(
        answer.map(|answer| answer.request),
        Some(WaitingRequest::NoQueue),
        "the latest registration stands"
    );
}

/// An infantryman's no-queue request waits for the batch and does what the
/// timer-running branch does: the owner's vt+0x548 clears its walking Doing
/// (0x75AF57..0x75AF5F). Next frame it routes with the answer, and when its
/// next cell then refuses it (code 7), the admission retry clears the path
/// and expires +640, and the retry's recursion waits for the batch after,
/// whose answer the repeated request takes.
#[test]
fn a_refused_walk_retry_waits_for_the_next_batch() {
    let rules = rules();
    let route = |sim: &Simulation, id: u64| {
        let foot = sim.substrate.entities.get(id).unwrap();
        foot.navigation.path_replay.remaining_directions().to_vec()
    };
    let (mut sim, [_, infantry]) = jam(&rules);
    sim.batch_path_searches();
    let run = |sim: &mut Simulation, request: &FootPathRequest| {
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(3)
            .unwrap();
        sim.run_foot_path_request(request, Some(&rules), None, FrameEffects::default())
            .expect("the request runs");
        let entity = sim.substrate.entities.get(infantry).unwrap();
        entity.mission_leaf.as_infantry().unwrap().doing()
    };
    let request = sim.no_queue_request(infantry).unwrap();
    assert_eq!(
        run(&mut sim, &request),
        -1,
        "the waiting request clears Doing"
    );
    assert_eq!(
        sim.path_batch.as_ref().unwrap().pending,
        [(infantry, WaitingRequest::NoQueue)]
    );
    assert!(route(&sim, infantry).is_empty(), "the request waited");

    sim.answer_batched_path_searches(Some(&rules), None);
    run(&mut sim, &request);
    assert_eq!(sim.path_batch.as_ref().unwrap().used, [1, 0]);
    assert!(!route(&sim, infantry).is_empty(), "the answer routes");

    let candidate = request.destination;
    let recursion = sim
        .replay_walk_admission_response(infantry, true, candidate, 7, &rules, None)
        .unwrap()
        .expect("code 7 retries once");
    run(&mut sim, &recursion);
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.pending, [(infantry, WaitingRequest::NoQueue)]);
    assert_eq!(batch.used, [1, 0], "the recursion did not search");
    assert!(
        route(&sim, infantry).is_empty(),
        "the retry cleared the path"
    );

    sim.answer_batched_path_searches(Some(&rules), None);
    let repeated = sim.no_queue_request(infantry).unwrap();
    run(&mut sim, &repeated);
    assert_eq!(sim.path_batch.as_ref().unwrap().used, [2, 0]);
    assert!(!route(&sim, infantry).is_empty(), "the next answer routes");
}

/// Drive's no-queue arm in a retry recursion (Process_Movement(0, 0)) waits
/// the same way: without an answer, and after the outer call took the
/// frame's.
#[test]
fn a_drive_retry_recursion_waits_after_the_frames_answer() {
    use super::super::track_fresh::ProcessMovementArgs;
    use super::super::track_process::TrackFamily;
    let rules = rules();
    let (mut sim, [tank, _]) = jam(&rules);
    sim.batch_path_searches();
    let retry = ProcessMovementArgs {
        allow_retry: false,
        force_single: false,
    };
    let process = |sim: &mut Simulation, args| {
        sim.run_track_process_movement(
            tank,
            TrackFamily::Drive,
            args,
            &rules,
            None,
            FrameEffects::default(),
        )
        .expect("Process_Movement runs");
    };
    process(&mut sim, retry);
    assert_eq!(
        sim.path_batch.as_ref().unwrap().pending,
        [(tank, WaitingRequest::NoQueue)],
        "without an answer the recursion waits"
    );

    sim.answer_batched_path_searches(Some(&rules), None);
    process(&mut sim, ProcessMovementArgs::OUTER);
    assert_eq!(sim.path_batch.as_ref().unwrap().used, [1, 0]);
    // The retry's own preparation: no path left and the timer run out.
    let frame = sim.session.binary_frame;
    let foot = sim.substrate.entities.get_mut(tank).unwrap();
    foot.navigation.path_replay.directions.clear();
    foot.navigation.path_runtime.start_movement(frame, 0);
    process(&mut sim, retry);
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.pending.last(), Some(&(tank, WaitingRequest::NoQueue)));
    assert_eq!(batch.used, [1, 0], "the recursion did not search");
}
