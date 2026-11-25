//! Speculated object turns (`world::speculation`): kept results leave the
//! world as the turns run at their slots would, a result whose object an
//! earlier turn of the pass wrote is dropped, a fork counts a steady visit
//! as the walk does, forks copy records into the allocations earlier passes
//! left, one worker makes the world the whole pool does, a pass begun under
//! the size threshold runs as with the switch off, a game loaded mid-way
//! runs its passes as the game it was saved from, the client's voice set
//! chooses no code a fork runs, and a tank firing at a visible enemy loses
//! the turns that fire, none to a per-player read.

use super::speculation::Lost;
use super::{FrameEffects, LifecycleTestEvent, SimSoundEvent, Simulation};
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::GameSnapshot;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\n[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nROT=5\nTurret=yes\nSight=6\n\
         SpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .expect("speculation test rules parse")
}

/// Rules whose tank carries a gun with a flying shell: a shot makes a
/// projectile, which joins Logic.
fn armed_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\n[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=3000\nArmor=heavy\nSpeed=6\nROT=10\nTurret=yes\nSight=10\n\
         Primary=GUN\nSpeedType=Track\nMovementZone=Normal\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\n\
         [GUN]\nDamage=1\nROF=16\nRange=6\nSpeed=20\nProjectile=SHELL\nWarhead=WH\n\n\
         [SHELL]\nInviso=no\n\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("armed test rules parse")
}

/// A skirmish with one player's tanks on Guard at `cells`, idle objects
/// sleeping and each turn drawing from its own stream as in a match, and
/// speculation on or off, from passes of any size.
fn tanks(rules: &RuleSet, cells: &[(u16, u16)], speculate: bool) -> (Simulation, PathGrid) {
    let (mut sim, grid) = tanks_in(rules, cells, speculate, 32);
    sim.speculate_from_slots_for_test(0);
    (sim, grid)
}

/// [`tanks`] on an arena `size` cells wide, from passes of the size a match
/// speculates.
fn tanks_in(
    rules: &RuleSet,
    cells: &[(u16, u16)],
    speculate: bool,
    size: u16,
) -> (Simulation, PathGrid) {
    let mut sim = Simulation::with_seed(0x51EE_9000_0003);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    let grid = crate::sim::arena_fixture::flat_arena_with_size(&mut sim, rules, size);
    let tanks: Vec<MapEntity> = cells
        .iter()
        .map(|&(cell_x, cell_y)| MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x,
            cell_y,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: Some(0),
            high: false,
            mission: None,
            trigger_tag: None,
            recruitable_a: true,
            recruitable_b: true,
            group: -1,
            follows: None,
            structure_columns: Default::default(),
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
        })
        .collect();
    sim.spawn_from_map(&tanks, Some(rules));
    sim.sleep_idle_objects(true);
    sim.separate_turn_streams(true);
    sim.speculate_object_turns(speculate);
    (sim, grid)
}

/// Two enemy tanks three cells apart on the arena, the player's
/// (Americans) and the Russians', as in a match, with speculation on or off.
fn duel(rules: &RuleSet, speculate: bool) -> (Simulation, PathGrid, [u64; 2]) {
    let mut sim = Simulation::with_seed(0x51EE_9000_0003);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, side, None, human, 0, 10,
            ));
    }
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let tanks = [("Americans", 10), ("Russians", 13)].map(|(owner, cell_x)| {
        sim.spawn_object("MTNK", owner, cell_x, 10, 0, rules)
            .expect("the tank spawns")
    });
    sim.sleep_idle_objects(true);
    sim.separate_turn_streams(true);
    sim.speculate_object_turns(speculate);
    (sim, grid, tanks)
}

fn order_move(sim: &mut Simulation, id: u64, target: (u16, u16)) {
    let americans = sim.interner.get("Americans").unwrap();
    sim.queue_command(CommandEnvelope::new(
        americans,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: id,
            target_rx: target.0,
            target_ry: target.1,
        },
    ));
}

fn order_attack(sim: &mut Simulation, attacker: u64, target: u64) {
    let americans = sim.interner.get("Americans").unwrap();
    sim.queue_command(CommandEnvelope::new(
        americans,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: attacker,
            target_id: target,
        },
    ));
}

fn tick(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
    let commands = sim.take_due_commands();
    let _ = sim.advance_tick(&commands, Some(rules), Some(grid), None, 67);
}

/// Three tanks far apart, one of them driving across the arena. With the
/// switch on, the idle tanks' turns and the driver's inside a cell are kept;
/// its cell steps are lost and run at their slots, and its path search runs
/// there untried, as a turn of a unit standing with a NavCom. After every
/// frame the world is its twin's with the switch off.
#[test]
fn kept_turns_leave_the_world_as_turns_at_their_slots_do() {
    let rules = rules();
    let cells = [(4, 4), (26, 4), (26, 26)];
    let (mut on, grid) = tanks(&rules, &cells, true);
    let (mut off, off_grid) = tanks(&rules, &cells, false);
    let driver = on.substrate.logic.as_slice()[0];
    for frame in 0..240 {
        if frame == 10 {
            order_move(&mut on, driver, (18, 14));
            order_move(&mut off, driver, (18, 14));
        }
        tick(&mut on, &rules, &grid);
        tick(&mut off, &rules, &off_grid);
        assert_eq!(
            on.state_hash(),
            off.state_hash(),
            "the twins differ after frame {frame}"
        );
    }
    assert_eq!(
        serde_json::to_value(&on.substrate.entities).unwrap(),
        serde_json::to_value(&off.substrate.entities).unwrap(),
    );
    assert_ne!(
        on.substrate.entities.get(driver).unwrap().position.rx(),
        4,
        "the driver drove"
    );
    let stats = on.object_pass_stats();
    assert!(
        stats.committed > 300 && stats.lost(Lost::CellStep) > 0 && stats.kept_invalid == 0,
        "{stats}"
    );
    assert_eq!(off.object_pass_stats().passes, 0);
}

/// A kept result applies at its slot only while no turn before it in the
/// pass wrote its object: a write in between drops it, so that the turn
/// runs at its slot.
#[test]
fn a_kept_result_is_dropped_once_an_earlier_turn_writes_its_object() {
    let rules = rules();
    // A pass after `frames` frames, with the second tank written before its
    // slot or not: whether its kept result applied.
    let applied = |frames: usize, written_first: bool| {
        let (mut sim, grid) = tanks(&rules, &[(4, 4), (26, 26)], true);
        for _ in 0..frames {
            tick(&mut sim, &rules, &grid);
        }
        let id = sim.substrate.logic.as_slice()[1];
        let mut effects = FrameEffects::empty();
        // As the live object pass begins.
        sim.begin_turn_streams();
        let mut pass = sim
            .speculate_live_object_turns(&rules, None, &mut effects)
            .expect("the switch is on");
        if written_first {
            sim.substrate
                .entities
                .get_mut(id)
                .expect("the tank is stored");
        }
        sim.enter_turn_stream(id);
        let applied = sim
            .commit_kept_turn(id, &mut pass, &rules, &mut effects)
            .is_some();
        sim.leave_turn_stream(id);
        sim.finish_speculated_pass(pass);
        sim.end_turn_streams();
        applied
    };
    let frames = (30..90)
        .find(|&frames| applied(frames, false))
        .expect("an idle tank's turn is kept");
    assert!(
        !applied(frames, true),
        "the kept result is dropped once its tank was written"
    );
}

/// A steady tank's turn on a fork counts its AI visit as its turn in the walk
/// does (`world::steady_ai`): the fork marks it steady again from the
/// pass-start state and notes the visit, which the pass counts where the
/// result applies, and no field write drops the turn. Every frame leaves the
/// records its twin's walk leaves with the switch off.
#[test]
fn a_steady_visit_counted_on_a_fork_leaves_the_walks_record() {
    let rules = rules();
    let cells = [(4, 4), (26, 26)];
    let (mut on, grid) = tanks(&rules, &cells, true);
    let (mut off, off_grid) = tanks(&rules, &cells, false);
    // The tanks settle and go steady.
    for _ in 0..60 {
        tick(&mut on, &rules, &grid);
        tick(&mut off, &rules, &off_grid);
    }
    let visits = (on.steady_ai_visits(), off.steady_ai_visits());
    let before = on.object_pass_stats().clone();
    for frame in 0..30 {
        tick(&mut on, &rules, &grid);
        tick(&mut off, &rules, &off_grid);
        assert_eq!(
            serde_json::to_value(&on.substrate.entities).unwrap(),
            serde_json::to_value(&off.substrate.entities).unwrap(),
            "the twins' records differ after frame {frame}"
        );
    }
    let counted = (
        on.steady_ai_visits() - visits.0,
        off.steady_ai_visits() - visits.1,
    );
    assert!(counted.1 > 0, "the walk counted steady visits");
    assert_eq!(counted.0, counted.1, "the forks counted what the walk did");
    let stats = on.object_pass_stats();
    assert!(stats.committed > before.committed, "{stats}");
    assert_eq!(stats.fields, before.fields, "{stats}");
}

/// A turn on a fork writes its copy of its object into a record allocation
/// an earlier pass left (a record a kept result replaced, a copy no result
/// applied): once there is one for every slot, no copy takes a new one.
#[test]
fn turns_on_forks_copy_into_the_allocations_earlier_passes_left() {
    let rules = rules();
    let cells = [(4, 4), (26, 4), (26, 26)];
    let (mut sim, grid) = tanks(&rules, &cells, true);
    for _ in 0..60 {
        tick(&mut sim, &rules, &grid);
    }
    let stats = sim.object_pass_stats();
    assert!(stats.committed > 10, "{stats}");
    assert!(stats.new_records <= cells.len() as u64, "{stats}");
}

/// Each verdict depends on the pass-start world alone, whichever turns a
/// worker's fork ran before: a run on one worker and one on the whole pool
/// make the same world, frame by frame.
#[test]
fn one_worker_and_the_whole_pool_make_the_same_world() {
    let rules = rules();
    let run = || {
        let cells: Vec<(u16, u16)> = (0..4)
            .flat_map(|x| (0..4).map(move |y| (4 + 6 * x, 4 + 6 * y)))
            .collect();
        let (mut sim, grid) = tanks(&rules, &cells, true);
        let ids = sim.substrate.logic.as_slice().to_vec();
        let mut hashes = Vec::new();
        for frame in 0..120 {
            if frame == 5 {
                for (&id, target) in ids.iter().zip([(27, 27), (2, 27), (27, 2), (14, 14)]) {
                    order_move(&mut sim, id, target);
                }
            }
            tick(&mut sim, &rules, &grid);
            hashes.push(sim.state_hash());
        }
        (hashes, sim.object_pass_stats().committed)
    };
    let pool = run();
    let mut one = None;
    crate::sim::parallel::on_single_worker_for_test(|| one = Some(run()));
    assert!(pool.1 > 0, "turns were kept");
    assert_eq!(one.expect("the run finished"), pool);
}

/// A pass that begins at the size threshold runs turns on forks. One that
/// begins a Logic slot under it runs every turn in the walk and leaves the
/// world its twin's with the switch off: also the pass in which a tank's
/// shot puts a shell into Logic and so brings it to the threshold, for the
/// slots are counted once, as the pass begins.
#[test]
fn a_pass_begun_under_the_threshold_runs_as_with_the_switch_off() {
    let rules = armed_rules();
    let (mut on, grid, [shooter, target]) = duel(&rules, true);
    let (mut off, off_grid, _) = duel(&rules, false);
    let slots = on.substrate.logic.len();
    on.speculate_from_slots_for_test(slots);
    tick(&mut on, &rules, &grid);
    tick(&mut off, &rules, &off_grid);
    assert_eq!(
        on.object_pass_stats().passes,
        1,
        "a pass begun at the threshold runs turns on forks"
    );
    assert_eq!(on.state_hash(), off.state_hash());
    on.speculate_from_slots_for_test(slots + 1);
    order_attack(&mut on, shooter, target);
    order_attack(&mut off, shooter, target);
    for frame in 1..120 {
        assert_eq!(
            on.substrate.logic.len(),
            slots,
            "the pass of frame {frame} begins under the threshold"
        );
        let before = on.object_pass_stats().clone();
        on.clear_lifecycle_test_events_for_test();
        tick(&mut on, &rules, &grid);
        tick(&mut off, &rules, &off_grid);
        let after = on.object_pass_stats();
        assert_eq!(
            (after.passes, after.unspeculated),
            (before.passes, before.unspeculated + 1),
            "the pass of frame {frame} ran every turn in the walk"
        );
        assert_eq!(
            on.state_hash(),
            off.state_hash(),
            "the twins differ after frame {frame}"
        );
        if on
            .lifecycle_test_events_for_test()
            .contains(&LifecycleTestEvent::LogicAppended)
        {
            return;
        }
    }
    panic!("no shot put a shell into Logic");
}

/// A game loaded mid-way, its passes of a match's size (4,096 tanks), runs
/// them as the game it was saved from: both run and keep the same turns on
/// forks and reach the same hash, frame by frame. The game that goes on
/// holds smudge dirt no save keeps, as a smudge write after its frame's
/// flush leaves it; its pass publishes the dirt before deciding whether to
/// run turns on forks, as the loaded game's finds none.
#[test]
fn a_game_loaded_mid_way_runs_its_passes_as_the_game_it_was_saved_from() {
    let rules = rules();
    let size = 72;
    let cells: Vec<(u16, u16)> = (4..68).flat_map(|x| (4..68).map(move |y| (x, y))).collect();
    let (mut sim, grid) = tanks_in(&rules, &cells, true, size);
    assert!(sim.substrate.logic.len() >= 4096);
    *sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(size, size));
    for _ in 0..3 {
        tick(&mut sim, &rules, &grid);
    }
    assert!(
        sim.object_pass_stats().kept > 0,
        "{}",
        sim.object_pass_stats()
    );
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "speculation", 0);
    let mut loaded = GameSnapshot::load(&bytes).expect("the save loads").sim;
    loaded
        .restore_after_snapshot_load()
        .expect("the save restores");
    let loaded_grid = crate::sim::arena_fixture::flat_ground_with_size(&mut loaded, &rules, size);
    // A match's switches, which no save carries (`app::persistence`).
    loaded.sleep_idle_objects(true);
    loaded.separate_turn_streams(true);
    loaded.speculate_object_turns(true);
    // Native load reseeds the Scenario stream and clears each Foot's move
    // sound (FootClass::Load 0x4DB60D..0x4DB624).
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    for id in sim.substrate.logic.as_slice().to_vec() {
        sim.substrate.restore_move_sound_state_after_load(id);
    }
    assert_eq!(sim.state_hash(), loaded.state_hash(), "the loaded game");
    sim.smudge_grid
        .as_mut()
        .expect("the smudge grid")
        .test_force_set(30, 30, Default::default());
    let before = sim.object_pass_stats().clone();
    for frame in 0..6 {
        tick(&mut sim, &rules, &grid);
        tick(&mut loaded, &rules, &loaded_grid);
        assert_eq!(
            sim.state_hash(),
            loaded.state_hash(),
            "the games differ after frame {frame}"
        );
    }
    let (went_on, loaded) = (sim.object_pass_stats(), loaded.object_pass_stats());
    assert_eq!(
        [
            went_on.passes - before.passes,
            went_on.unspeculated - before.unspeculated,
            went_on.speculated - before.speculated,
            went_on.kept - before.kept,
            went_on.committed - before.committed,
        ],
        [
            loaded.passes,
            loaded.unspeculated,
            loaded.speculated,
            loaded.kept,
            loaded.committed,
        ],
        "\n{went_on}\n{loaded}"
    );
    assert_eq!(loaded.passes, 6, "{loaded}");
}

/// The client's voice set chooses no code a turn on a fork runs: of twins
/// whose steady tanks' turns are kept, the one waiting on a tank's voice
/// keeps, applies and counts as steady what the other does, and its frames
/// still record the tank's voice visit (Techno6F9EBB).
#[test]
fn a_voice_wait_on_one_twin_keeps_the_turns_of_the_other() {
    let rules = rules();
    let cells = [(4, 4), (26, 26)];
    let (mut waiting, grid) = tanks(&rules, &cells, true);
    let (mut other, other_grid) = tanks(&rules, &cells, true);
    // The tanks settle and go steady.
    for _ in 0..60 {
        tick(&mut waiting, &rules, &grid);
        tick(&mut other, &rules, &other_grid);
    }
    let tank = waiting.substrate.logic.as_slice()[0];
    let counts = |sim: &Simulation| {
        let stats = sim.object_pass_stats();
        [
            stats.speculated,
            stats.kept,
            stats.committed,
            sim.steady_ai_visits(),
        ]
    };
    let before = (counts(&waiting), counts(&other));
    for frame in 0..30 {
        waiting.sound_events.clear_for_test();
        waiting.prepare_unit_voice_visits([tank].into());
        tick(&mut waiting, &rules, &grid);
        tick(&mut other, &rules, &other_grid);
        assert!(
            waiting.sound_events.as_slice().iter().any(
                |event| matches!(event, SimSoundEvent::UnitVoiceVisit { owner } if *owner == tank)
            ),
            "frame {frame} recorded the tank's voice visit"
        );
        assert_eq!(
            waiting.state_hash(),
            other.state_hash(),
            "the twins differ after frame {frame}"
        );
    }
    let delta =
        |now: [u64; 4], then: [u64; 4]| std::array::from_fn::<u64, 4, _>(|at| now[at] - then[at]);
    let (waited, went) = (
        delta(counts(&waiting), before.0),
        delta(counts(&other), before.1),
    );
    assert!(
        went[2] > 0 && went[3] > 0,
        "turns were kept and counted steady"
    );
    assert_eq!(waited, went, "speculated, kept, applied, steady");
}

/// A tank firing at a visible enemy asks GetFireError every turn, whose
/// visual state (T17) reads the target's owner byte, a per-player value,
/// only of an `Invisible=` target: its turns on forks are lost to its shots
/// (`Lost::Fire`), none to a per-player read (`Lost::Local`).
#[test]
fn a_tank_firing_at_a_visible_enemy_loses_the_turns_that_fire() {
    let rules = armed_rules();
    let (mut sim, grid, [shooter, target]) = duel(&rules, true);
    sim.speculate_from_slots_for_test(0);
    order_attack(&mut sim, shooter, target);
    for _ in 0..120 {
        tick(&mut sim, &rules, &grid);
    }
    let stats = sim.object_pass_stats();
    assert!(stats.lost(Lost::Fire) > 0 && stats.committed > 0, "{stats}");
    assert_eq!(stats.lost(Lost::Local), 0, "{stats}");
}
