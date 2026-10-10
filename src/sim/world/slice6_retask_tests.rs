//! Slice 6 — verb API + dispatch-adoption integration tests.
//!
//! Two jobs:
//!   1. `replay_hash_stable_through_slice6` — the behavior-preserving gate. A
//!      scripted skirmish drives every retasking command site (Move / Stop /
//!      Attack / ForceAttack / ForceAttackCell / AttackMove) and asserts the
//!      end-of-run `state_hash()` equals the committed baseline. At the slice's
//!      introduction, this exposed wrong aircraft dock teardown subsets and dropped
//!      legacy-field clears. Changes require causal behavior or hash-composition
//!      evidence before updating the Rust regression receipt.
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

fn slice6_rules() -> RuleSet {
    // Two attack-capable vehicles + an infantry; ranges short enough that no
    // auto-combat fires during the scripted window (commands drive everything,
    // while constructor and Walk head-selection draws still use Scenario RNG).
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GACNST\n\n\
         [E1]\nImage=GI\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [GACNST]\nStrength=1000\nArmor=wood\nFoundation=4x3\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    );
    // Explicit authored GI inputs use the production fixed-ART reader and binder;
    // zero-count constructor records do not admit native Ready/idle actions.
    let art = IniFile::from_str(crate::rules::retail_ini_fixture::GI_ART_EXCERPT);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    rules
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
// 2026-09-29 retired ground move phase (snapshot 244, composition only; #726):
// the locomotor's VERA-only `GroundMovePhase` and its stashed twin leave the
// object and piggyback folds in every projection; no schema can rebuild the
// retired value, so this one step re-pins every projection in this test.
// Ceremony: on the parent commit with only those two folds removed, and on
// this change, a soft-assert probe of every pin in this test printed identical
// values, and per-tick replay and the RNG receipts passed at all 16 ticks
// (the probe patch was not committed): the only change to these pins is the
// fold. Old values: the commit that moved them.
// 2026-09-29 one locomotor enum (snapshot 248, composition only; #725):
// LocomotorKind keeps only the eight installable classes, so the active kind,
// the installed slot and the stash fold renumbered discriminants, and the
// dormant Tunnel and DropPod states leave the object and payload folds, in
// every projection. No schema rebuilds the old numbering, so this one step
// re-pins every projection in this test. Ceremony: on the parent commit and on
// this change, a probe printing every object's position, exact Z, health,
// mission, NavCom, attack and movement targets, locomotor kind and layer and
// all three RNG states matched at all 16 ticks (the probe patch was not
// committed). Old values: the commit that moved them.
// 2026-09-30 no cached GetCurrentSpeed (composition only; #844): the
// Foot owner's Rust-only `cached_current_speed` leaves the object fold.
// Ceremony: the parent commit with only that fold removed printed this
// exact value, as this change does, with the RNG pins above unchanged
// (the probe patch was not committed): the only change to this pin is
// the fold. Old value: the commit that moved it.
// 2026-09-30 bridge-response prerequisites (behavior + supplied ART): native
// Guard/Hunt idle runs at mission dispatch, before first stationary Ready.
// Frame0 omits the old global idle draws, changing the later admitted Walk
// head. Explicit GI ART now admits Ready. The paired production-call receipts
// in foot_bridge_layer.replay.json localize both changes; all40 original
// numeric paid-step vectors remain unchanged. Hash the live mission counters,
// whose native first-visit ordering is asserted below, without inverting them.
// 2026-09-30 one locomotor object (snapshot 258, composition only; #680): the
// active locomotor and its piggyback stash hash through one fold of every
// LocomotorState field. The active fold gains BalloonHover, HoverAttack,
// SpeedType, MovementZone, the sub-cell destination and the Hover speed
// request and drops the installed slot, which is the stash's own kind; the
// stash drops its retired separators. Ceremony: the parent commit with only
// that fold changed printed this exact value, as this change does, with the
// RNG pins above unchanged (the probe patch was not committed): the only
// change to this pin is the fold. Old value: the commit that moved it.
// 2026-09-30 unread movement bookkeeping (snapshot 260, composition only;
// #685): the fold drops bridge_occupancy and the ground cell enter order;
// the enter-order counter (and so AirTracker order values) advances only
// for AirTracker entries; Foot+0x68B is write-1-only. Ceremony: the
// parent and this change, each with those five inputs removed from the
// hash, printed the same value, with the RNG pins above unchanged (the
// probe patch was not committed). Old value: the commit that moved it.
// Snapshot264: private native Stage and Infantry Doing/sequence timing.
// Complete before/candidate/final receipts in foot_bridge_layer.replay.json
// (techno_stage264_followup) attribute every changed actor field and retain
// all819 frame rows, all three RNG states and ordered raw draw callers.
// Incoming main preserves candidate state; its source-line changes remain
// recorded. This Rust replay pin does not establish native whole-world parity.
// Incoming-main integration: v24 diagnostics match all main actor fields and
// all three RNG streams at all 17 boundaries except tank 1 MissionCom: Stop
// clears TarCom/NavCom at frame10 without queuing Stop13; Attack retains its
// frame5 timer and ends with11 visits instead of5. Frames0..9 hashes match
// exactly; final changed fields are only current/start/dispatch/counter.
// Native Event IDLE: tools/spatial_oracle/fv_cell_attack/paid_conditional*.json.
// Native Hover host (snapshot 269): the locomotor fold drops the three
// SimFixed Hover copies and hashes the whole HoverLocomotionClass object in
// place of its head. Ceremony: this change with the old composition (three
// zero words, head only; probe not committed) printed the previous pin, so
// no hashed state moved. Previous: 0x38A2_A509_33EC_5219.
// 2026-09-30 one FootClass::Mark owner (#922): Mark no longer writes the
// AircraftTracker, which native Mark never touches, so a ground object keeps
// its constructor-seeded enter order (its stable id) where the old lifecycle
// Mark reset it to 0 on every Foot and Building Mark. Ceremony, rerun after
// merging main: this change with only that reset restored printed the old
// value for all three replay pins (bridge, global, slice 6), with the RNG pins
// above unchanged (the probe patch was not committed). Previous:
// 0x3983_6E86_AD68_DCF0.
// 2026-10-01 Drive/Ship legacy lane removal (behavior, fixture inputs): a
// Drive/Ship visit always runs the native Process, whose Find_Path reads map
// cells, zones and Map Size, so the fixture now supplies a clear 64x64 map
// (`arena_fixture::supply_native_map`). With those inputs the E1 also leaves
// the Walk fixture fallback for native Walk admission. The RNG stream pins,
// mission counters and per-frame replay equality are unchanged. Previous:
// 0x9703_A2A8_E0FF_2C81.
// 2026-10-01 Walk route owner (hash composition only): Walk keeps no
// MovementTarget route cells and no locomotor sub-cell destination, both of
// which the state hash folded. Ceremony: main and this change, each with only
// those fields dropped from the hash (probe not committed), printed the same
// value for all three replay pins (bridge, global, slice 6). Previous: 0xBB09_43C9_DBB1_8884.
// Historical retained-path integration compared all17 main7412/candidate
// observations after omitting newly retained actor fields and RNG source
// locations; gameplay observations matched and only hash composition moved.
// That pre-#962/#963 candidate pin was 0x2A95_6763_E046_638A. The bounded Rust
// receipt remains tools/spatial_oracle/astar_path_finishing_replay/receipt.json.
// Main963 retained-path composition: the same-binary control reproduces
// 0x5BA5_F8CB_B5F7_2FF5 by omitting only navigation history, House threat, Foot530
// and cached Techno508 hash feeds. All17 control/current observations
// match exactly except tick hashes, including full actor state and all
// three RNG streams/draws/caller positions. The temporary gate was removed.
// Rust-only receipt: tools/spatial_oracle/astar_path_finishing_replay/main963/receipt.json.
// 2026-10-01 Stop through the Unit setter (#952): tank 1's Stop at frame 10
// now takes Unit 0x00741970's one-word PathHead clear (Path[0] = -1,
// 741C4F..741C78) and keeps the stale suffix behind it, where the old second
// body exhausted the queue. Both leave no remaining direction; the hash folds
// the queue's cursor and bytes. Previous: 0xCA13_8857_9211_B978.
// 2026-10-01 Every Foot's route has one owner (hash composition only):
// MovementTarget keeps no route cells, layers or cursor. Ceremony: main
// 80b56aa1 with only those fields dropped from the hash (probe not committed)
// printed this value; the bridge and global pins did not move. Previous:
// 0xD06C_DAC2_416D_EDE1.
// 2026-10-02 Building body has one retained owner (hash composition):
// same-binary control restores only the former two absent Up/Down
// folds and reproduces the preceding pin. All819 control/current
// boundaries match after only tick hashes are omitted: full actors,
// commands, lifecycle/Logic and three RNG streams/draws/callers.
// Rust-only receipt: tools/spatial_oracle/building_construction_replay/receipt.json.
// Previous: 0x67C241E9F79C8F43. Native proof is the separate construction corpus.
// 2026-10-02 Engineer/House retained state (Rust-only hash composition):
// The same-binary four-feed control reproduces the preceding main pin.
// All819 complete current/control boundaries match except tick hashes,
// including House/Factory state and three full RNG streams/draws/callers.
// Scope and receipt: tools/spatial_oracle/engineer_repair_replay/receipt.json.
// 2026-10-02 Unit deployment/body ownership: snapshot280 replaces the legacy
// DeployPhase hash with Techno130/134, adds Unit6E0, and advances the existing
// Foot538 counter for voxel Units. Mission, health and all three RNG pins
// remain unchanged. The same-binary hash control reproduces incoming
// 1AD1402910CE62D8; all 17 off/on boundaries differ only in tick hash. Receipt:
// tools/spatial_oracle/unit_simple_deploy_replay/receipt.json. This is Rust
// attribution; unit_simple_deploy separately pins native body cadence.
// Shared Techno Door hash composition after main339b57d18 integration:
// one diagnostic binary restores incoming B20E11E0579BC4E4 when only the Door
// hash feed is omitted. Behavior and RNG assertions reach the final pin in
// both modes. The temporary control is removed; native expected values stay
// unchanged. Receipt: tools/spatial_oracle/anytown_damage/unit_unlimbo.md.
// 2026-10-02 IFV owner migration: u8 last-shot hashing becomes signed i32
// current-weapon hashing and the redundant None override feed is removed.
// All 17 recorded boundaries retain gameplay/RNG after guarded field migration;
// tools/spatial_oracle/ifv_turret_replay/receipt.json records this Rust attribution.
// Shared Door composition after main1009: same test binary, 17 complete
// off/on boundaries equal except tick_result.state_hash. Omitting only Door
// restores incoming main; the temporary control is removed. Bounded Rust
// attribution: tools/spatial_oracle/anytown_damage/unit_unlimbo.md.
// Snapshot286: the entity-level absent Teleport tag is removed; the complete
//class payload owns its fold. Restoring only that old0 tag recovers the prior
//hash exactly, with all existing pose/gameplay and absolute RNG tripwires intact.
// See tools/spatial_oracle/infantry_teleport_destination.md. Rust hash ratchet only.
// Snapshot288: the complete Drive/Ship payload owns its retained Option fold.
// Same-binary old-feed control recovers43BC438F280E7D17; all17 complete rows
// match except state_hash, including all3 RNG states. Control removed.
// Rust attribution: tools/spatial_oracle/drive_instance_replay/other/slice6/receipt.json.
// Barracks output after main7e9: shared TechnoUnlimbo6F6DAA retains the
// constructor facing Some(0); private Foot6B3/Factory5D feeds enter the hash.
// The Drive/Ship payload migration remains. All618 same-binary Rust rows
// match except tick_result.state_hash; the control recovers incoming pins.
// tools/spatial_oracle/factory_infantry_output_replay/main7e9/receipt.json.
// Rocket locomotor port: the entity-level absent rocket_state tag is removed;
// the Rocket payload owns its fold. Restoring only that old 0 tag recovers
// incoming 0xF008D6D1349CCD9C in the same test binary. Control removed.
// Snapshot303: HoverAttack, target_pad and pad_index leave the hash. A control
// binary (main with a hash that skips only those fields) gives this value in
// the same test, with every tripwire above green. Control removed.
// Snapshot306: the legacy CellClass counters, flags, occlusion caches and
// visibility marks leave the hash (the ground bits stay), and SightAdmission
// drops fog_of_war. A control binary (main with a hash that skips only those
// fields) gives this value in the same test, with every tripwire above green.
// Control removed.
// Snapshot307: the session's LocalSize copy leaves the hash. A control binary
// (main with a hash that skips only that tuple) gives this value in the same
// test, with every tripwire above green. Control removed.
// Snapshot318 hashes MoveSound's signed i32 countdown instead of u8. The
// fixture's vectors are empty, but qualifying Foot visits retain countdown3
// while inactive (4DAA87; foot_move_sound.json empty_qualifies). No sound is
// selected and no Main draw is paid. This remains a Rust regression pin.
// Snapshot320 retains native Techno constructor disguise identity/timer state
// for every actor, replacing the absent component. These fixtures acquire no
// disguise; their gameplay and absolute RNG pins remain the independent gates.
const SLICE6_BASELINE_HASH: u64 = 0x8C85_319B_7E81_7B48;

#[test]
fn replay_hash_stable_through_slice6() {
    let rules = slice6_rules();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let grid = (*sim.path_grid_snapshot().unwrap()).clone();
    // id 1: Americans MTNK (the unit we retask). id 2: enemy MTNK (Soviet, hostile
    // by default — no alliance entry). id 3: Americans E1 (second attacker).
    let mut diagnostic = super::global_parity_harness_tests::replay_diagnostic_file("slice6");
    let mut seed = || {
        sim.spawn_from_map(
            &[
                unit("Americans", "MTNK", 3, 3, EntityCategory::Unit),
                unit("Soviet", "MTNK", 25, 3, EntityCategory::Unit),
                unit("Americans", "E1", 5, 5, EntityCategory::Infantry),
            ],
            Some(&rules),
        )
    };
    let (_, seed_draws) = if diagnostic.is_some() {
        crate::sim::rng::trace_draws(seed)
    } else {
        (seed(), Vec::new())
    };
    super::global_parity_harness_tests::record_replay_diagnostic(
        &mut diagnostic,
        &sim,
        None,
        &[],
        &seed_draws,
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
    // Numeric paid-step parity is pinned at its actual post-admission
    // boundary by walk_step::tests::paid_walk_matches_original_numeric_facing_and_boundary_vectors.
    // Its historical Slice6 rows supply head1728,1216/facing10855; they do not
    // establish this replay's live head choice after mission/idle RNG changes.
    let mut paid_head = None;
    let mut paid_coord = None;
    for tick in 0..16u64 {
        let due: Vec<CommandEnvelope> = script
            .iter()
            .filter(|(t, _)| *t == tick + 1)
            .map(|(t, c)| cmd_envelope(&sim, "Americans", *t, c.clone()))
            .collect();
        let mut advance = || sim.advance_tick(&due, Some(&rules), Some(&grid), None, 67);
        let (result, draws) = if diagnostic.is_some() {
            crate::sim::rng::trace_draws(advance)
        } else {
            (advance(), Vec::new())
        };
        super::global_parity_harness_tests::record_replay_diagnostic(
            &mut diagnostic,
            &sim,
            Some(&result),
            &due,
            &draws,
        );
        assert!(result.frame_committed, "retask frame {tick} must commit");
        assert_eq!(
            result.executed_commands,
            due.len(),
            "scripted envelope at {tick} must be consumed"
        );
        log.record_tick(tick, due, result.state_hash);
        if tick >= 11 {
            let infantry = sim.substrate.entities.get(3).unwrap();
            let coord = crate::sim::movement::ground_pose::position_world_coord(&infantry.position);
            let head = infantry.locomotor.as_ref().unwrap().step_head().unwrap();
            if let Some(first) = paid_head {
                assert_eq!(head, first, "a paid Walk retains its admitted head");
            } else {
                paid_head = Some(head);
            }
            if let Some(previous) = paid_coord {
                assert_ne!(coord, previous, "each paid visit moves toward its head");
                let distance = |position: crate::sim::components::DriveCoord| {
                    let dx = i64::from(position.x) - i64::from(head.x);
                    let dy = i64::from(position.y) - i64::from(head.y);
                    dx * dx + dy * dy
                };
                assert!(distance(coord) < distance(previous));
            }
            paid_coord = Some(coord);
            assert_eq!(sim.current_speed_for_test(3, &rules), 10);
        }

        if tick >= 10 {
            let tank = sim.substrate.entities.get(1).expect("retasked tank lives");
            assert_eq!(
                tank.mission.current(),
                MissionId::from_known(MissionType::Attack)
            );
            assert_eq!(tank.mission.queued(), MissionId::NONE);
            assert_eq!(tank.mission.mission_start_frame(), 5);
            assert_eq!(tank.mission.ai_counter(), tick as u32 - 4);
            assert_eq!(
                (
                    tank.mission.dispatch_timer().start_frame(),
                    tank.mission.dispatch_timer().delay()
                ),
                (5, 14),
                "Stop retains Attack's dispatch timer and counter"
            );
            let drive = tank
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .expect("Drive owner");
            if tick == 10 {
                stopped_head = drive.head_to();
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
                drive.destination().is_none(),
                "Stop clears the class destination"
            );
            assert!(
                drive.head_to().is_none() || drive.head_to() == stopped_head,
                "Stop must not select a new head from abandoned orders"
            );
            // The Unit setter's one-word PathHead clear (Path[0] = -1,
            // 741C4F..741C78): the stale suffix behind it is never read.
            assert!(
                tank.navigation
                    .path_replay
                    .remaining_directions()
                    .is_empty(),
                "Stop clears the live path head"
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
    crate::sim::arena_fixture::supply_native_map(&mut replay);
    replay.spawn_from_map(
        &[
            unit("Americans", "MTNK", 3, 3, EntityCategory::Unit),
            unit("Soviet", "MTNK", 25, 3, EntityCategory::Unit),
            unit("Americans", "E1", 5, 5, EntityCategory::Infantry),
        ],
        Some(&rules),
    );
    let replayed = ReplayRunner::run_fixture_with_overlay_registry(
        &mut replay,
        &log,
        Some(&rules),
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
    super::global_parity_harness_tests::print_replay_summary("slice6", &sim);
    assert_eq!(
        [1, 2, 3].map(|id| sim.substrate.entities.get(id).unwrap().mission.ai_counter()),
        [11, 16, 7],
        "Stop retains Attack; Unit/Infantry Commence precedes the Techno counter increment"
    );
    // Original Unit736473 / Infantry51BC51 Commence clears C4 before
    // Techno6FA64E increments it; the mission_counter corpus pins that owner.
    assert_eq!(
        (
            sim.scenario_rng.state(),
            sim.main_rng.state(),
            sim.mapgen_rng.state()
        ),
        (
            0x650C_CCE5_32C8_9F6E,
            0x4CB6_FE1C_CB45_47FF,
            0x1CE8_1848_7043_6163
        ),
        "absolute stream pins supplement per-frame replay equality"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .body_facing
            .rot_per_frame(),
        0x7F00
    );
    assert_eq!(
        sim.state_hash(),
        SLICE6_BASELINE_HASH,
        "scripted-retask state drifted: establish the changed behavior, RNG producer, or hash composition before updating"
    );
}

#[test]
fn slice6_move_command_retasks_via_mission_substrate_and_clears_state() {
    // A Move command must route through the compatibility boundary: the mission
    // substrate's `current` becomes Move (checked BEFORE any tick-tail shadow
    // refresh) AND the legacy conflicting fields are cleared.
    let rules = slice6_rules();
    let grid = PathGrid::new(64, 64);
    let mut sim = Simulation::new();
    sim.install_fixture_path_grid(Some(&grid));
    sim.spawn_from_map(
        &[unit("Americans", "MTNK", 3, 3, EntityCategory::Unit)],
        Some(&rules),
    );
    // Seed a conflicting prior order the Move must tear down.
    {
        let e = sim.substrate.entities.get_mut(1).expect("unit");
        e.attack_target = Some(AttackTarget::new(2));
        e.order_intent = Some(OrderIntent::AttackMove {
            goal_rx: 3,
            goal_ry: 3,
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
