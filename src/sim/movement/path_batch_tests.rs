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
/// A code-2 re-path at urgency 1, as a Foot whose blockage timer has not run
/// out asks it.
const CODE2: WaitingRequest = WaitingRequest::Code2Ladder { urgency: 1 };

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
            .path_replay_mut()
            .set_directions_for_test(vec![WEST_STEP; 3]);
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
        (CODE2, 1),
        (WaitingRequest::Code2Ladder { urgency: 2 }, 2),
    ] {
        for mover in 0..2 {
            let search = |batched: bool| {
                let (mut sim, movers) = jam(&rules);
                let id = movers[mover];
                if kind != WaitingRequest::NoQueue {
                    let frame = sim.session.binary_frame();
                    let runtime = &mut sim
                        .substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .navigation
                        .path_runtime_mut();
                    runtime.set_path_blocked_for_test(urgency == 2);
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
                    WaitingRequest::Code2Ladder { urgency } => {
                        sim.code2_ladder_request(id, urgency)
                    }
                }
                .expect("a request");
                assert_eq!(request.urgency, urgency);
                let found = sim
                    .foot_find_path(&request, &rules, None, FrameEffects::default())
                    .expect("Find_Path runs");
                if batched {
                    let batch = sim.path_batch.as_ref().unwrap();
                    assert_eq!(batch.used[kind.kind()], 1, "Find_Path used the answer");
                    assert_eq!(
                        batch.ready[&id],
                        ReadySlot {
                            claimed: true,
                            request: kind,
                            answer: None,
                        }
                    );
                }
                let route = sim.substrate.entities.get(id).unwrap();
                (found, route.navigation.path_replay().directions().to_vec())
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

/// An answer serves only the request it was computed for: the same ladder
/// asking at urgency 2 searches for itself.
#[test]
fn an_answer_for_another_request_is_not_used() {
    let rules = rules();
    let (mut sim, [tank, _]) = jam(&rules);
    sim.batch_path_searches();
    assert!(sim.defer_path_search(tank, CODE2));
    sim.answer_batched_path_searches(Some(&rules), None);
    assert!(
        !sim.defer_path_search(tank, CODE2),
        "an answered Foot no longer waits"
    );
    let request = sim.code2_ladder_request(tank, 2).unwrap();
    sim.foot_find_path(&request, &rules, None, FrameEffects::default())
        .expect("Find_Path runs");
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.used, [0, 0]);
    assert!(
        batch.ready[&tank].answer.is_some(),
        "the unused answer is left for the frame"
    );
}

/// A code-2 re-path a capped pass leaves out keeps the urgency it registered
/// at while its Foot registers it again, and loses it when a pass finds no
/// registration at all. The kept urgencies are saved and hashed.
#[test]
fn a_left_out_code2_repath_keeps_its_urgency() {
    let rules = rules();
    let (mut sim, [tank, infantry]) = jam(&rules);
    sim.batch_path_searches();
    let others = 1_000_000..1_000_000 + SEARCHES_PER_PASS as u64;
    let leave_out = |sim: &mut Simulation| {
        for id in others.clone() {
            assert!(sim.defer_path_search(id, WaitingRequest::NoQueue));
        }
        sim.path_batch.as_mut().unwrap().next_turn = others.start;
        sim.answer_batched_path_searches(Some(&rules), None);
        sim.finish_batched_path_searches();
    };
    assert!(sim.defer_path_search(tank, CODE2));
    assert!(sim.defer_path_search(infantry, CODE2));
    leave_out(&mut sim);
    let batch = sim.path_batch.as_ref().unwrap();
    assert!(!batch.answered.contains(&tank) && !batch.answered.contains(&infantry));
    assert_eq!(
        batch.left_out_code2,
        BTreeMap::from([(tank, 1), (infantry, 1)])
    );

    // The tank asks again at urgency 2 and keeps 1; the infantryman asks a
    // no-queue request instead.
    let urgent = WaitingRequest::Code2Ladder { urgency: 2 };
    assert!(sim.defer_path_search(tank, urgent));
    assert!(sim.defer_path_search(infantry, WaitingRequest::NoQueue));
    assert_eq!(
        sim.path_batch.as_ref().unwrap().pending[..2],
        [(tank, CODE2), (infantry, WaitingRequest::NoQueue)]
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "left_out_code2", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("the save loads")
        .sim;
    assert_eq!(
        restored.path_batch.as_ref().unwrap().left_out_code2,
        BTreeMap::from([(tank, 1), (infantry, 1)])
    );
    let hash = sim.state_hash();
    let kept = std::mem::take(&mut sim.path_batch.as_mut().unwrap().left_out_code2);
    assert_ne!(sim.state_hash(), hash, "the kept urgencies are hashed");
    sim.path_batch.as_mut().unwrap().left_out_code2 = kept;

    leave_out(&mut sim);
    assert_eq!(
        sim.path_batch.as_ref().unwrap().left_out_code2,
        BTreeMap::from([(tank, 1)]),
        "only the code-2 re-path is kept"
    );
    // A pass with no registration at all keeps nothing.
    sim.answer_batched_path_searches(Some(&rules), None);
    sim.finish_batched_path_searches();
    assert!(sim.path_batch.as_ref().unwrap().left_out_code2.is_empty());
    assert!(sim.defer_path_search(tank, urgent));
    assert_eq!(sim.path_batch.as_ref().unwrap().pending, [(tank, urgent)]);
}

/// A tank, then an infantryman, blocked with code 2 behind a tank that
/// cannot move registers a re-path at urgency 1. Then a full turn of other
/// registrations goes first every frame: its blockage timer runs out as it
/// waits, and once the pass answers it, its own ladder (Drive's, then
/// Walk's) takes the answer, searched at the urgency 1 it first asked at.
#[test]
fn a_ladder_left_out_past_its_blockage_timer_takes_its_first_urgency() {
    let rules = rules();
    let delay = rules.general.blockage_path_delay_ticks;
    assert!(delay > 0, "the blockage timer runs: {delay}");
    let others = 1_000_000..1_000_000 + SEARCHES_PER_PASS as u64;
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
        let live = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(mover)
                .unwrap()
                .navigation
                .path_runtime()
                .code2_urgency(sim.session.binary_frame())
        };
        // It routes, rolls up to the stuck tank and registers a re-path.
        for _ in 0..300 {
            if sim
                .path_batch
                .as_ref()
                .unwrap()
                .pending
                .contains(&(mover, CODE2))
            {
                break;
            }
            sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        }
        assert!(
            sim.path_batch
                .as_ref()
                .unwrap()
                .pending
                .contains(&(mover, CODE2)),
            "{follower} is blocked with code 2"
        );
        let mut left_out_at_2 = 0;
        for _ in 0..300 {
            for id in others.clone() {
                assert!(sim.defer_path_search(id, WaitingRequest::NoQueue));
            }
            sim.path_batch.as_mut().unwrap().next_turn = others.start;
            sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let batch = sim.path_batch.as_ref().unwrap();
            assert!(!batch.answered.contains(&mover), "{follower} waits");
            if batch.pending.contains(&(mover, CODE2)) && live(&sim) == 2 {
                left_out_at_2 += 1;
                if left_out_at_2 == 3 {
                    break;
                }
            }
        }
        assert_eq!(left_out_at_2, 3, "{follower} waits past its blockage timer");
        let used = sim.path_batch.as_ref().unwrap().used[CODE2.kind()];
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        let batch = sim.path_batch.as_ref().unwrap();
        assert!(batch.answered.contains(&mover), "{follower} is answered");
        assert_eq!(
            batch.used[CODE2.kind()],
            used + 1,
            "{follower}'s ladder takes the urgency-1 answer"
        );
    }
}

/// A request whose target redirects (a building on the destination) is left
/// to the Foot's own Find_Path, which retargets its NavCom before searching.
#[test]
fn a_redirected_request_is_left_to_find_path() {
    let rules = rules();
    let (mut sim, [tank, _]) = jam(&rules);
    spawn(&mut sim, &rules, "HUT", (20, 10));
    let destination = sim.code2_ladder_request(tank, 1).unwrap().destination;
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
    assert!(sim.defer_path_search(tank, CODE2));
    sim.answer_batched_path_searches(Some(&rules), None);
    assert_eq!(sim.path_batch.as_ref().unwrap().ready[&tank].answer, None);
    let request = sim.code2_ladder_request(tank, 1).unwrap();
    sim.foot_find_path(&request, &rules, None, FrameEffects::default())
        .expect("Find_Path runs");
    assert_eq!(sim.path_batch.as_ref().unwrap().used, [0, 0]);
}

/// Each answer is its search's own, whatever thread ran it: the same
/// searches one at a time on this thread, reading every entry fact from the
/// world, agree, for no-queue requests and code-2 ladders, the urgency-2
/// ones started first. Movers along the map edge stamp the shared Dummy from
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
        // Tanks repeat a no-queue request, infantrymen a code-2 ladder's,
        // every third at urgency 2.
        let ladder = WaitingRequest::Code2Ladder {
            urgency: if i % 3 == 2 { 2 } else { 1 },
        };
        movers.extend([(tank, WaitingRequest::NoQueue), (infantry, ladder)]);
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
        let alone =
            dummy.detached(|| sim.batched_search(id, request, &rules, None, &grid, counts, None));
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
            foot.navigation
                .path_replay()
                .remaining_directions()
                .to_vec()
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
            let frame = sim.session.binary_frame() as i32;
            for &(id, kind) in &batch.pending {
                let runtime = sim
                    .substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .navigation
                    .path_runtime();
                assert!(
                    runtime.movement_timer().expired(frame),
                    "{follower}: a Foot that waits asks again"
                );
                waited += usize::from(kind != WaitingRequest::NoQueue);
            }
        }
        let used = sim.path_batch.as_ref().unwrap().used[CODE2.kind()];
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
    assert!(sim.defer_path_search(tank, CODE2));
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
    assert!(sim.defer_path_search(tank, CODE2));
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
    assert!(sim.defer_path_search(tank, CODE2), "a later one waits");
    assert!(
        sim.defer_path_search(infantry, WaitingRequest::NoQueue),
        "an unanswered Foot waits"
    );
    assert_eq!(
        sim.path_batch.as_ref().unwrap().pending,
        [(tank, CODE2), (infantry, WaitingRequest::NoQueue)]
    );
    assert!(sim.defer_path_search(tank, WaitingRequest::NoQueue));
    sim.answer_batched_path_searches(Some(&rules), None);
    let slot = &sim.path_batch.as_ref().unwrap().ready[&tank];
    assert!(slot.answer.is_some());
    assert_eq!(
        slot.request,
        WaitingRequest::NoQueue,
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
        foot.navigation
            .path_replay()
            .remaining_directions()
            .to_vec()
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
    let frame = sim.session.binary_frame();
    let foot = sim.substrate.entities.get_mut(tank).unwrap();
    foot.navigation
        .path_replay_mut()
        .set_directions_for_test(Vec::new());
    foot.navigation.path_runtime_mut().start_movement(frame, 0);
    process(&mut sim, retry);
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.pending.last(), Some(&(tank, WaitingRequest::NoQueue)));
    assert_eq!(batch.used, [1, 0], "the recursion did not search");
}

/// Over the cap a pass takes the registrations from the next turn on,
/// wrapping round to the lowest ids, and moves the turn past the last one
/// taken; under it a pass takes them all and keeps the turn.
#[test]
fn a_capped_pass_takes_its_turn_round_the_ids() {
    let pending = |ids: &[u64]| -> Vec<(u64, WaitingRequest)> {
        ids.iter()
            .map(|&id| (id, WaitingRequest::NoQueue))
            .collect()
    };
    let ids = |turn: Vec<(u64, WaitingRequest)>| -> Vec<u64> {
        turn.into_iter().map(|(id, _)| id).collect()
    };
    let mut next = 5;
    assert_eq!(ids(take_turn(pending(&[2, 4, 6]), &mut next, 3)), [2, 4, 6]);
    assert_eq!(next, 5, "a pass under the cap keeps the turn");
    let five = [2, 4, 6, 8, 10];
    assert_eq!(ids(take_turn(pending(&five), &mut next, 3)), [6, 8, 10]);
    assert_eq!(next, 11);
    assert_eq!(ids(take_turn(pending(&five), &mut next, 3)), [2, 4, 6]);
    assert_eq!(next, 7);
    assert_eq!(ids(take_turn(pending(&five), &mut next, 3)), [2, 8, 10]);
    assert_eq!(next, 3);
}

/// A pass answers at most [`SEARCHES_PER_PASS`] registrations. A Foot it
/// leaves out has no answer, so its request registers again, and the next
/// capped pass starts where this one stopped. The turn is saved and hashed.
#[test]
fn a_pass_answers_at_most_its_cap_and_the_rest_wait() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    sim.batch_path_searches();
    let cap = SEARCHES_PER_PASS as u64;
    // Ids no object holds: each search finds no request to answer.
    let (first, end) = (1_000_000, 1_000_000 + cap + 10);
    for id in first..end {
        assert!(sim.defer_path_search(id, WaitingRequest::NoQueue));
    }
    sim.answer_batched_path_searches(Some(&rules), None);
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.answered, (first..first + cap).collect::<Vec<_>>());
    assert_eq!(batch.next_turn, first + cap);
    assert!(
        sim.defer_path_search(end - 1, WaitingRequest::NoQueue),
        "a Foot the pass left out registers again"
    );
    assert!(
        !sim.defer_path_search(first, WaitingRequest::NoQueue),
        "a Foot the pass answered claims its slot"
    );

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "path_batch_turn", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("the save loads")
        .sim;
    assert_eq!(restored.path_batch.as_ref().unwrap().next_turn, first + cap);
    let hash = sim.state_hash();
    sim.path_batch.as_mut().unwrap().next_turn = 0;
    assert_ne!(sim.state_hash(), hash, "the turn is hashed");
    sim.path_batch.as_mut().unwrap().next_turn = first + cap;

    sim.finish_batched_path_searches();
    for id in first..end {
        assert!(sim.defer_path_search(id, WaitingRequest::NoQueue));
    }
    sim.answer_batched_path_searches(Some(&rules), None);
    let batch = sim.path_batch.as_ref().unwrap();
    let mut second: Vec<u64> = (first + cap..end).chain(first..first + cap - 10).collect();
    second.sort_unstable();
    assert_eq!(batch.answered, second, "the next pass wraps round");
    assert_eq!(batch.next_turn, first + cap - 10);
}

/// An urgency-2 code-2 re-path, as a Foot past its blockage timer asks it.
const URGENT: WaitingRequest = WaitingRequest::Code2Ladder { urgency: 2 };

/// Eight infantrymen with routes across the arena, each answered alone.
fn eight_ladders(rules: &RuleSet) -> (Simulation, Vec<u64>) {
    let (mut sim, _) = arena(rules);
    let ladders = (0..8u16)
        .map(|i| {
            let infantry = spawn(&mut sim, rules, "E1", (1, 3 + 3 * i));
            send(&mut sim, rules, infantry, (30, 24 - 3 * i));
            infantry
        })
        .collect();
    sim.batch_path_searches();
    (sim, ladders)
}

/// A pass answers its urgency-2 re-paths in their turn's order while the
/// ones before stay under its budget of expansions; the rest register again
/// at urgency 2 and go first at the next pass. Which ones it answers does not
/// depend on the threads (one worker takes the re-paths past the budget only
/// after it is spent, and skips them).
#[test]
fn a_pass_answers_urgency2_repaths_within_its_budget() {
    let rules = rules();
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a worker pool");
        pool.install(|| {
            let (mut sim, ladders) = eight_ladders(&rules);
            let register = |sim: &mut Simulation, ids: &[u64]| {
                for &id in ids {
                    assert!(sim.defer_path_search(id, URGENT));
                }
            };
            // Without a budget every re-path is answered; alone, each makes
            // the expansions it makes in the batch.
            register(&mut sim, &ladders);
            sim.answer_path_searches_within(Some(&rules), None, u64::MAX);
            assert_eq!(sim.path_batch.as_ref().unwrap().answered, ladders);
            let grid = sim.path_grid_snapshot().unwrap();
            let counts = sim.movement_pass_cache.current_blocker_plane();
            let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
            let alone: Vec<(Option<BatchedSearch>, u64)> = ladders
                .iter()
                .map(|&id| {
                    count_search_steps(|| {
                        dummy.detached(|| {
                            sim.batched_search(id, URGENT, &rules, None, &grid, counts, None)
                        })
                    })
                })
                .collect();
            assert!(
                alone
                    .iter()
                    .all(|(answer, steps)| answer.is_some() && *steps > 0),
                "every re-path searches"
            );

            // The first two stay under the budget and the third reaches it.
            sim.finish_batched_path_searches();
            register(&mut sim, &ladders);
            let budget = alone[0].1 + alone[1].1 + 1;
            sim.answer_path_searches_within(Some(&rules), None, budget);
            let batch = sim.path_batch.as_ref().unwrap();
            assert_eq!(batch.answered, ladders[..3]);
            for (id, (answer, _)) in ladders.iter().zip(&alone).take(3) {
                assert_eq!(batch.ready[id].answer, *answer, "{id}'s answer");
            }
            for &id in &ladders[3..] {
                assert!(sim.defer_path_search(id, CODE2), "{id} registers again");
                let pending = &sim.path_batch.as_ref().unwrap().pending;
                assert_eq!(pending.last(), Some(&(id, URGENT)), "{id} keeps urgency 2");
            }

            // The next pass takes the left-out ones first, ahead of a lower
            // id that registers anew, and answers the first whatever its
            // budget.
            sim.finish_batched_path_searches();
            register(&mut sim, &ladders[..1]);
            sim.answer_path_searches_within(Some(&rules), None, 1);
            let batch = sim.path_batch.as_ref().unwrap();
            assert_eq!(batch.answered, [ladders[3]]);
            let waiting: Vec<_> = batch
                .left_out_code2
                .iter()
                .map(|(&id, &u)| (id, u))
                .collect();
            let mut expected: Vec<_> = ladders[4..].iter().map(|&id| (id, 2)).collect();
            expected.insert(0, (ladders[0], 2));
            assert_eq!(waiting, expected);
            (
                ladders,
                alone
                    .into_iter()
                    .map(|(_, steps)| steps)
                    .collect::<Vec<_>>(),
            )
        })
    };
    assert_eq!(run(1), run(4));
}

/// Urgency-2 re-paths take their own turn: when the budget and the cap both
/// bind, the cap's turn still moves past the registrations it took.
#[test]
fn the_urgency2_budget_leaves_the_caps_turn_alone() {
    let rules = rules();
    let (mut sim, ladders) = eight_ladders(&rules);
    let cap = SEARCHES_PER_PASS as u64;
    // Ids no object holds: each search finds no request to answer.
    let (first, end) = (1_000_000, 1_000_000 + cap + 10);
    for id in first..end {
        assert!(sim.defer_path_search(id, WaitingRequest::NoQueue));
    }
    for &id in &ladders {
        assert!(sim.defer_path_search(id, URGENT));
    }
    sim.answer_path_searches_within(Some(&rules), None, 1);
    let batch = sim.path_batch.as_ref().unwrap();
    assert_eq!(batch.next_turn, first + cap);
    let answered: Vec<u64> = std::iter::once(ladders[0])
        .chain(first..first + cap)
        .collect();
    assert_eq!(batch.answered, answered);
    assert_eq!(
        batch.left_out_code2.keys().copied().collect::<Vec<_>>(),
        ladders[1..]
    );
}

/// However the registrations change between passes, a Foot registered at
/// every pass is taken within one sweep of the ids, from any turn.
#[test]
fn a_foot_registered_every_pass_gets_its_turn_within_one_sweep() {
    let (limit, top, foot) = (3usize, 30u64, 17u64);
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut draw = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for start in 0..top + 2 {
        let mut next = start;
        let mut passes = 0;
        loop {
            passes += 1;
            let mut ids: Vec<u64> = (1..=top).filter(|_| draw() % 3 == 0).collect();
            if !ids.contains(&foot) {
                ids.push(foot);
                ids.sort_unstable();
            }
            let pending = ids.into_iter().map(|id| (id, WaitingRequest::NoQueue));
            if take_turn(pending.collect(), &mut next, limit)
                .iter()
                .any(|&(id, _)| id == foot)
            {
                break;
            }
            assert!(
                passes <= top.div_ceil(limit as u64) as usize,
                "from turn {start}: still waiting after {passes} passes"
            );
        }
    }
}

/// An order given at once to more Foots than a pass answers: the first pass
/// answers the lowest ids; each Foot it left out still waits with its timer
/// run out, so it asks again, and the next pass routes it. A save between the
/// passes keeps the turn, so the loaded game's next pass routes it as well.
#[test]
fn an_order_to_more_foots_than_a_pass_answers_routes_over_two_passes() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let mut foots = Vec::new();
    for y in 2..12 {
        for x in 2..28 {
            foots.push(spawn(&mut sim, &rules, "TANK", (x, y)));
        }
    }
    assert!(foots.len() > SEARCHES_PER_PASS);
    for &id in &foots {
        send(&mut sim, &rules, id, (16, 28));
    }
    sim.batch_path_searches();
    let pending = |sim: &Simulation| -> Vec<u64> {
        let mut ids: Vec<u64> = sim
            .path_batch
            .as_ref()
            .unwrap()
            .pending
            .iter()
            .map(|&(id, _)| id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let routed = |sim: &Simulation, id: u64| {
        let foot = sim.substrate.entities.get(id).unwrap();
        !foot
            .navigation
            .path_replay()
            .remaining_directions()
            .is_empty()
    };
    for _ in 0..10 {
        if pending(&sim) == foots {
            break;
        }
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    }
    assert_eq!(pending(&sim), foots, "the whole order registers at once");

    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    let (answered, left_out) = foots.split_at(SEARCHES_PER_PASS);
    assert_eq!(sim.path_batch.as_ref().unwrap().answered, answered);
    let frame = sim.session.binary_frame() as i32;
    for &id in left_out {
        assert!(pending(&sim).contains(&id), "{id} asks again");
        let foot = sim.substrate.entities.get(id).unwrap();
        assert!(
            foot.navigation
                .path_runtime()
                .movement_timer()
                .expired(frame)
        );
        assert!(!routed(&sim, id), "{id} waits");
    }

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "path_batch_turns", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("the save loads")
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    // A save carries no map: the load re-installs the arena's cells.
    let restored_grid = crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    restored
        .overlay_grid
        .get_or_insert_with(|| OverlayGrid::new(32, 32));
    assert_eq!(
        restored.path_batch.as_ref().unwrap().next_turn,
        answered[SEARCHES_PER_PASS - 1] + 1
    );
    for (game, grid) in [(&mut sim, &grid), (&mut restored, &restored_grid)] {
        game.advance_tick(&[], Some(&rules), Some(grid), None, 67);
        let batch = game.path_batch.as_ref().unwrap();
        for &id in left_out {
            assert!(batch.answered.contains(&id), "the next pass answers {id}");
            assert!(routed(game, id), "{id} routes");
        }
    }
}
