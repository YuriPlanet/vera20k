//! Slice 8 — global lockstep parity harness.
//!
//! Records a deterministic multi-faction skirmish as a `ReplayLog` and re-runs it
//! through the same registry-aware `ReplayRunner` master-frame path the live
//! game uses, asserting (1)
//! every tick's replayed hash equals the recorded hash (intra-run determinism)
//! and (2) the final hash equals a committed baseline. This is the project-wide
//! desync tripwire for the whole mission/radio substrate migration.
//!
//! Coverage: two hostile houses; an Allied war factory + refinery + harvester
//! over a seeded ore patch (the harvester gets a `Miner` component at spawn,
//! reaches the patch and cuts — that state folds into the hash);
//! tanks + infantry under scripted Move/AttackMove/Stop, with the two sides
//! closing to combat range (exercises mission retask, movement, targeting/
//! retaliation, and the RNG streams). The harvester carries the real
//! `Harvester`/`Dock`/`Storage` flags and the refinery `Refinery=yes`.
//!
//! Scope note: this is a determinism + baseline guard, not a miner-dock test.
//! Driving a harvester physically to ore and through the full refinery dock
//! handshake needs movement world-setup (resolved terrain, navigation) that
//! the dedicated miner-dock suite (`miner_tests.rs`) provides and owns; this
//! harness only guards that the miner system stays wired and deterministic.
//!
//! The `tools/spatial_oracle/*_replay/` and `foot_bridge_layer*.replay.json`
//! receipts cited below left the tree; `git show 1b1a8b43e:<path>` retrieves them.

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use std::io::Write;

/// Optional observations of the two bounded replay fixtures, never golden input.
/// The file records existing owners without changing or draining their state.
pub(super) fn replay_diagnostic_file(name: &str) -> Option<std::io::BufWriter<std::fs::File>> {
    let root = std::path::PathBuf::from(std::env::var_os("VERA20K_REPLAY_DIAGNOSTICS")?);
    std::fs::create_dir_all(&root).unwrap();
    Some(std::io::BufWriter::new(
        std::fs::File::create(root.join(format!("{name}.jsonl"))).unwrap(),
    ))
}

pub(super) fn record_replay_diagnostic(
    writer: &mut Option<std::io::BufWriter<std::fs::File>>,
    sim: &Simulation,
    result: Option<&TickResult>,
    commands: &[CommandEnvelope],
    draws: &[serde_json::Value],
) {
    let Some(writer) = writer else { return };
    let actors: Vec<_> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .map(|id| sim.substrate.entities.get(id).unwrap())
        .collect();
    let fires: Vec<_> = sim
        .fire_events
        .iter()
        .map(|fire| {
            serde_json::json!({
                "attacker":fire.attacker_id, "target":fire.target,
                "weapon":sim.resolve(fire.weapon_id),
                "position":[fire.fire_coord.x,fire.fire_coord.y,fire.fire_coord.z],
            })
        })
        .collect();
    let retained_path_inputs = serde_json::json!({
        "navigation_present":sim.zone_grid.is_some(),
        "houses":sim.houses.iter().map(|(owner,house)| serde_json::json!({
            "owner":sim.resolve(*owner),
            "spatial_grid_all_zero":house.spatial_threat_values().iter().all(|&value|value==0),
        })).collect::<Vec<_>>(),
        "actors":actors.iter().map(|actor| serde_json::json!({
            "id":actor.stable_id(),
            "coefficient_bits":actor.navigation.path_threat_coefficient().bits(),
            "cached_contribution":actor.cached_spatial_threat(),
            "team_override":sim.team_script_vm.member_avoids_threats(actor.stable_id()),
        })).collect::<Vec<_>>(),
        "team_count":sim.team_script_vm.teams_in_order().count(),
        // House/Factory receipts observe shared consumers through their existing
        // owners, including private power state; they never mutate the replay.
        "engineer_house_attribution":{
            "house_order":sim.session.house_order.iter().map(|owner|sim.resolve(*owner)).collect::<Vec<_>>(),
            "current_house":sim.session.current_house.map(|owner|sim.resolve(owner)),
            "houses":sim.houses.iter().map(|(owner,house)|serde_json::json!({
                "owner":sim.resolve(*owner),"stats":house.stats,"economy":house.economy,
                "capture_notified":house.building_capture_notified(),
                "discovered_by_current_house":house.discovered_by_current_house(),
                "building_order":house.base_projection.buildings(),
                "eva_funds_timer":house.eva_funds_timer,"eva_low_power_guard":house.eva_low_power_guard,
                "repair_delay_bits":house.repair_delay().to_bits(),
                "repair_start_latch":house.repair_start_latch,"repair_latch_timer":house.repair_latch_timer,
            })).collect::<Vec<_>>(),
            "power_states":sim.power_states.iter().map(|(owner,state)|serde_json::json!({
                "owner":sim.resolve(*owner),"state":state,
            })).collect::<Vec<_>>(),
            "factory_count":sim.production.factories.len(),
            "factories":sim.production.factories.holders_insertion_ordered().into_iter()
                .map(|(holder,factory)|serde_json::json!({"holder":holder,"factory":factory})).collect::<Vec<_>>(),
        },
        // Only the global fixture kills a Unit. Preserve the shared cleanup
        // owners around that death without inventing or draining any state.
        "lifecycle_hash_attribution":{
            "raw_occupation":format!("{:?}",sim.substrate.raw_cell_occupation),
            "shared_dummy":format!("{:?}",sim.effective_shared_cell_dummy().snapshot()),
            "shared_dummy_raw_occupation":sim.substrate.raw_cell_occupation.dummy_for_hash(),
            "houses":sim.houses.iter().map(|(owner,house)|serde_json::json!({
                "owner":sim.resolve(*owner),"retained":house,
            })).collect::<Vec<_>>(),
        },
    });
    serde_json::to_writer(
        &mut *writer,
        &serde_json::json!({
            "next_frame":sim.session.binary_frame,
            "tick_result":result.map(|r| serde_json::json!({"tick":r.tick,
                "committed":r.frame_committed,"executed_commands":r.executed_commands,
                "state_hash":r.state_hash,"terminal_score_finalized":r.terminal_score_finalized})),
            "commands":commands,"entities":actors,"logic_order":sim.logic_order(),
            "fire_events_accumulated":fires,
            "lifecycle_outputs_accumulated":format!("{:?}",sim.lifecycle_outputs),
            "rng":{"scenario":sim.scenario_rng.native_state_hex(),
                "main":sim.main_rng.native_state_hex(),"mapgen":sim.mapgen_rng.native_state_hex()},
            "draws":draws,"retained_path_inputs":retained_path_inputs,
        }),
    )
    .unwrap();
    writeln!(writer).unwrap();
    writer.flush().unwrap();
}

pub(super) fn print_replay_summary(name: &str, sim: &Simulation) {
    if std::env::var_os("VERA20K_REPLAY_DIAGNOSTICS").is_none() {
        return;
    }
    let actors: Vec<_> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .map(|id| {
            let e = sim.substrate.entities.get(id).unwrap();
            serde_json::json!({"id":id,"type":sim.resolve(e.type_ref()),
            "health":e.health.current(),"position":e.position,"mission":e.mission,
            "target":e.attack_target,"nav":e.navigation.nav_com(),
            "passive_timer":e.passive_scan.timer(),"last_scan":e.passive_scan.last_scan_frame(),
            "rearm":e.rearm_timer.timer()})
        })
        .collect();
    println!(
        "[replay observation {name}] {}",
        serde_json::json!({
            "next_frame":sim.session.binary_frame,"hash":sim.state_hash(),
            "streams":[sim.scenario_rng.state(),sim.main_rng.state(),sim.mapgen_rng.state()],
            "actors":actors,"fire_count":sim.fire_events.len(),
            "hash_owners":sim.hash_owner_diagnostics_for_test(),
        })
    );
}

/// Same-binary Rust hash attribution, independent of gameplay/native goldens.
/// Only the removed TriggerRuntime last_announcement=None delimiter is fed.
pub(super) fn assert_announcement_feed_retirement_hash_control(
    name: &str,
    sim: &Simulation,
    expected_state_hasher_pin: u64,
) {
    let current = sim.state_hash();
    let retained_authority = bincode::serialize(sim).expect("retained replay authority");
    let retained_dummy = sim.effective_shared_cell_dummy().snapshot();
    let retained_streams = (
        sim.scenario_rng.state(),
        sim.main_rng.state(),
        sim.mapgen_rng.state(),
    );
    let control =
        crate::sim::trigger_runtime::with_legacy_announcement_hash_control_for_test(|| {
            sim.state_hash()
        });
    println!(
        "[announcement hash attribution {name}] current={current:016X} control={control:016X}"
    );
    assert_eq!(
        control, expected_state_hasher_pin,
        "restoring only the retired None feed must recover the main StateHasher pin"
    );
    assert_eq!(sim.state_hash(), current, "the diagnostic feed is scoped");
    assert_eq!(
        bincode::serialize(sim).expect("unchanged replay authority"),
        retained_authority,
        "diagnostic feeds cannot alter serialized gameplay state"
    );
    assert_eq!(sim.effective_shared_cell_dummy().snapshot(), retained_dummy);
    assert_eq!(
        (
            sim.scenario_rng.state(),
            sim.main_rng.state(),
            sim.mapgen_rng.state()
        ),
        retained_streams
    );
}

const HARNESS_SEED: u64 = 0xC0FFEE1234;
const HARNESS_TICKS: u64 = 600;
const HARNESS_TICK_MS: u32 = 67;
const HARNESS_MAP_SIZE: u16 = 64;
const HARNESS_CELL_SIDE: u16 = 2 * HARNESS_MAP_SIZE + 1;
const HARNESS_COORD_SHIFT: u16 = HARNESS_MAP_SIZE / 2;
/// AT-8: ticks at which the per-stream RNG cursors are compared record-vs-replay
/// (after the tick at this index executes).
const STREAM_CHECKPOINT_TICKS: &[u64] = &[149, 299, 449, 599];

/// Rust regression receipts, not native whole-skirmish goldens. Record and
/// replay agree on all 600 frames and the three-stream checkpoints. Against
/// main at c6d6cd7e0 the first difference is tank 4's attack-move order at
/// frame 40: main queued raw mission 29 and committed it at 41, while
/// Assign_Target_Command answers Move, which Queue_Mission's same-mission guard
/// (`0x005B3601..0x005B3612`) leaves as the committed Move. From frame 46 its
/// targeting timer runs Arrival_Target_Handler, whose scan draws Scenario
/// `RandomRanged(0, 2)` each time; main/MapGen are unchanged. The scan finds
/// the infantryman at frame 297 (main's per-visit scan found tank 6 at 281),
/// and Event IDLE (`4C74CB..4C76BB`) after 299 keeps its committed Attack and
/// its order. The 15-shot fight leaves tank 6 at 108 HP; target expiry 556,
/// Guard Commence 568 and the first passive scan 569 pass the transition
/// checks below. Original conditional native Stop/FV evidence and
/// reproduction entry points:
/// `tools/spatial_oracle/fv_cell_attack/README.md`. These fixture observations
/// refresh a Rust regression pin; they do not establish native world parity.
const FINAL_STREAM_STATES: (u64, u64, u64) = (
    // MERGE 2026-08-03: both branches re-baselined these independently (dev:
    // passive acquire + spawner; foundations: Move cadence + hashed runtime
    // state). Neither side's values describe the merged tree; re-derived below
    // from the merged tree's own output in the same merge commit.
    // 2026-09-25 combat chain 1: Mission_Guard cadence draws from frame 0 and
    // the tank duel from tick 281 (see GLOBAL_HARNESS_FINAL_HASH).
    // 2026-09-25 combat chain 2: vehicle Guard cadence draws from frame 0.
    // 2026-09-25 ore-field chain: the fixture's playfield and the harvester's
    // native ore field (see GLOBAL_HARNESS_FINAL_HASH).
    // 2026-09-25 ore-field review: the fixture's map cells (same place).
    // 2026-09-30 native mission-site idle/initial Ready plus explicit GI ART:
    // frame0 omits four old global idle-tail draws; first bound ART idle at
    // frame14 precedes Guard cadence. Saved production-call receipts are in
    // foot_bridge_layer.replay.json. Full Main/MapGen are unchanged.
    // 2026-10-01 turretless hull turn (#690): tank 4's FACING turn waits for
    // its Drive to stop, so its later shots and their Scenario draws move
    // (see GLOBAL_HARNESS_FINAL_HASH). Main/MapGen are unchanged.
    // 2026-10-01 Stop through the Unit setter (#952): see
    // GLOBAL_HARNESS_FINAL_HASH. Main/MapGen are unchanged.
    // 2026-10-13 native attack-move (Foot MegaMission): tank 4 scans only on
    // its targeting timer, so its fight and its draws start at frame 297
    // (see GLOBAL_HARNESS_FINAL_HASH). Main/MapGen are unchanged.
    0x5F1A_988D_BB0A_157F,
    0x39F3_258B_A550_EB7C,
    0x1CE8_1848_7043_6163,
);

// 2026-09-23 Drive/Ship Process path request (behavior, not composition):
// Drive/Ship orders from every producer (player, Approach, rally, miners) are
// accepted by the Unit setter without an order-time A*, PowerOn or redirect.
// This fixture has no native zone topology, so the first Process searches in
// the legacy inline lane, not the Find_Path owner. The pins move with that
// state; the RNG stream pins, per-tick replay equality and route tripwires in
// this file are unchanged. The old values are in the commit that moved them.
// 2026-09-23 Drive/Ship same-call track-end continuation (behavior): first
// divergence from main is tick 30, tank 4's first track end, now continuing
// into its next track in that Process (residual-only Process_Track(1)). The
// scenario and main RNG streams match main over all 600 ticks. The
// retained-destination check expects a live track instead of the removed
// next-frame deferral. Old values: the commit that moved them.
// Schema202 folds the object's rearm timer (TechnoClass+0x2EC, started at the
// construction frame as the constructor does) in place of the AttackTarget
// cooldown/burst-delay counters and the cloak copy: composition only.
// Before(202) reproduces the v201 pin; per-tick replay, the RNG stream pins and
// the route tripwires are unchanged.
// 2026-09-25 combat chain 1 (behavior, snapshot 204), traced tick by tick
// against 947c7044: from frame 0 three objects on Guard run Mission_Guard and
// draw its RandomRanged(0, 2) cadence, and the infantry idle fidgets move with
// them (Scenario stream only). Cells and health match until tick 281, when
// tank 4's attack-move, which acquired tank 6 there before too, now fires on
// it. 947c7044 never fired and at tick 292 swapped to infantryman 7; its fire
// path carried a shroud gate and invented retargets, neither of which native
// has, and this chain deletes both. Tank 6 retaliates and the duel ends with tank
// 4 dead at tick 590. The main and mapgen streams are unchanged. Schema 204
// adds the house ROF bias and bullet OnBridge folds. Old values: the commit
// that moved them.
// 2026-09-25 combat chain 2 (behavior, snapshot 205), traced against d02452cd:
// the vehicles take their idle mission on Unlimbo (UnitClass::Enter_Idle_Mode
// 0x00738970), so their Mission_Guard cadence draws from frame 0 and every
// vehicle move starts one frame earlier. The duel shifts by a tick (tank 4
// dies at 588 instead of 587; tank 6 still ends at 12 HP); main/mapgen streams
// unchanged. With that hook disabled every old pin reproduces, so the other
// chain-2 mechanisms (impact ladder, Middle, debris, veterancy) leave this
// fixture untouched: it binds no anim art. Old values: the moving commit.
// Schema206 drops the retired Chrono dock folds (home refinery, dock-queued
// byte, dock phase, pivot facing) from the harvester's miner block and tags
// Techno+0x1F8, which no object here raises: composition only. Before(206)
// reproduces the v205 pin; the three RNG stream pins, per-tick replay and the
// miner-engagement tripwire are unchanged.
// 2026-09-25 ore-field chain (behavior, snapshot 207), two causes:
// 1. The fixture now installs the map's playfield, as map load does: the
//    native ore scan (`Is_Cell_Harvestable`) admits only playfield cells.
//    On origin/main 474a71ca with only that change the harvester cuts 20
//    bales by tick 599 and the duel ends with tank 4 alive at 12 HP and tank
//    6 at 12 HP (final 0xFE28_0F62_91CC_4516; Scenario stream
//    0x59F4_735D_FB94_7D28; main and mapgen unchanged).
// 2. Mission_Harvest states 0/1 run natively: the harvester reaches its
//    field, cuts one bale per StageClass gate and hops without a fresh wait,
//    21 bales by tick 599; its Rate epilogue draws and a mined-out cell's
//    spread-queue draws move the Scenario stream only. The duel is as in 1.
// Schema 207 drops the retired target-cell and harvest-timer folds and adds
// the StageClass and Unit+0x6D1/+0x6D2; Before(207) pins that projection.
// Every older projection moves with the behavior. Old values: the moving commit.
// 2026-09-25 ore-field review (fixture): the scenario stands on flat map
// cells with its playfield installed before placement and a [Tiberium] land
// row, as a map load leaves them. Unlimbo's Unit Can_Enter_Cell and the
// harvester's ore scan (`Is_Cell_Harvestable` through the native-compared
// `foot_can_enter`) read them; without map cells no ore cell is harvestable.
// The harvester still reaches its field and cuts 21 bales by tick 599; the
// duel's timing shifts and tank 4 takes its seventh hit and dies at tick
// 586..590 (tank 6 ends at 12 HP). Only the Scenario stream moves. Every
// projection moves with the scenario. Old values: the moving commit.
// 2026-09-25 combat chain 3 (snapshot 208): schema 208 folds every object's
// crash latch and its AI edge (Foot+0x425/+0x426) and a Fly's fall counter,
// composition only (nothing here flies or crashes): Before(208) reproduces the
// ore-field pin, and the RNG stream pins, per-tick replay and every older
// projection are unchanged.
// 2026-09-26 retired weapon identity (snapshot 212, composition only): the
// entity hash no longer folds `current_weapon_ref`, the weapon id of the last
// live selection, which has had no reader since combat chain 3 took the death
// weapon from GetCurrentWeapon. No schema can rebuild that per-fire value, so
// this one step re-pins every projection in this test, before_power included.
// Ceremony: with only that fold line deleted from the previous tree, these
// pins fail and nothing else in the lib suite moves; their `left` values are
// pasted here, and the full retirement (field, constructor, selection write
// and its interning, snapshot 212) reproduces every one. FINAL_STREAM_STATES,
// POSITION_FINGERPRINT, per-tick record/replay equality and the duel's
// outcome are unchanged: the only change to these pins is the removed fold.
// Old values: the commit that moved them.
// 2026-09-26 building sale (snapshot 213, composition only): schema 213 folds
// each building's AI sale byte (`BuildingClass+0x6DC`). Before(213)
// reproduces the retired-weapon pin; FINAL_STREAM_STATES, per-tick replay and
// every older projection are unchanged.
// 2026-09-26 building repair (snapshot 216, composition only): schema 216
// folds each building's repair byte (`BuildingClass+0x6E8`) and AI repair
// byte (`+0x6CB`) and each house's repair delay (`HouseClass+0x1C0`),
// auto-repair latch (`+0x245`) and its timer (`+0x280`). Before(216)
// reproduces the building-sale pin; FINAL_STREAM_STATES, per-tick replay and
// every older projection are unchanged, so the chain moved no behavior here.
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
// 2026-09-27 combat chain 11 (behavior, snapshot 221), traced tick by tick
// against 11c2e2e9: the war factory and refinery (ids 1, 2) now queue Guard
// at Unlimbo (0x0044D6A0) and commence it on their first Update; unarmed and
// with HasStupidGuardMode's constructor true, each Guard dispatch returns
// 100 frames and draws nothing. From tick 0 their MissionCom holds Guard and
// a 100-frame dispatch timer (last armed at frame 500) where 11c2e2e9 held no
// mission and a zero timer. Every other object's full state, the logic order
// and all three RNG streams match 11c2e2e9 at all 600 ticks, and resetting
// just those two fields of the two buildings at tick 599 reproduces every
// old pin, Schema220's current pin and before_power included. Old values:
// the commit that moved them.
// 2026-09-27 combat chain 15 (snapshot 230, composition only): schema 230
// folds every building's `+0x388` FacingClass (BuildingClass::Init's Set_ROT,
// `0x00442CA5`); the war factory and refinery here are turretless, so they
// carried none before. Before(230) reproduces the chain-11 pin, and the three
// RNG stream pins, per-tick replay and every older projection are unchanged:
// neither building is armed, so ReceiveDamage's retaliation block draws and
// targets nothing here. Old values: the commit that moved them.
// 2026-09-28 one body facing (snapshot 237, composition only; #580): each
// object's hash folds its body FacingClass (`+0x388`) in place of the retired
// 8-bit facing mirror, turn target and optional turn interpolator, and a
// building's `+0x388` moves from the turret slot into it, in every projection.
// No schema can rebuild the mirror or the target, so this one step re-pins
// every projection in this test. Schema 230's building-facing gate went with
// them: its projection now equals the current hash, and its pin is gone.
// Ceremony: on origin/main 99935d1d and on this change, a probe hash folding no
// facing, and one folding only each object's body heading word at the hashed
// frame (the old tree's interpolator, else its mirror), matched at all 600
// ticks, RNG streams included (the probe patch was not committed): every other
// fold and every object's heading are unchanged, so the only change to these
// pins is the fold. Drive's never-written turn target (`DriveTurnState`) left
// the fold in the same step: with its four default fields folded back in their
// old place, this change reproduced every facing-only pin (final
// 0x6158_8E4E_3576_4D36), RNG streams included. Old values: the commit that
// moved them.
// 2026-09-28 houses in the fixture (#824): a harvester's Dock checks read its
// house's tracked BuildingType counts (`+0x5500`), so `seed_scenario` now makes
// the two owners' houses, as computer houses in no house order, before placing
// objects. Every projection folds the houses, so this one step re-pins every
// projection in this test, schema 239's too. Ceremony: on origin/main 58605c6e
// and on this change, a probe printing every object's mission, queued mission,
// NavCom, health, position and attack target and the RNG state matched at all
// 600 ticks (the probe patch was not committed), and `FINAL_STREAM_STATES`
// holds: the only change is the houses' state in the hash. A human house would
// change behaviour (its AttackMove tank keeps moving when hit), and a `[Map]
// Size=` would clip the threat scan to its diamond; neither is in this step.
// Old values: the commit that moved them.
// 2026-09-29 retired ground move phase (snapshot 244, composition only; #726):
// the locomotor's VERA-only `GroundMovePhase` and its stashed twin leave the
// object and piggyback folds in every projection; no schema can rebuild the
// retired value, so this one step re-pins every projection in this test.
// Ceremony: on the parent commit with only those two folds removed, and on
// this change, a soft-assert probe of every pin in this test printed identical
// values, and per-tick replay and the RNG receipts passed at all 600 ticks
// (the probe patch was not committed): the only change to these pins is the
// fold. Old values: the commit that moved them.
// 2026-09-29 retired purifier count (snapshot 245, composition only; #705):
// each house's retained OrePurifier count, which only the hash read, leaves the
// house fold in every projection (none folds it back), so this one step
// re-pins every projection it moved. Ceremony: on the parent
// commit with only that fold removed, and on this change, a soft-assert probe
// of every pin in this test printed identical values, and per-tick replay and
// the RNG receipts passed at all 600 ticks (the probe patch was not committed):
// the only change to these pins is the fold. Old values: the commit that moved
// them.
// 2026-09-29 one wall-plane mode (snapshot 247, composition only; #717):
// `OverlayGrid::new` now retains an all-zero wall plane like every production
// grid, so this fixture's grid folds the plane's tag, length and bytes where
// it folded the retired plane-less tag. No schema folds the retired mode, so
// this one step re-pins every projection it moved. Ceremony: on the parent
// commit and on this change, a probe printing every object's position, exact
// Z, health, mission, NavCom, attack and movement targets and all three RNG
// states matched at all 600 ticks (the probe patch was not committed): the
// fixture has no walls, and its Foot neighbour sources come from the
// lifecycle writes instead of positions with the same result. Old values: the
// commit that moved them.
// 2026-09-29 one locomotor enum (snapshot 248, composition only; #725):
// LocomotorKind keeps only the eight installable classes, so the active kind,
// the installed slot and the stash fold renumbered discriminants, and the
// dormant Tunnel and DropPod states leave the object and payload folds, in
// every projection. No schema rebuilds the old numbering, so this one step
// re-pins every projection in this test. Ceremony: on the parent commit and on
// this change, a probe printing every object's position, exact Z, health,
// mission, NavCom, attack and movement targets, locomotor kind and layer and
// all three RNG states matched at all 600 ticks (the probe patch was not
// committed). Old values: the commit that moved them.
// 2026-09-29 garrison Unload chain (hashed state, not behavior): an idle
// turretless building's UpdateAnimation now sets its ready byte +0x6DD every
// frame (0x00451218), which the building leaf hash folds. With only that write
// disabled the old pin reproduces exactly; the three RNG stream pins, per-tick
// replay equality and every object's final mission are unchanged.
// 2026-09-29 bridge query owner migration: the ore probe requires native Size
// and zone authority, so this fixture now uses Size64x64, Local2,2,60,56, an
// empty bridge-map receipt and a common +32,+32 coordinate translation. Its
// former width-zero/absent-zone setup exercised a legacy movement lane. Under
// this same completed fixture, parent ore/zone query code and the new shared
// queries produced identical world hashes, serialized objects, all three RNG
// fingerprints and 319 raw draws at all 600 ticks. Absolute stream pins and duel
// outcomes remain unchanged. This hash move is fixture/context coverage, not a
// native skirmish golden. Receipts: tools/spatial_oracle/foot_bridge_layer.replay.json.
// 2026-09-30 no cached GetCurrentSpeed (composition only; #844): the
// Foot owner's Rust-only `cached_current_speed` leaves the object fold.
// Ceremony: the parent commit with only that fold removed printed this
// exact value, as this change does, with the RNG pins above unchanged
// (the probe patch was not committed): the only change to this pin is
// the fold. Old value: the commit that moved it.
// 2026-09-30: native class target/destination and Foot mission/idle owners,
// retained House radius/Foot688/Infantry68D state, and explicit GI ART inputs.
// The first Scenario change is localized by foot_bridge_layer.replay.json;
// this remains a Rust regression pin, not a native whole-skirmish golden.
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
// Incoming main's BuildingType +0x6C (0x00464A70) floor coordinate is now
// retained by Reveal, including zero. Complete before/final/control receipts
// in foot_bridge_layer.main986.replay.json attribute all819 observed frames:
// only GAWEAP/GAREFN exact Z changes None -> Some(0); every other actor field,
// all three RNG states and ordered draw values match. Raw caller line changes
// are retained. Omitting only Structure exact Z from the hash recovers every
// prior frame hash and CE21_A562_A129_5C86; gameplay XYZ remains untouched.
// The uncommitted control is preserved with source/binary identities. This
// composition change establishes a Rust regression pin, not native world parity.
// v25 integration: native own-slot Fire, literal passive mission and Stop
// retention change behavior; see the complete causal account above.
// Native Hover host (snapshot 269): the locomotor fold drops the three
// SimFixed Hover copies and hashes the whole HoverLocomotionClass object in
// place of its head. Ceremony: this change with the old composition (three
// zero words, head only; probe not committed) printed the previous pin, so
// no hashed state moved. Previous: 0xC9C5_B19A_6873_6EA8.
// 2026-09-30 one FootClass::Mark owner (#922): Mark no longer writes the
// AircraftTracker, which native Mark never touches, so a ground object keeps
// its constructor-seeded enter order (its stable id) where the old lifecycle
// Mark reset it to 0 on every Foot and Building Mark. Ceremony, rerun after
// merging main: this change with only that reset restored printed the old
// value for all three replay pins (bridge, global, slice 6), with the RNG pins
// above unchanged (the probe patch was not committed). Previous:
// 0x64B4_3781_2052_4AF1.
// 2026-10-01 turretless hull turn (#690, behavior): `Fire_At_Target`'s FACING
// case turns a turretless hull only with no NavCom (`0x00736FB6`) and the
// locomotor's Is_Moving false (`0x00736FE1`); VERA asked for no order. Tank 4,
// retasked from Move to Attack, rolls on to its Drive's committed head through
// frames 336..364 with no NavCom and no order, so it now turns at 365 instead
// of 336. Its five later shots move from 338/390/441/492/542 to
// 366/417/469/521/571, and the reordered Scenario draws move tank 6's killing
// shot from 588 to 587; the 13 shots and tank 6's 12 HP stand. Ceremony: this
// change with only the two old order gates restored printed the previous value
// and every RNG pin above (the probe patch was not committed). Previous:
// 0xF066_BF94_338B_683B.
// 2026-10-01 Walk route owner (hash composition only): Walk keeps no
// MovementTarget route cells and no locomotor sub-cell destination, both of
// which the state hash folded. Ceremony: main and this change, each with only
// those fields dropped from the hash (probe not committed), printed the same
// value for all three replay pins (bridge, global, slice 6). Previous: 0x7099_F3F4_CCAB_0F1E.
// Historical retained-path integration compared all601 main7412/candidate
// observations after omitting newly retained actor fields and RNG source
// locations; gameplay observations matched and only hash composition moved.
// That pre-#962/#963 candidate pin was 0xB252_83B6_7486_B760. The bounded Rust
// receipt remains tools/spatial_oracle/astar_path_finishing_replay/receipt.json.
// Main963 retained-path composition: the same-binary control reproduces
// 0xEB54_33C6_1160_7535 by omitting only navigation history, House threat, Foot530
// and cached Techno508 hash feeds. All601 control/current observations
// match exactly except tick hashes, including full actor state and all
// three RNG streams/draws/caller positions. The temporary gate was removed.
// Rust-only receipt: tools/spatial_oracle/astar_path_finishing_replay/main963/receipt.json.
// 2026-10-01 Stop through the Unit setter (#952): Event IDLE's null
// destination is now Unit 0x00741970, which returns before any write without a
// NavCom (0x00741A80). Tank 4 is at rest and attacking with no NavCom at the
// tick-300 Stop, still holding a Foot speed fraction of about 0.73. The old
// body's Drive Stop zeroed it (navcom.rs `drive_stop_moving`, itself a
// stand-in for the unported Drive rest tail 0x4B0828), so the old tick-320
// retreat order (to 8,8) now departs faster, leaves tank 6's fire, and the
// duel never resolved. RESIDUAL: the native departure speed after an at-rest
// stop is not established until 0x4B0828 is ported. The order now moves tank 4
// one cell toward tank 6, and tank 4 still dies inside the run. Main/MapGen
// are unchanged. Previous: 0x8976_4F17_0FCF_A3FB.
// 2026-10-02 Building body has one retained owner (hash composition):
// same-binary control restores only the former two absent Up/Down
// folds and reproduces the preceding pin. All819 control/current
// boundaries match after only tick hashes are omitted: full actors,
// commands, lifecycle/Logic and three RNG streams/draws/callers.
// Rust-only receipt: tools/spatial_oracle/building_construction_replay/receipt.json.
// Previous: 0xC3551AAB44B60A29. Native proof is the separate construction corpus.
// 2026-10-02 Engineer/House retained state (Rust-only hash composition):
// The same-binary four-feed control reproduces the preceding main pin.
// All819 complete current/control boundaries match except tick hashes,
// including House/Factory state and three full RNG streams/draws/callers.
// Scope and receipt: tools/spatial_oracle/engineer_repair_replay/receipt.json.
// 2026-10-02 Unit deployment/body ownership: snapshot280 replaces the legacy
// DeployPhase hash with Techno130/134, adds Unit6E0, and advances the existing
// Foot538 counter for voxel Units. The 13-shot duel, surviving tank health,
// mission transitions and RNG pins remain unchanged. The same-binary hash
// control reproduces incoming 851C57C576CA991A; all 601 off/on boundaries differ
// only in tick hash. tools/spatial_oracle/unit_simple_deploy_replay/receipt.json
// records this Rust attribution; unit_simple_deploy separately pins native cadence.
// Shared Techno Door hash composition after main339b57d18 integration:
// one diagnostic binary restores incoming C86F736CE95687B3 when only the Door
// hash feed is omitted. Behavior and RNG assertions reach the final pin in
// both modes. The temporary control is removed; native expected values stay
// unchanged. Receipt: tools/spatial_oracle/anytown_damage/unit_unlimbo.md.
// 2026-10-02 IFV owner migration: u8 last-shot hashing becomes signed i32
// current-weapon hashing and the redundant None override feed is removed.
// All 601 boundaries retain gameplay/RNG; each new saved charge delay is the
// last of the 13 unchanged FireAt rearm writes. Bounded Rust attribution:
// tools/spatial_oracle/ifv_turret_replay/receipt.json (not native replay evidence).
// Shared Door composition after main1009: same test binary, 601 complete
// off/on boundaries equal except tick_result.state_hash. Omitting only Door
// restores incoming main; the temporary control is removed. Bounded Rust
// attribution: tools/spatial_oracle/anytown_damage/unit_unlimbo.md.
// Snapshot287: the entity-level absent Teleport tag is removed; the complete
// class payload owns its fold. Restoring only that old zero tag recovers the prior
// hash exactly, with all existing pose/gameplay and absolute RNG tripwires intact.
// See tools/spatial_oracle/infantry_teleport_destination.md. Rust hash ratchet only.
// Depot Mission_Repair Stage620 is independently owned and hashed. The
// same-binary control omitting only this feed restores incoming main; all
// 601 complete rows match after removing only tick_result.state_hash.
// Integrated Rust attribution (not native parity):
// tools/spatial_oracle/depot_service_replay/main1018/receipt.json.
// The preceding-main attribution remains in depot_service_replay/receipt.json.
// After main1024 integration, restoring only that absent tag recovers incoming
// CDA10AEE6EA0AE46 in the same test binary. All 601 complete observations remain
// equal except tick_result.state_hash. The control is removed in this candidate.
// tools/spatial_oracle/infantry_teleport_replay/main1024/receipt.json.
// Snapshot288: retained Drive/Ship state hashes once in its complete class payload.
// The same-binary old entity-feed control recovers incoming534C7788BB49A3D8;
// all601 complete rows match except state_hash. Bounded Rust attribution:
// tools/spatial_oracle/drive_instance_replay/receipt.json. Control removed.
// Barracks output after main7e9: shared TechnoUnlimbo6F6DAA retains the
// constructor facing Some(0); private Foot6B3/Factory5D feeds enter the hash.
// The Drive/Ship payload migration remains. All618 same-binary Rust rows
// match except tick_result.state_hash; the control recovers incoming pins.
// tools/spatial_oracle/factory_infantry_output_replay/main7e9/receipt.json.
// Rocket locomotor port: the entity-level absent rocket_state tag is removed;
// the Rocket payload owns its fold. Restoring only that old 0 tag recovers
// incoming 0F2CFFB842D8155E in the same test binary. Control removed.
// Snapshot303: HoverAttack, target_pad and pad_index leave the hash. A control
// binary (main with a hash that skips only those fields) gives this value in
// the same test, with every tripwire above green. Control removed.
// Snapshot305: Miner.forced_return leaves the hash. A control binary (main
// with a hash that skips only that fold) gives this value in the same test,
// with every tripwire above green. Control removed.
// Snapshot306: the legacy CellClass counters, flags, occlusion caches and
// visibility marks leave the hash (the ground bits stay), and SightAdmission
// drops fog_of_war. A control binary (main with a hash that skips only those
// fields) gives this value in the same test, with every tripwire above green.
// Control removed.
// Snapshot307: the session's LocalSize copy leaves the hash. A control binary
// (main with a hash that skips only that tuple) gives this value in the same
// test, with every tripwire above green. Control removed.
// Snapshot313: the house score is one Economy field, so its hash slot carries
// the kill points and MatchStatistics hashes no score. A control binary (main
// with only that hash layout) gives this value in the same test, with every
// tripwire above green. Control removed.
// Snapshot318 hashes MoveSound's signed i32 countdown instead of u8. The
// fixture's vectors are empty, but qualifying Foot visits retain countdown3
// while inactive (4DAA87; foot_move_sound.json empty_qualifies). No sound is
// selected and no Main draw is paid. This remains a Rust regression pin.
// Snapshot320 retains native Techno constructor disguise identity/timer state
// for every actor, replacing the absent component. These fixtures acquire no
// disguise; their gameplay and absolute RNG pins remain the independent gates.
// Snapshot324 replaces the disabled/fired trigger-latch fold with retained
// definition/live-instance/registration/attachment/dispatch state. This fixture
// has no authored Tags; per-frame replay, combat/mining and all three RNG gates
// precede this structural Rust hash pin and remain unchanged.
// Snapshot332 drops the C4 walk-up intent, whose empty value every entity's
// fold hashed. This fixture has no C4 infantry; per-frame replay, combat/mining
// and all three RNG gates remain unchanged.
// 2026-10-13 native attack-move (Foot MegaMission, snapshot 340): tank 4's
// order is the MegaMission. Arrival_Target_Handler scans only when its
// targeting timer runs out, so it finds the infantryman at frame 297 instead
// of the per-visit OrderIntent scan finding tank 6 at 281, and the fight that
// follows changes (story at the end of the test). The Scenario stream moves
// with it; Main/MapGen, per-frame replay and the miner tripwires are
// unchanged.
// Snapshot342 retains House+1FC. Neither fixture House joins house_order,
// so both retain the constructor's pending recheck. Clearing only those two
// flags in the same finished replay recovers the incoming 9F0CFB4D01682378;
// all per-frame replay, gameplay and absolute RNG gates remain unchanged.
// The diagnostic control was removed.
// 2026-10-14: hash composition and coverage only. state_hash folds
// through the pinned StateHasher instead of std's DefaultHasher (a
// temporary control folding the finished replay through DefaultHasher
// still gave the previous 0x08F7_A04E_1574_71DE), fog grids and entities fold
// as per-House and per-entity digests, cell grids and footprint lists
// fold as packed byte records, and the edge cache also covers
// uninitialized cells: code that writes no simulation state.
// Snapshot349 drops the move-order adapter (`MovementTarget`, #689), whose
// presence and speed stamp every entity's fold hashed. Hash composition
// only: a control binary (main with a hash that skips only that fold)
// gives this value in the same test, with every tripwire above green.
// Control removed. Previous: 0x6F38_B6BF_3787_953B.
// Snapshot350 drops the locomotor's altitude copy (#692), which every
// locomotor's fold hashed. Hash composition only: a control binary (main
// with a hash that skips only that fold) gives this value in the same
// test, with every tripwire above green.
// Control removed. Previous: 0xE527_F567_DC13_C55E.
// Integrated StateHasher pin after executed scoped feed attribution. The test
// below recovers current main exactly with retired feeds and preserves serialized
// gameplay, the live Dummy and all three RNG streams; this is a Rust ratchet.
// Re-pinned after merging main's +0x438/+0x439 latches into the hash.
const GLOBAL_HARNESS_FINAL_HASH: u64 = 0x3B11_939D_CFE5_8242;

// ALL01 r6 (fork-local schema345) used DefaultHasher before main's
// pinned StateHasher, entity/fog digests and owner migrations. These remain
// historical Rust receipts; they are not comparable to current u64 outputs.
const GLOBAL_HARNESS_INCOMING_HASH: u64 = 0x0D69_4099_AD39_5D63;
const GLOBAL_HARNESS_R6_FINAL_HASH: u64 = 0x488E_0917_87DF_8E1A;
// The legacy-feed control uses this main StateHasher reference, which
// includes the +0x438/+0x439 infantry tracking latches.
const GLOBAL_HARNESS_MAIN_HASH: u64 = 0x6616_10FD_E8C9_ADDE;

fn harness_ini() -> IniFile {
    // Multi-faction vehicles + infantry + buildings (war factory, refinery) plus a
    // real harvester (Harvester/Dock/Storage) and a real refinery (Refinery=yes)
    // so the miner dock path is reachable. Short weapon ranges keep combat to the
    // scripted engagements, keeping the scenario deterministic.
    //
    // KNOWN COVERAGE GAP — the fire-range gate is invisible to this tripwire.
    // `[M60] Range=5` and `[105mm] Range=6` are whole cells, neither weapon sets
    // `MinimumRange=` or `CellRangefinding=`, no projectile section exists (so
    // nothing is `Arcing=`), and no attacker is airborne. The 2026-09 change that
    // moved `TechnoClass::InRange` 0x006F7220 onto lepton ranges and onto the
    // source coordinate `TechnoClass::CanFireAt` 0x006F77B0 builds shifted the
    // reach of 60 of the 256 stock `Range=` weapons — Rhino and Apocalypse at
    // `Range=5.75` among them — and moved NO hash here, because this fixture
    // cannot express any of those inputs. Read an unchanged harness hash as
    // "this scenario is unaffected", never as "range behaviour did not move".
    // Anyone extending this fixture: a fractional `Range=` on `[105mm]` would
    // close the gap, at the cost of one re-baseline of GLOBAL_HARNESS_FINAL_HASH
    // plus the stream pins.
    //
    // The same gap covers the line-of-fire walk at `InRange`'s tail
    // (0x006F7642 -> 0x004CC310). It fires only for a projectile that sets
    // `SubjectToWalls=` or `SubjectToCliffs=`, and neither `[M60]` nor
    // `[105mm]` declares a `Projectile=` at all, so no projectile section
    // exists to carry either key; the only overlay here is `TIB01` ore, so
    // there is no wall on any line; and the terrain the scenario builds is
    // flat, so no four-Level cliff step exists either. Stock `[Cannon]` and
    // `[InvisibleLow]` — every cannon tank and every small-arms infantryman —
    // DO set both keys. Closing this half needs a projectile section plus a
    // wall overlay or a level step in the fixture terrain, and the same
    // re-baseline.
    IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n1=HARV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAWEAP\n1=GAREFN\n\n\
         [OverlayTypes]\n0=TIB01\n\n\
         [Tiberiums]\n0=Riparius\n\n\
         [Riparius]\nImage=1\nValue=25\n\n\
         [TIB01]\nTiberium=yes\n\n\
         [Tiberium]\nFoot=100%\nTrack=100%\nWheel=100%\n\n\
         [E1]\nImage=GI\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [HARV]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=600\nArmor=heavy\nSpeed=5\nHarvester=yes\nStorage=28\nDock=GAREFN\n\n\
         [GAWEAP]\nStrength=1000\nArmor=wood\nFoundation=4x3\n\n\
         [GAREFN]\nStrength=1000\nArmor=wood\nRefinery=yes\nDockUnload=yes\nFoundation=3x3\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    )
}

fn harness_rules() -> RuleSet {
    let ini = harness_ini();
    // Explicit authored GI inputs use the production fixed-ART reader and binder;
    // zero-count constructor records do not admit native Ready/idle actions.
    let mut art = IniFile::from_str(crate::rules::retail_ini_fixture::GI_ART_EXCERPT);
    art.merge(&IniFile::from_str(
        "[GAWEAP]\nFoundation=4x3\n[GAREFN]\nFoundation=3x3\n",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    rules
}

fn harness_overlays() -> OverlayTypeRegistry {
    OverlayTypeRegistry::from_ini(&harness_ini(), None)
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
        veterancy: Some(0),
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        trigger_tag: None,
    }
}

/// Build the recorded scenario into `sim`. Spawn order fixes stable ids
/// 1..=7 (war factory, refinery, harvester, Allied tank, Allied infantry,
/// Soviet tank, Soviet infantry).
fn seed_scenario(sim: &mut Simulation, rules: &RuleSet, overlays: &OverlayTypeRegistry) {
    // Flat map cells and the playfield, as a map load installs them before
    // placing objects: Unlimbo's Unit `Can_Enter_Cell` reads both, and the
    // harvester's ore scan (`FootClass::Is_Cell_Harvestable @ 0x004DCE80`)
    // admits only playfield cells of LandType 5.
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(
        HARNESS_CELL_SIDE,
    ));
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 0,
        width: u32::from(HARNESS_MAP_SIZE),
        height: u32::from(HARNESS_MAP_SIZE),
        local_left: 2,
        local_top: 2,
        local_width: 60,
        local_height: 56,
    });
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            sim.resolved_terrain.as_ref().unwrap(),
            true,
            rules.bridge_rules.strength,
            (i32::from(HARNESS_MAP_SIZE), i32::from(HARNESS_MAP_SIZE)),
        ),
    );
    // The owners' houses exist before a map load places objects, so each
    // object's constructor `Add_Tracking` counts it: the harvester's Dock
    // checks read the house's tracked BuildingType counts (`+0x5500`). They
    // are computer houses, which keep this run's behaviour: the damage
    // response treated a house-less owner as a computer's (a human house's
    // AttackMove tank keeps moving when hit), and the harvester branches that
    // treat a missing house as human are not reached in these 600 ticks. They
    // join no house order, so no house AI runs.
    for (owner, side) in [("Americans", 0), ("Soviet", 1)] {
        let id = sim.interner.intern(owner);
        sim.houses.entry(id).or_insert_with(|| {
            crate::sim::house_state::HouseState::new(id, side, None, false, 0, 10)
        });
    }
    let mut authored = [
        unit("Americans", "GAWEAP", 3, 3, EntityCategory::Structure), // 1
        unit("Americans", "GAREFN", 3, 10, EntityCategory::Structure), // 2
        unit("Americans", "HARV", 8, 12, EntityCategory::Unit),       // 3
        unit("Americans", "MTNK", 10, 8, EntityCategory::Unit),       // 4
        unit("Americans", "E1", 11, 9, EntityCategory::Infantry),     // 5
        unit("Soviet", "MTNK", 40, 8, EntityCategory::Unit),          // 6
        unit("Soviet", "E1", 41, 9, EntityCategory::Infantry),        // 7
    ];
    // Preserve relative scene geometry inside a valid native Size diamond.
    // The former width-zero rectangle cannot contain both (3,3) and (41,9).
    for entity in &mut authored {
        entity.cell_x += HARNESS_COORD_SHIFT;
        entity.cell_y += HARNESS_COORD_SHIFT;
        assert!(sim.map_cell_in_bounds((entity.cell_x as i16, entity.cell_y as i16)));
        assert!(
            sim.playfield_bounds
                .unwrap()
                .contains_geometry_packed(i32::from(entity.cell_x), i32::from(entity.cell_y),)
        );
    }
    sim.spawn_from_map(&authored, Some(rules));
    // Seed the native CellClass overlay authority near the harvester.
    let tib01 = overlays.id_for_name("TIB01").expect("harness TIB01");
    let mut overlay_grid = OverlayGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    let terrain = sim.resolved_terrain.as_mut().expect("installed above");
    for (rx, ry) in [(12, 13), (13, 13), (12, 14), (13, 14)] {
        let (rx, ry) = (rx + HARNESS_COORD_SHIFT, ry + HARNESS_COORD_SHIFT);
        overlay_grid.place_overlay(rx, ry, tib01, 11);
        // RecalcAttributes: LandType 5 and its [Tiberium] speed row.
        overlay_grid.recalculate_runtime_cell(terrain, overlays, (rx, ry));
    }
    overlay_grid.take_dirty_cells();
    sim.overlay_grid = Some(overlay_grid);
    // Live Foot reachability consumes the same map authorities as a map load.
    // The old ore-query copy silently skipped zones in this direct-spawn scene.
    // This harness passes its PathGrid into each frame; retain that owner
    // rather than installing a second grid while completing the zone fixture.
    let path = PathGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
            &path,
            sim.resolved_terrain.as_ref().unwrap(),
            &[],
            sim.map_size_diamond(),
            sim.playfield_bounds,
        ),
    );
}

/// Scripted commands keyed by `execute_tick` (fires when tick+1 == execute_tick).
/// The former frame120 Allied-issued Move for Soviet tank6 was rejected by
/// VERA's issuer gate, so it never affected this duel's pinned transitions.
/// Native Event4C71CA accepts that actor independently of the issuer. Its
/// execution and replay are covered separately below, preserving this fixture's
/// Stop/retaliation/expiry coverage instead of silently changing its scenario.
fn harness_script() -> Vec<(u64, Command)> {
    vec![
        (
            2,
            Command::Move {
                entity_id: 4,
                target_rx: 24 + HARNESS_COORD_SHIFT,
                target_ry: 8 + HARNESS_COORD_SHIFT,
            },
        ),
        (
            40,
            Command::AttackMove {
                entity_id: 4,
                target: None,
                destination: Some(crate::sim::combat::TargetKind::Cell(
                    38 + HARNESS_COORD_SHIFT,
                    8 + HARNESS_COORD_SHIFT,
                )),
            },
        ),
        (300, Command::Stop { entity_id: 4 }),
        // One cell toward tank 6: the move keeps tank 4 inside the duel.
        (
            320,
            Command::Move {
                entity_id: 4,
                target_rx: 36 + HARNESS_COORD_SHIFT,
                target_ry: 8 + HARNESS_COORD_SHIFT,
            },
        ),
    ]
}

/// Owner of every scripted command (all are issued by the Allied player).
fn due_commands(sim: &Simulation, script: &[(u64, Command)], tick: u64) -> Vec<CommandEnvelope> {
    let owner = sim.interner.get("Americans").expect("Americans interned");
    script
        .iter()
        .filter(|(t, _)| *t == tick + 1)
        .map(|(t, c)| CommandEnvelope::new(owner, *t, c.clone()))
        .collect()
}

#[test]
fn cross_house_move_executes_and_replays_without_changing_actor_owner() {
    let rules = harness_rules();
    let overlays = harness_overlays();
    let grid = PathGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    let mut recorded = Simulation::with_seed(HARNESS_SEED);
    seed_scenario(&mut recorded, &rules, &overlays);
    let actor_owner = recorded.substrate.entities.get(6).unwrap().owner();
    let issuer = recorded.interner.get("Americans").unwrap();
    assert_ne!(actor_owner, issuer);
    let destination = (28 + HARNESS_COORD_SHIFT, 10 + HARNESS_COORD_SHIFT);
    let command = CommandEnvelope::new(
        issuer,
        1,
        Command::Move {
            entity_id: 6,
            target_rx: destination.0,
            target_ry: destination.1,
        },
    );
    let mut replay = ReplayLog::new(ReplayHeader {
        version: 1,
        pixel_conversion_bounds: recorded.session.pixel_conversion_bounds,
        tick_hz: 15,
        seed: HARNESS_SEED,
        map_name: "cross_house_move".into(),
        rules_hash: 0,
    });
    let result = recorded.advance_tick(
        std::slice::from_ref(&command),
        Some(&rules),
        Some(&grid),
        Some(&overlays),
        HARNESS_TICK_MS,
    );
    assert_eq!(
        recorded
            .substrate
            .entities
            .get(6)
            .unwrap()
            .navigation
            .nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(
            destination.0,
            destination.1
        ))
    );
    replay.record_tick(0, vec![command], result.state_hash);
    let replay: ReplayLog = serde_json::from_slice(&serde_json::to_vec(&replay).unwrap()).unwrap();
    let mut restored = Simulation::with_seed(HARNESS_SEED);
    seed_scenario(&mut restored, &rules, &overlays);
    let hashes = ReplayRunner::run_fixture_with_overlay_registry(
        &mut restored,
        &replay,
        Some(&rules),
        Some(&grid),
        Some(&overlays),
        HARNESS_TICK_MS,
    );
    assert_eq!(hashes, vec![result.state_hash]);
    assert_eq!(restored.state_hash(), recorded.state_hash());
    let actor = restored.substrate.entities.get(6).unwrap();
    assert_eq!(actor.owner(), actor_owner);
    assert_eq!(
        actor.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(
            destination.0,
            destination.1
        ))
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn global_skirmish_replay_is_deterministic_and_baseline_stable() {
    let rules = harness_rules();
    let overlays = harness_overlays();
    let grid = PathGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    let script = harness_script();

    // ---- Record pass: build a ReplayLog through the live advance_tick path. ----
    let mut rec = Simulation::with_seed(HARNESS_SEED);
    let mut diagnostic = replay_diagnostic_file("global");
    let mut seed = || seed_scenario(&mut rec, &rules, &overlays);
    let (_, seed_draws) = if diagnostic.is_some() {
        crate::sim::rng::trace_draws(seed)
    } else {
        (seed(), Vec::new())
    };
    record_replay_diagnostic(&mut diagnostic, &rec, None, &[], &seed_draws);
    let mut log = ReplayLog::new(ReplayHeader {
        version: 1,
        pixel_conversion_bounds: rec.session.pixel_conversion_bounds,
        tick_hz: 15,
        seed: HARNESS_SEED,
        map_name: "global_parity_harness".to_string(),
        rules_hash: 0,
    });
    // Coverage tripwire: the harvester (id 3) must be picked up by the miner
    // system — it acquires an ore target via the SearchOre path, drives to its
    // field and cuts. (The full dock handshake is the dedicated miner-dock
    // suites' coverage.) This guards that miner-component creation + the
    // acquisition path stay wired and contribute to the hash.
    let mut miner_acquired_field = false;
    let mut miner_moved = false;
    let mut miner_cut_ore = false;
    // AT-8 stream pins: per-stream cursor fingerprints captured at checkpoint
    // ticks during record, re-asserted in replay. Total-hash equality can mask
    // a draw routed to the wrong stream when a compensating error exists;
    // per-stream checkpoints catch misrouting directly.
    let mut recorded_streams: Vec<(u64, u64, u64, u64)> = Vec::new();
    let mut first_uncommitted_frame = None;
    let mut target_expired = None;
    let mut guard_commenced = None;
    let mut post_guard_scan = None;
    for tick in 0..HARNESS_TICKS {
        let due = due_commands(&rec, &script, tick);
        let before_scan = {
            let tank = rec.substrate.entities.get(6).expect("surviving tank");
            (
                tank.passive_scan.last_scan_frame(),
                tank.passive_scan.timer(),
            )
        };
        let mut advance = || {
            rec.advance_tick(
                &due,
                Some(&rules),
                Some(&grid),
                Some(&overlays),
                HARNESS_TICK_MS,
            )
        };
        let (result, draws) = if diagnostic.is_some() {
            crate::sim::rng::trace_draws(advance)
        } else {
            (advance(), Vec::new())
        };
        record_replay_diagnostic(&mut diagnostic, &rec, Some(&result), &due, &draws);

        // Tank 4's attack-move scan (Arrival_Target_Handler on its targeting
        // timer, `0x006FA681..0x006FA695`) finds the infantryman at frame 297
        // and commits Attack there (Queue_Mission(Attack, 1), `0x004DF418`).
        // Event IDLE after Logic frame 299 clears TarCom and NavCom; it queues
        // nothing, keeps MissionCom and calls no MegaMission body, so Attack
        // stays committed with its dispatch timer and the engaged order
        // survives, until tank 6's first hit (frame 302) draws retaliation.
        if (299..302).contains(&tick) {
            let tank = rec.substrate.entities.get(4).expect("stopped tank lives");
            assert_eq!(
                tank.mission.current(),
                MissionId::from_known(MissionType::Attack)
            );
            assert_eq!(tank.mission.queued(), MissionId::NONE);
            assert_eq!(tank.mission.mission_start_frame(), 297);
            assert_eq!(
                (
                    tank.mission.dispatch_timer().start_frame(),
                    tank.mission.dispatch_timer().delay()
                ),
                (298, 16),
                "Stop retains the active dispatch timer"
            );
            assert!(tank.attack_target.target().is_none());
            assert!(tank.navigation.nav_com().is_none());
            assert!(tank.mega_mission.is_attack_move());
            assert_eq!(tank.passive_scan.last_scan_frame(), 297);
        }
        // The fatal Bullet detaches TarCom after the Unit's own visit. The
        // literal committed Attack gate cannot scan before a due Attack
        // handler queues Guard and the later Unit Commence promotes it.
        // Observe that transition rather than importing a pre-integration
        // death frame: main's native Infantry cadence changes the duel's draws.
        if rec.substrate.entities.get(4).is_none() {
            let tank = rec.substrate.entities.get(6).expect("surviving tank");
            assert!(tank.attack_target.target().is_none());
            let scan = (
                tank.passive_scan.last_scan_frame(),
                tank.passive_scan.timer(),
            );
            if target_expired.is_none() {
                target_expired = Some(tick);
                assert_eq!(scan, before_scan, "expiry itself cannot run a scanner");
            }
            match tank.mission.current().known() {
                Some(MissionType::Attack) => {
                    assert!(guard_commenced.is_none());
                    assert_eq!(scan, before_scan, "committed Attack rejects passive scan");
                }
                Some(MissionType::Guard) => {
                    if let Some(guard_frame) = guard_commenced {
                        if tick == guard_frame + 1 {
                            assert_eq!(tank.passive_scan.last_scan_frame(), tick as u32);
                            post_guard_scan = Some(tick);
                        }
                    } else {
                        guard_commenced = Some(tick);
                        assert_eq!(scan, before_scan, "passive slot precedes Unit Commence");
                    }
                }
                mission => panic!("unexpected surviving-tank mission: {mission:?}"),
            }
        }

        if !result.frame_committed {
            first_uncommitted_frame.get_or_insert(tick);
        }

        if let Some(harvester) = rec.substrate.entities.get(3) {
            if let Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) =
                harvester.navigation.nav_com()
            {
                miner_acquired_field |= (12 + HARNESS_COORD_SHIFT..=13 + HARNESS_COORD_SHIFT)
                    .contains(&rx)
                    && (13 + HARNESS_COORD_SHIFT..=14 + HARNESS_COORD_SHIFT).contains(&ry);
            }
            miner_moved |= (harvester.position.rx(), harvester.position.ry())
                != (8 + HARNESS_COORD_SHIFT, 12 + HARNESS_COORD_SHIFT);
            miner_cut_ore |= harvester
                .miner
                .as_ref()
                .is_some_and(|miner| !miner.cargo().is_empty());
        }
        log.record_tick(tick, due, result.state_hash);
        if STREAM_CHECKPOINT_TICKS.contains(&tick) {
            recorded_streams.push((
                tick,
                rec.scenario_rng.state(),
                rec.main_rng.state(),
                rec.mapgen_rng.state(),
            ));
        }
    }
    assert!(
        target_expired.is_some(),
        "the duel must detach the dead target"
    );
    assert!(
        guard_commenced > target_expired,
        "Guard follows target expiry"
    );
    assert_eq!(post_guard_scan, guard_commenced.map(|frame| frame + 1));
    assert_eq!(
        first_uncommitted_frame, None,
        "all scripted frames must commit"
    );
    // The blocker plane follows the entity store's touch log. Anything that
    // hands out every entity mutably each frame (`values_mut`) would quietly
    // turn that back into a whole-world read per frame, with correct results
    // and no other symptom. It is rebuilt from the whole map only when the
    // terrain epoch or the wall plane moves (or a reader misses Foot deltas);
    // a rebuild per moving object's turn would be thousands.
    let plane_reads = rec.movement_pass_cache.blocker_plane_world_rebuilds();
    assert!(
        plane_reads <= 60,
        "the blocker plane was rebuilt from the whole map {plane_reads} times"
    );
    assert!(
        miner_acquired_field && miner_moved && miner_cut_ore,
        "the miner must acquire its field, move and cut ore: \
         acquired={miner_acquired_field}, moved={miner_moved}, cut={miner_cut_ore}"
    );

    // ---- Replay pass: fresh sim, real ReplayRunner, assert tick-by-tick.
    // The registry-aware entry uses the SAME master-frame path as the legacy
    // convenience entry, chunked at the stream checkpoints so the per-stream
    // cursors can be pinned between chunks. ----
    let mut rep = Simulation::with_seed(HARNESS_SEED);
    seed_scenario(&mut rep, &rules, &overlays);
    let mut replayed: Vec<Option<u64>> = Vec::with_capacity(log.ticks.len());
    let mut replayed_streams: Vec<(u64, u64, u64, u64)> = Vec::new();
    let mut chunk_start = 0usize;
    for &checkpoint in STREAM_CHECKPOINT_TICKS {
        let chunk_end = (checkpoint as usize + 1).min(log.ticks.len());
        let chunk = ReplayLog {
            header: log.header.clone(),
            ticks: log.ticks[chunk_start..chunk_end].to_vec(),
        };
        replayed.extend(ReplayRunner::run_fixture_with_overlay_registry(
            &mut rep,
            &chunk,
            Some(&rules),
            Some(&grid),
            Some(&overlays),
            HARNESS_TICK_MS,
        ));
        replayed_streams.push((
            checkpoint,
            rep.scenario_rng.state(),
            rep.main_rng.state(),
            rep.mapgen_rng.state(),
        ));
        chunk_start = chunk_end;
    }
    if chunk_start < log.ticks.len() {
        let tail = ReplayLog {
            header: log.header.clone(),
            ticks: log.ticks[chunk_start..].to_vec(),
        };
        replayed.extend(ReplayRunner::run_fixture_with_overlay_registry(
            &mut rep,
            &tail,
            Some(&rules),
            Some(&grid),
            Some(&overlays),
            HARNESS_TICK_MS,
        ));
    }
    assert_eq!(
        recorded_streams, replayed_streams,
        "per-stream cursor consistency: a nondeterminism moved streams between record and replay"
    );
    assert_eq!(
        replayed.len(),
        log.ticks.len(),
        "replay tick count must match record"
    );
    for (i, h) in replayed.iter().enumerate() {
        assert_eq!(
            *h, log.ticks[i].state_hash,
            "intra-run determinism: replay tick {i} hash must equal the recorded hash"
        );
    }

    let (_, final_scen, final_main, final_mapgen) =
        *recorded_streams.last().expect("final checkpoint recorded");
    let final_hash = replayed
        .last()
        .expect("at least one tick recorded")
        .expect("every replayed frame hashes");
    print_replay_summary("global", &rep);
    assert_eq!(
        (final_scen, final_main, final_mapgen),
        FINAL_STREAM_STATES,
        "absolute per-stream regression: establish the changed producer/cadence before updating"
    );
    assert!(
        rep.substrate
            .entities
            .get(2)
            .expect("harness refinery")
            .radio_contacts
            .is_empty(),
        "the fixture ends without a held refinery contact"
    );
    assert_eq!(
        rep.substrate.anims.len(),
        0,
        "the fixture emits no Anim objects"
    );
    assert!(
        rep.substrate
            .entities
            .values()
            .all(|e| e.gap_generator == Default::default())
    );
    assert!(
        rep.power_states
            .values()
            .all(|s| !s.has_drained_power_source)
    );
    assert!(
        rep.substrate
            .entities
            .values()
            .all(|e| *e.building_art.storage() == Default::default()
                && e.ammo().is_none_or(|ammo| ammo.current() == -1)),
        "the fixture contains no storage or nondefault ammo state"
    );

    // Tank 4's attack-move finds the infantryman at frame 297 and fires on
    // it; tank 6's first hit at 302 draws its retaliation. Stop at 299 leaves
    // the engaged order (the Stop window above); the Move at 319 ends it
    // (Assign_Target_Command clears the MegaMission). Tank 4 kills the
    // infantryman with its second shot, then trades four hits with tank 6,
    // which ends at 108 HP; tank 6's sixth hit, after the infantryman's three,
    // kills tank 4 (target expiry at 556). These are Rust regression values,
    // not a native execution of this synthetic whole-skirmish fixture.
    assert_eq!(rep.fire_events.len(), 15);
    assert_eq!(
        rep.substrate
            .entities
            .get(4)
            .map(|tank| tank.health.current()),
        None,
        "the retasked tank dies in its duel"
    );
    assert_eq!(
        rep.substrate
            .entities
            .get(6)
            .map(|tank| tank.health.current()),
        Some(108),
        "tank 6 takes four 105mm hits (65 * 75% heavy)"
    );
    let legacy_super_owner = rep.interner.get("Americans").expect("building owner");
    assert!(
        rep.session.house_order.is_empty(),
        "this fixture runs no House AI"
    );
    assert!(
        rep.super_weapons.is_empty(),
        "the fixture constructs no Supers"
    );
    assert!(
        rep.substrate
            .entities
            .values()
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .all(|entity| entity.owner() == legacy_super_owner),
        "the retired initializer visited only this building owner"
    );
    let retained_authority = bincode::serialize(&rep).expect("retained replay authority");
    let retained_dummy = rep.effective_shared_cell_dummy().snapshot();
    let retained_streams = (
        rep.scenario_rng.state(),
        rep.main_rng.state(),
        rep.mapgen_rng.state(),
    );
    let incoming_hash_control =
        crate::sim::trigger_runtime::with_legacy_announcement_hash_control_for_test(|| {
            rep.state_hash_with_legacy_empty_super_owner_for_test(legacy_super_owner)
        });
    println!(
        "[global retired hash feeds] current={final_hash:016X} control={incoming_hash_control:016X} main={GLOBAL_HARNESS_MAIN_HASH:016X} archived_default_incoming={GLOBAL_HARNESS_INCOMING_HASH:016X} archived_r6={GLOBAL_HARNESS_R6_FINAL_HASH:016X}"
    );
    assert_eq!(
        incoming_hash_control, GLOBAL_HARNESS_MAIN_HASH,
        "restoring only the retired None and empty Super owner feeds must recover the main StateHasher pin; other drift needs attribution"
    );
    assert_eq!(rep.state_hash(), final_hash, "diagnostic feeds stay scoped");
    assert!(
        retained_authority == bincode::serialize(&rep).expect("unchanged replay authority"),
        "diagnostic feeds cannot alter serialized gameplay state"
    );
    assert_eq!(rep.effective_shared_cell_dummy().snapshot(), retained_dummy);
    assert_eq!(
        (
            rep.scenario_rng.state(),
            rep.main_rng.state(),
            rep.mapgen_rng.state()
        ),
        retained_streams
    );
    assert_eq!(
        final_hash, GLOBAL_HARNESS_FINAL_HASH,
        "committed global-harness baseline drifted: establish the changed behavior, RNG producer, or hash composition before updating"
    );
}

const DENSE_SEED: u64 = 0x00BA771E5EED;
const DENSE_TICKS: u64 = 300;
const DENSE_ROWS: u16 = 10;

/// S2 churn — DENSE arrival case: two facing tank columns (10 Allied vs 10 Soviet) both
/// ordered to converge on the same centre column, so a whole column reaches its
/// destination on the same tick and flips Move→Sleep together. Each Move is issued under
/// ITS OWN owner — the thin generic harness silently rejected one side's move as
/// non-owned, leaving only one real mover. This measures the *simultaneous* per-tick
/// churn the S2 authority flip must survive (a single-mover scenario understates it).
///
/// Scope note: this fixture was built to exercise movement/arrival churn only, and
/// for most of its life the tanks converged without engaging. That is no longer
/// true. Each tank is ordered under its own owner, arrives, and is then
/// ordered-then-idle. The fixture now includes target acquisition and combat;
/// its original finished-mission workaround has since been removed. The
/// position fingerprint below therefore covers engagement churn as well as
/// arrival churn.
/// Shared construction for the dense converging-battle fixture (20 tanks, two
/// facing columns converging on x=25; per-owner Move script due on tick 2).
/// Used by the churn measurement and the S2 position fingerprint below.
#[allow(clippy::type_complexity)]
fn dense_converging_setup() -> (
    Simulation,
    RuleSet,
    PathGrid,
    Vec<(u64, crate::sim::intern::InternedId, Command)>,
) {
    let rules = harness_rules();
    let mut sim = Simulation::with_seed(DENSE_SEED);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let grid = (*sim.path_grid_snapshot().unwrap()).clone();
    let mut roster: Vec<MapEntity> = Vec::new();
    for i in 0..DENSE_ROWS {
        roster.push(unit("Americans", "MTNK", 10, 5 + i, EntityCategory::Unit));
        // ids 1..=10
    }
    for i in 0..DENSE_ROWS {
        roster.push(unit("Soviet", "MTNK", 40, 5 + i, EntityCategory::Unit)); // ids 11..=20
    }
    sim.spawn_from_map(&roster, Some(&rules));

    // Both columns converge on x=25, same row — they close together and arrive/stall
    // in formation. Each Move is under its OWN owner (the thin generic harness rejected
    // one side's move as non-owned, leaving a single real mover). Measures the
    // synchronized-arrival churn (a whole column flipping Move→Sleep on one tick).
    let allied = sim.interner.get("Americans").expect("Americans interned");
    let soviet = sim.interner.get("Soviet").expect("Soviet interned");
    let mut script: Vec<(u64, crate::sim::intern::InternedId, Command)> = Vec::new();
    for i in 0..DENSE_ROWS as u64 {
        let y = 5 + i as u16;
        script.push((
            2,
            allied,
            Command::Move {
                entity_id: 1 + i,
                target_rx: 25,
                target_ry: y,
            },
        ));
        script.push((
            2,
            soviet,
            Command::Move {
                entity_id: 11 + i,
                target_rx: 25,
                target_ry: y,
            },
        ));
    }
    (sim, rules, grid, script)
}

/// S2 movement-neutrality tripwire: per-tick position fingerprint of the dense
/// converging scenario, captured PRE-flip (T2). The S2 dispatch flip changes
/// only `mission.current`/`tick_counter` write points — if this fingerprint
/// shifts, the flip moved someone: that is a bug, never a re-baseline.
/// Re-baselined ONCE after the flip validation closed, for the tube-gate fix:
/// off-tube non-adjacent path steps (sharp-turn fallback bumps) are no longer
/// killed on their issue tick, so movers that previously froze now drive —
/// an intended movement-behavior change, not dispatch-order drift.
/// Re-baselined for the Phase-0 native Main_Tick order: EventClass commands
/// now dispatch at the tail, after the live object/movement walk, so a move
/// accepted on frame N first advances its object on frame N+1.
/// Re-baselined 2026-08-02 for the GSI-04.12 bridge-marker slice (c0b688a6),
/// which moves positions on purpose: `FootPathQueue::reference_cell` advances
/// the path-reference cell when Drive accepts a direction, before the curve
/// physically crosses into the destination cell, and ship locomotion split out
/// of Drive. Hash composition is not involved — this fingerprint folds entity
/// positions directly, and its value was byte-identical with the pre-branch
/// hash schema swapped in.
/// Re-baselined 2026-08-02 for passive/opportunity target acquisition, and this
/// is the fixture where it finally bites. All twenty tanks get a plain Move
/// under their OWN owner, so each one commits the Move selector, arrives, and
/// then has nothing left running — no destination, no navigation goal, no
/// standing order. Those are exactly the objects the finished-mission bridge
/// releases back to Guard, so they now acquire each other on arrival and open
/// fire instead of sitting nose to nose. Positions move because units die.
///
/// It is worth recording why the sibling global-harness constants did NOT move
/// with it, since that looked wrong until it was instrumented: nothing in that
/// fixture is ordered-then-idle. Its Allied MTNK sits on the AttackMove
/// selector, which the gate does not admit, for the whole run; its other three
/// combatants sit on the `NONE` selector and were already scanning before this
/// change. The per-tick observations, and the one cause left UNCHECKED, are
/// recorded in this file's Git history.
/// Re-baselined 2026-08-04 with FINAL_STREAM_STATES for the same
/// constructed-`Rate` change; see the provenance note there.
/// Re-baselined 2026-08-05 for the Drive cell-admission gate. A curve is now
/// refused when the cell it would step into is refused by *either* arm of
/// gamemd's cell-entry predicate — a body in the cell's object list, or another
/// vehicle's occupation mark — where before the runtime consulted neither at
/// selection time. This fixture is twenty tanks converging on one column, so
/// movers that previously drove through each other now wait, scatter and
/// repath; positions move on purpose.
///
/// WITHDRAWN: an earlier revision of this note cited `FINAL_STREAM_STATES` to
/// certify that "no RNG draw moved and no stream was misrouted" for THIS
/// fixture. That claim is wrong on two axes and is retracted. First, the pin
/// lives in a different test — `s2_dense_scenario_position_fingerprint_stable`
/// pins no stream at all, so this twenty-tank convergence has ZERO RNG
/// observation of its own. Second, `SimRng::next_u32` advances as a pure
/// function of its own state, so even where the pin does apply an unchanged
/// final state proves only that the DRAW COUNT on that stream is unchanged; it
/// says nothing about which tick or which consumer took them.
///
/// ATTRIBUTION, MEASURED. This fixture folds only entity ids and positions, so
/// it carries no hash-schema component at all — and that is confirmed rather
/// than assumed. With `occupation_handoff` still on the struct but every
/// behaviour writer neutralised (the experiment written out at
/// `GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH` in this file at 148327c3), this
/// fixture returns to exactly its previous committed value
/// `0x0FC6_3769_AADD_1F8A`. So its shift is 100% behaviour and 0% schema —
/// the mirror image of the global harness above.
///
/// What is still NOT separated: the individual contribution of the object-list
/// arm, the mask arm and the handoff mark, which landed together and were
/// neutralised together. UNVERIFIED.
/// Re-baselined 2026-09-08 for residual cell normalization. A parent (588f4079)
/// comparison of all 6000 tick/entity rows found five isolated world-XY pulses
/// (80 rows, maximum 23 leptons), all equal again on the next tick. The old delayed
/// paid-crossing return skipped that frame's remaining budget/interpolation;
/// the already-normalized cell now takes the native same-cell loop/tail
/// (4B1F56 / 4B22D9). Paths, missions, speeds and membership stayed equal;
/// final entity states differ only in exact Z. This is an intentional movement
/// correction, not a hash-fold change or full locomotor parity claim. The
/// existing early return on an actual paid crossing remains a separate limit.
/// See RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md, Rust replay provenance.
// 2026-09-13: unpaid fresh budget0 retains current XY instead of eagerly
// publishing point0. First native-adjudicated divergence is tick2 (128 vs139);
// see docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md.
// 2026-09-20: destination commands no longer turn/admit ahead of fresh Process.
// All6000 rows were compared with passing bd5928e6: east column unchanged;
// west column exactly one tick later. Native4B3408 calls Do_Turn then returns
// before admission evenROT0. Independent review accepted this Rust regression
// re-pin; it is not a native whole-scenario golden. Same-frame facing repair
// changed no XY. See TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md for scope/evidence.
// 2026-09-23: same-call track-end continuation. First divergence from main is
// tick 30, where the column's first track ends continue into their next
// tracks in that Process; both RNG streams match main over the 300 ticks.
const POSITION_FINGERPRINT: u64 = 0x828D_9C15_C129_26CE;

#[test]
fn fresh_drive_turn_publishes_on_request_frame_and_restores_before_admission() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/drive_fresh_turn.json",
    ))
    .unwrap();
    for rot in [0, 5] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\nROT={rot}\n\
             Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(DENSE_SEED);
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        // Source and loaded graphs both use the retained map/bounds inputs.
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let grid = (*sim.path_grid_snapshot().unwrap()).clone();
        sim.spawn_from_map(
            &[unit("Americans", "MTNK", 40, 5, EntityCategory::Unit)],
            Some(&rules),
        );
        let owner = sim.interner.get("Americans").unwrap();
        let native = rows
            .iter()
            .find(|row| {
                row["input"]["initial"] == 0x4000
                    && row["input"]["direction"] == 6
                    && row["input"]["rate"] == rot * 256
            })
            .unwrap();
        for tick in 1..=3 {
            let due = if tick == 2 {
                vec![CommandEnvelope::new(
                    owner,
                    tick,
                    Command::Move {
                        entity_id: 1,
                        target_rx: 25,
                        target_ry: 5,
                    },
                )]
            } else {
                Vec::new()
            };
            sim.advance_tick(&due, Some(&rules), Some(&grid), None, HARNESS_TICK_MS);
        }
        let entity = sim.substrate.entities.get(1).unwrap();
        let call = &native["calls"][0];
        assert_eq!(
            u64::from(entity.body_facing_byte(sim.session.binary_frame - 1)),
            call["sampled_after"].as_u64().unwrap() >> 8,
            "ROT={rot}: same-frame native sample"
        );
        let drive = entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap();
        assert_eq!(
            drive.track().turn_index(),
            -1,
            "turn must return before admission"
        );
        assert!(drive.head_to().is_none());
        if rot > 0 {
            assert_eq!(
                u64::from(entity.body_facing.current(sim.session.binary_frame - 1)),
                call["sampled_after"].as_u64().unwrap(),
                "full16-bit native sample, before the next binary frame"
            );
            assert_eq!(
                entity.body_facing.timer_start_frame(),
                Some(call["timer_start"].as_u64().unwrap() as u32)
            );
        }
        // Native load resets Scenario to Seed0 and retains process streams.
        // Equalize only that documented load effect before comparing the
        // movement-state round trip and its continued full-world hashes.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 0, "Fresh turn", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
        restored.restore_after_snapshot_load().unwrap();
        assert!(
            restored
                .zone_grid
                .as_ref()
                .unwrap()
                .is_native_load_pending()
        );
        // Bind scenario terrain before original LoadContent's full hierarchy
        // publication. A decoded saved base is not an active hierarchy.
        restored.rebuild_caches_after_load(
            sim.resolved_terrain.as_ref().unwrap().clone(),
            sim.terrain_speed_config.clone(),
            &rules,
        );
        assert!(restored.rebuild_dynamic_navigation(&rules));
        assert!(
            !restored
                .zone_grid
                .as_ref()
                .unwrap()
                .is_native_load_pending()
        );
        for tick in 4..=35 {
            for world in [&mut sim, &mut restored] {
                world.advance_tick(&[], Some(&rules), Some(&grid), None, HARNESS_TICK_MS);
            }
            assert_eq!(
                sim.state_hash(),
                restored.state_hash(),
                "ROT={rot}, tick={tick}"
            );
            if tick == 4 {
                let entity = sim.substrate.entities.get(1).unwrap();
                assert_eq!(
                    entity
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_drive_runtime())
                        .and_then(|r| r.retained())
                        .unwrap()
                        .head_to()
                        .is_some(),
                    rot == 0
                );
            }
        }
    }
}

#[test]
fn s2_dense_scenario_position_fingerprint_stable() {
    use std::hash::{Hash, Hasher};
    let (mut sim, rules, grid, script) = dense_converging_setup();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for tick in 0..DENSE_TICKS {
        let due: Vec<CommandEnvelope> = script
            .iter()
            .filter(|(t, _, _)| *t == tick + 1)
            .map(|(t, owner, c)| CommandEnvelope::new(*owner, *t, c.clone()))
            .collect();
        let _ = sim.advance_tick(&due, Some(&rules), Some(&grid), None, HARNESS_TICK_MS);
        for (id, e) in sim.substrate.entities.iter_sorted() {
            (
                id,
                e.position.rx(),
                e.position.ry(),
                e.position.sub_x(),
                e.position.sub_y(),
            )
                .hash(&mut h);
        }
    }
    assert_eq!(
        h.finish(),
        POSITION_FINGERPRINT,
        "committed per-tick position sequence changed; attribute the first divergence before updating"
    );
}
