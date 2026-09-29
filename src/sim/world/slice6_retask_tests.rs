//! Slice 6 — verb API + dispatch-adoption integration tests.
//!
//! Two jobs:
//!   1. `replay_hash_stable_through_slice6` — the behavior-preserving gate. A
//!      scripted skirmish drives every retasking command site (Move / Stop /
//!      Attack / ForceAttack / ForceAttackCell / AttackMove) and asserts the
//!      end-of-run `state_hash()` equals the committed baseline. At the slice's
//!      introduction, this exposed wrong `DockTeardown` subsets and dropped
//!      legacy-field clears. Later hash-schema changes require a separately
//!      proven composition-only re-baseline.
//!   2. The verb-write + retaliation-gate tripwires (added below the gate).

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::AttackTarget;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::OrderIntent;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use std::collections::BTreeMap;

fn slice6_rules() -> RuleSet {
    // Two attack-capable vehicles + an infantry; ranges short enough that no
    // auto-combat fires during the scripted window (commands drive everything,
    // while constructor and Walk head-selection draws still use Scenario RNG).
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GACNST\n\n\
         [E1]\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [GACNST]\nStrength=1000\nArmor=wood\nFoundation=4x3\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    );
    RuleSet::from_ini(&ini).expect("slice6 test rules should parse")
}

fn cmd_envelope(
    sim: &Simulation,
    owner: &str,
    execute_tick: u64,
    payload: Command,
) -> CommandEnvelope {
    let owner_id = sim
        .interner
        .get(owner)
        .unwrap_or_else(|| panic!("owner '{owner}' not interned"));
    CommandEnvelope::new(owner_id, execute_tick, payload)
}

fn unit(owner: &str, type_id: &str, cx: u16, cy: u16, cat: EntityCategory) -> MapEntity {
    MapEntity {
        owner: owner.to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: cx,
        cell_y: cy,
        facing: 64,
        category: cat,
        sub_cell: 0,
        veterancy: 0,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
        structure_ai_sellable: false,
        structure_ai_repairable: false,
    }
}

// Schema171: live type acceleration preserves retasked track progression;
// fresh turning defers admission.
// See docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md, PR415 attribution.
// Infantry ctor 517BBD supplies PrimaryFacing ROT 127 (asserted below).
// 2026-09-23 Drive/Ship Process path request (behavior, not composition):
// Drive/Ship orders from every producer (player, pursuit, rally, miners) are
// accepted by the Unit setter without an order-time A*, PowerOn or redirect.
// This fixture has no native zone topology, so the first Process searches in
// the legacy inline lane, not the Find_Path owner. The pins move with that
// state; the RNG stream pins, per-tick replay equality and route tripwires in
// this file are unchanged. The old values are in the commit that moved them.
// Schema202 folds the object's rearm timer (TechnoClass+0x2EC, started at the
// construction frame as the constructor does) in place of the AttackTarget
// cooldown/burst-delay counters and the cloak copy: composition only.
// Before(202) reproduces the v201 pin; per-tick replay, the RNG stream pins and
// the route tripwires are unchanged.
// 2026-09-25 combat chain 1 (snapshot 204): state only. Both RNG streams,
// every entity's cell and health match 947c7044 on all 16 frames (traced);
// the pins move with retained object state and the schema 204 folds (house
// ROF bias, bullet OnBridge). Old values: the commit that moved them.
// 2026-09-25 combat chain 2 (snapshot 205): the tanks take Guard on Unlimbo
// (UnitClass::Enter_Idle_Mode 0x00738970) and draw its cadence from frame 0,
// which moves the infantry's native paid-Walk sub-cell pick (walk_paid_step
// slice6 rows regenerated natively). Old values: the commit that moved them.
// Schema208 folds every object's crash latch and its AI edge (Foot+0x425/
// +0x426) and a Fly's fall counter: composition only. Before(208) reproduces
// the v207 pin (the ore-field schema moved nothing here);
// the RNG stream pins are unchanged.
// 2026-09-26 retired weapon identity (snapshot 212, composition only): the
// entity hash no longer folds `current_weapon_ref`, the weapon id of the last
// live selection, so this one step re-pins every projection in this test, the
// constructor-rate probe included. Ceremony: with only that fold line deleted
// from the previous tree, these pins fail and nothing else in the lib suite
// moves; their `left` values are pasted here, and the full retirement
// reproduces every one. Per-tick record/replay equality, the RNG streams and
// the actors' health are unchanged: the only change to these pins is the
// removed fold. Old values: the commit that moved them.
// Schema217 composition only for this fixture: the native constructor cursor,
// native object IDs and retained fallback-cell Land now enter the hash. The
// repair diagnostic reproduced the prior current pin with Before(217), and
// all earlier projections, replay/behavior checks and RNG receipts still passed.
// These Rust regression receipts do not establish native gameplay parity.
// Schema220 retires the two VERA rally copies (the house's `rally_point` and
// each building's `rally_target`; the rally is the factory's ArchiveTarget):
// composition only, as no rally is set here. Before(220) folds their empty
// values and reproduces the prior current pin; every earlier projection,
// per-tick replay and the RNG receipts are unchanged.
// 2026-09-28 one body facing (snapshot 237, composition only; #580): each
// object's hash folds its body FacingClass (`+0x388`) in place of the retired
// 8-bit facing mirror, turn target and optional turn interpolator, and a
// building's `+0x388` moves from the turret slot into it, in every projection.
// No schema can rebuild the mirror or the target, so this one step re-pins
// every projection in this test, the constructor-rate probe included. Ceremony:
// on origin/main 99935d1d and on this change, a probe hash folding no facing,
// and one folding only each object's body heading word at the hashed frame (the
// old tree's interpolator, else its mirror), matched at all 16 ticks, RNG
// streams included (the probe patch was not committed): every other fold and
// every object's heading are unchanged, so the only change to these pins is the
// fold. Drive's never-written turn target (`DriveTurnState`) left the fold in
// the same step: with its four default fields folded back in their old place,
// this change reproduced every facing-only pin (final 0x6CAD_D2FC_438A_0BC2),
// RNG streams included. Old values: the commit that moved them.
const SLICE6_BASELINE_HASH: u64 = 0x8CB9_BA0F_43C5_BB2A;

#[test]
fn replay_hash_stable_through_slice6() {
    let rules = slice6_rules();
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let grid = PathGrid::new(64, 64);
    let mut sim = Simulation::new();
    // id 1: Americans MTNK (the unit we retask). id 2: enemy MTNK (Soviet, hostile
    // by default — no alliance entry). id 3: Americans E1 (second attacker).
    sim.spawn_from_map(
        &[
            unit("Americans", "MTNK", 3, 3, EntityCategory::Unit),
            unit("Soviet", "MTNK", 25, 3, EntityCategory::Unit),
            unit("Americans", "E1", 5, 5, EntityCategory::Infantry),
        ],
        Some(&rules),
        &heights,
    );

    // (execute_tick, command) — apply_due_commands fires each when self.session.tick+1 == tick.
    let script: &[(u64, Command)] = &[
        (
            1,
            Command::Move {
                entity_id: 1,
                target_rx: 10,
                target_ry: 10,
                queue: false,
            },
        ),
        (
            3,
            Command::AttackMove {
                entity_id: 1,
                target_rx: 15,
                target_ry: 3,
                queue: false,
            },
        ),
        (
            5,
            Command::ForceAttackCell {
                attacker_id: 1,
                target_rx: 18,
                target_ry: 3,
            },
        ),
        (
            7,
            Command::ForceAttack {
                attacker_id: 1,
                target_id: 2,
            },
        ),
        (
            9,
            Command::Attack {
                attacker_id: 3,
                target_id: 2,
            },
        ),
        (11, Command::Stop { entity_id: 1 }),
    ];

    let mut log = ReplayLog::new(ReplayHeader {
        version: 1,
        pixel_conversion_bounds: sim.session.pixel_conversion_bounds,
        tick_hz: 15,
        seed: sim.session.seed,
        map_name: "slice6_retask".to_string(),
        rules_hash: rules.simulation_config_hash(),
    });
    let mut stopped_head = None;
    let walk_vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_paid_step.json"
    ))
    .unwrap();
    let paid_steps: Vec<_> = walk_vectors
        .iter()
        .filter(|row| {
            row["input"]["name"]
                .as_str()
                .unwrap()
                .starts_with("slice6_paid_step_")
        })
        .collect();
    assert_eq!(paid_steps.len(), 5);
    for tick in 0..16u64 {
        let due: Vec<CommandEnvelope> = script
            .iter()
            .filter(|(t, _)| *t == tick + 1)
            .map(|(t, c)| cmd_envelope(&sim, "Americans", *t, c.clone()))
            .collect();
        let result = sim.advance_tick(&due, Some(&rules), &heights, Some(&grid), None, 67);
        assert!(result.frame_committed, "retask frame {tick} must commit");
        assert_eq!(
            result.executed_commands,
            due.len(),
            "scripted envelope at {tick} must be consumed"
        );
        log.record_tick(tick, due, result.state_hash);
        if tick >= 11 {
            let row = paid_steps[(tick - 11) as usize];
            let infantry = sim.substrate.entities.get(3).unwrap();
            let coord = crate::sim::movement::ground_pose::position_world_coord(&infantry.position);
            assert_eq!(
                [coord.x, coord.y, coord.z],
                std::array::from_fn::<_, 3, _>(|i| row["proposed"][i].as_i64().unwrap() as i32),
                "native paid Walk frame {}",
                tick + 1
            );
            assert_eq!(
                u64::from(infantry.body_facing.current(sim.session.binary_frame)),
                row["facing"].as_u64().unwrap()
            );
            assert_eq!(infantry.foot_speed.cached_current_speed, 10);
        }

        if tick >= 10 {
            let tank = sim.substrate.entities.get(1).expect("retasked tank lives");
            let drive = tank.drive_locomotion.as_ref().expect("Drive owner");
            if tick == 10 {
                stopped_head = drive.head_to;
                assert!(
                    stopped_head.is_some(),
                    "Stop must exercise an already committed segment"
                );
            }
            assert!(
                tank.navigation.nav_com.is_none(),
                "Stop clears owner NavCom"
            );
            assert!(
                drive.destination.is_none(),
                "Stop clears the class destination"
            );
            assert!(
                drive.head_to.is_none() || drive.head_to == stopped_head,
                "Stop must not select a new head from abandoned orders"
            );
            assert_eq!(
                tank.navigation.path_replay.cursor as usize,
                tank.navigation.path_replay.directions.len(),
                "Stop exhausts the abandoned direction suffix"
            );
            assert!(
                tank.attack_target.is_none(),
                "Stop retires the previous attack"
            );
        }
    }

    // Unlike the old single-run hash gate, execute every recorded command a
    // second time through ReplayRunner and compare each committed frame.
    let mut replay = Simulation::new();
    replay.spawn_from_map(
        &[
            unit("Americans", "MTNK", 3, 3, EntityCategory::Unit),
            unit("Soviet", "MTNK", 25, 3, EntityCategory::Unit),
            unit("Americans", "E1", 5, 5, EntityCategory::Infantry),
        ],
        Some(&rules),
        &heights,
    );
    let replayed = ReplayRunner::run_fixture_with_overlay_registry(
        &mut replay,
        &log,
        Some(&rules),
        &heights,
        Some(&grid),
        None,
        67,
    );
    assert_eq!(replayed.len(), log.ticks.len());
    for (frame, (actual, recorded)) in replayed.iter().zip(&log.ticks).enumerate() {
        assert_eq!(*actual, recorded.state_hash, "retask replay frame {frame}");
    }
    assert_eq!(
        replay.scenario_rng.logical_state(),
        sim.scenario_rng.logical_state()
    );
    assert_eq!(
        replay.main_rng.logical_state(),
        sim.main_rng.logical_state()
    );
    assert_eq!(
        replay.mapgen_rng.logical_state(),
        sim.mapgen_rng.logical_state()
    );
    for (id, health) in [(1, 300), (2, 300), (3, 125)] {
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .expect("actor lives")
                .health
                .current,
            health,
            "retask window must not turn into a combat/death fixture"
        );
    }
    let live_hash = sim.state_hash();
    assert_eq!(
        [1, 2, 3].map(|id| sim.substrate.entities.get(id).unwrap().mission.ai_counter()),
        [5, 16, 7],
        "Unit/Infantry Commence precedes the Techno counter increment"
    );
    // Original Unit736473 / Infantry51BC51 Commence clears the counter before
    // Techno6FA64E increments it. The two missions promoted during AI therefore
    // have one extra counted visit; actor2 was already in Guard at Unlimbo.
    // Invert only that established correction for the historical hash pins.
    // These are Rust regression receipts, not native goldens; the original
    // first-visit counter1 is pinned separately by the mission_counter corpus.
    // See docs/research/bridge-concrete-ground-damage.md.
    let live_missions = [1, 3].map(|id| (id, sim.substrate.entities.get(id).unwrap().mission));
    for (id, mission) in live_missions {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: mission.current(),
                suspended: mission.suspended(),
                queued: mission.queued(),
                movement_bypass_latch: mission.movement_bypass_latch(),
                handler_state: mission.handler_state(),
                mission_start_frame: mission.mission_start_frame(),
                ai_counter: mission.ai_counter() - 1,
                dispatch_timer: mission.dispatch_timer(),
            });
    }
    let hash = sim.state_hash();
    println!(
        "[slice6] counter-projected={hash:016X} live={live_hash:016X} streams={:016X},{:016X},{:016X}",
        sim.scenario_rng.state(),
        sim.main_rng.state(),
        sim.mapgen_rng.state()
    );
    for id in sim.substrate.entities.keys_sorted() {
        let entity = sim.substrate.entities.get(id).unwrap();
        println!(
            "[slice6 owner] id={id} cell=({},{}) sub=({},{}) health={} mission={:?} queued={:?} nav={:?} path={:?} drive={:?} speed={:?}",
            entity.position.rx,
            entity.position.ry,
            entity.position.sub_x,
            entity.position.sub_y,
            entity.health.current,
            entity.mission.current(),
            entity.mission.queued(),
            entity.navigation.nav_com,
            entity.movement_target.as_ref().map(|target| (
                target.next_index,
                target.path.len(),
                target.final_goal
            )),
            entity.drive_locomotion,
            entity.foot_speed,
        );
    }
    let infantry_facing = sim.substrate.entities.get(3).unwrap().body_facing;
    assert_eq!(infantry_facing.rot_per_frame(), 0x7F00);
    assert_eq!(
        hash, SLICE6_BASELINE_HASH,
        "Slice 6 scripted-retask state hash drifted. Treat this as behavior drift \
         unless a documented native behavior change or hash-composition change \
         is causally demonstrated"
    );
    for (id, mission) in live_missions {
        sim.substrate.entities.get_mut(id).unwrap().mission = mission;
    }
    assert_eq!(sim.state_hash(), live_hash, "restore both live missions");
}

#[test]
fn slice6_move_command_retasks_via_mission_substrate_and_clears_state() {
    // A Move command must route through the compatibility boundary: the mission
    // substrate's `current` becomes Move (checked BEFORE any tick-tail shadow
    // refresh) AND the legacy conflicting fields are cleared.
    let rules = slice6_rules();
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let grid = PathGrid::new(64, 64);
    let mut sim = Simulation::new();
    sim.spawn_from_map(
        &[unit("Americans", "MTNK", 3, 3, EntityCategory::Unit)],
        Some(&rules),
        &heights,
    );
    // Seed a conflicting prior order the Move must tear down.
    {
        let e = sim.substrate.entities.get_mut(1).expect("unit");
        e.attack_target = Some(AttackTarget::new(2));
        e.order_intent = Some(OrderIntent::Guard {
            anchor_rx: 3,
            anchor_ry: 3,
        });
    }

    let issued = sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: 1,
            target_rx: 10,
            target_ry: 10,
            queue: false,
        },
        Some(&rules),
        Some(&grid),
        &heights,
    );
    assert!(issued, "move command should issue");

    let e = sim.substrate.entities.get(1).expect("unit");
    assert_eq!(
        e.mission.queued(),
        MissionId::from_known(MissionType::Move),
        "the command queued Move through the exact authority (host promotes later)"
    );
    assert!(
        e.attack_target.is_none(),
        "Move tore down the attack target"
    );
    assert!(e.order_intent.is_none(), "Move tore down the order intent");
}
