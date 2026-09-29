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
//! handshake needs movement world-setup (terrain costs / resolved terrain) that
//! the dedicated miner-dock suite (`miner_tests.rs`) provides and owns; this
//! harness only guards that the miner system stays wired and deterministic.

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use std::collections::BTreeMap;

const HARNESS_SEED: u64 = 0xC0FFEE_1234;
const HARNESS_TICKS: u64 = 600;
const HARNESS_TICK_MS: u32 = 67;
/// AT-8: ticks at which the per-stream RNG cursors are compared record-vs-replay
/// (after the tick at this index executes).
const STREAM_CHECKPOINT_TICKS: &[u64] = &[149, 299, 449, 599];

/// AT-8 proper: ABSOLUTE committed per-stream fingerprints at the final
/// checkpoint (tick 599). Record-vs-replay equality alone cannot catch a
/// deterministic cross-stream misroute — both passes run the same code, so a
/// misrouted draw appears identically in both. Only committed values detect
/// it, and when a legitimate change shifts the total hash, these localize
/// WHICH stream moved. Same re-baseline ceremony as GLOBAL_HARNESS_FINAL_HASH
/// (one documented re-baseline per behavior-bearing change; paste the failing
/// `left` values).
/// Baselined at SC-2 review hardening. scenario == main here: this scripted
/// scenario consumes ZERO draws from either gameplay stream (they stay at the
/// identical post-seed state), and MapGen holds the fresh native Seed(0)
/// fingerprint — so ANY future draw in this scenario shifts exactly one
/// component loudly.
/// Re-baselined after MapGen was split from the scenario seed. The new MapGen
/// value was identical in two focused runs with pristine fresh Seed(0) MapGen.
/// This remains a Rust regression ratchet, not a gamemd parity reference.
/// Re-baselined with the tube-gate fix (off-tube non-adjacent path steps are
/// no longer killed as failed tube traversals): the harness harvester's
/// sharp-turn outbound legs now execute instead of dying on their issue tick,
/// so it reaches ore and Reduce_Tiberium's growth reseeding consumes scenario
/// draws this fixture never reached before. Streams 1 and 2 are unchanged and
/// the total hash moved with stream 0 — a behavior-bearing shift, not a
/// misroute.
/// Re-baselined with the native Mission_Harvest per-path dispatch delays:
/// the return/idle/still-driving handler exits now draw the native
/// RandomRanged(0,2) Rate-epilogue jitter on the scenario stream, so this
/// fixture's harvester consumes scenario draws on every non-productive
/// dispatch. Streams 1 and 2 are unchanged and the total hash moved with
/// stream 0 — a behavior-bearing shift, not a misroute.
/// Re-baselined for the Phase-0 native-frame authority: 600 admitted visits now
/// commit exactly 600 frames. The former 67-ms-derived clock skipped three
/// frame values, changing frame-anchored Harvest dispatch jitter draws.
/// Main and MapGen remain unchanged, localizing the intended shift to Scenario.
/// Re-baselined 2026-08-02 for passive/opportunity target acquisition: every
/// object that passes the gate now draws one `RandomRanged(0, 2)` on the
/// SCENARIO stream when its scan timer expires, roughly every 27-29 frames.
/// Which objects that is, in THIS fixture, is narrower than it looks. Measured,
/// per tick, on an instrumented run — stated here as observations, with the
/// causes marked where they were not established:
///
/// - The Allied MTNK (id 4) holds `mission.current() == AttackMove` and
///   `order_intent == Some(AttackMove)` continuously from roughly tick 45 to the
///   end of the run, across ticks 300 and 320. AttackMove is not one of the
///   three missions the gate admits, so id 4 never scans — identically before
///   and after the finished-mission bridge.
/// - The Stop and Move envelopes scripted for id 4 at 300 and 320 ARE delivered
///   and report `executed_commands == 0`, leaving its mission and order intent
///   untouched. **Why they have no effect is UNCHECKED** — it is a property of
///   this fixture, not of this change, and it is the reason the earlier claim
///   here (that those handlers clear the order intent) did not match what the
///   run actually does.
/// - `due_commands` issues every scripted command under the Allied owner, so the
///   Soviet MTNK's tick-120 Move does not move id 6 off the `NONE` selector.
///
/// So the objects that actually scan here are the Soviet MTNK (id 6) and both
/// E1 riflemen (ids 5 and 7) — all three sitting on the `NONE` selector, which
/// already read as Guard before the bridge — plus the harvester's own Harvest
/// mission. Nothing in this fixture is ordered-then-idle, which is why the
/// bridge left every pin here untouched.
/// Streams 1 (Main) and 2 (MapGen) are byte-identical to the previous
/// baseline, which is the proof that the new draw is routed to the scenario
/// instance and to no other — a lone stream-0 shift is the expected signature
/// here, and a shift in either other component would have been a misroute.
///
/// A SECOND scenario-stream source went live in the same slice and is part of
/// this shift: the pointer-expiry path already drew `RandomRanged(4, 8)` to
/// shorten a listener's passive-scan timer when its current target died, gated
/// on that timer having more than 10 frames left. That draw was unreachable in
/// production because the timer was always the zero-duration sentinel; arming it
/// at construction makes the gate satisfiable, so it now fires on target deaths
/// throughout the run. It is deterministic and on the same stream, so the
/// re-baseline stands — but the per-scan jitter draw is not the whole story.
/// Re-measured in the same slice when the passive block was extended to the
/// Infantry leaf (it reaches the common Techno AI body through the same foot
/// call the Unit leaf does). This fixture's two E1 riflemen now scan on the
/// same cadence, adding their draws. Streams 1 and 2 are still byte-identical
/// to the pre-slice baseline.
/// Re-baselined 2026-08-04 for the GSI-07.02 constructed-`Rate` default (0 ->
/// 0.016 min = 14 frames, the value gamemd's MissionControl ctor stores when a
/// `[<MissionName>]` section or its `Rate=` key is absent). `harness_rules()`
/// declares no mission sections at all, so every mission in this fixture moved
/// off the zero sentinel and the per-object dispatch timer now arms on a
/// different schedule. Streams 1 and 2 below are byte-identical to the previous
/// baseline -- only stream 0, the scenario stream the cadence jitter draws
/// from, moved -- and the intra-run determinism assertion still passes, so this
/// is a changed schedule, not an RNG misroute. No draw site was added or
/// removed.
/// Re-baselined 2026-08-11 after this fixture stopped using the since-removed
/// per-cell resource node stand-in and installed the production `OverlayGrid` plus
/// Tiberium rules on both record and replay. The harvester now reaches the
/// native overlay authority and consumes the Scenario draws owned by that
/// path. Main and MapGen remain byte-identical, and record/replay equality
/// remains exact, localizing the intended change to Scenario.
/// Re-baselined 2026-08-15 for `167527ac`: this fixture reaches a natural
/// terminal edge, where sim now latches the preserved Rust score projection
/// before the returned hash and consumes its victory-bonus draw from Scenario.
/// Main and MapGen remain byte-identical and record/replay equality remains
/// exact. The native bonus formula and score traversal remain UNCHECKED.
/// Re-baselined for TechnoClass::TechnoClass @ 0x006F2B90: seven authored
/// Technos now consume the raw Scenario words stored at 0x006F3254. Only
/// Scenario moves; Main and MapGen plus tick-for-tick record/replay remain exact.
/// Re-baselined 2026-09-05 for GSI-09.03 harvest bite size: `Harvest_Ore_Tick`
/// @ 0x0073D450 requests `ftol(min(1.0f, Storage - total))` = one density level
/// per 19-frame gate (was the whole free capacity, draining a cell per gate).
/// The harness harvester fills ~36 gates later, so its return/dock Scenario
/// draws land on different frames. Only Scenario moves; Main and MapGen plus
/// tick-for-tick record/replay remain exact.
// 2026-09-13: first changed Scenario draw follows the live NavCom guard at
// tick116 after corrected TrackProcess payment; Main/MapGen remain unchanged.
// See TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md for the two raw draw values.
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
    0xCA99_BA6D_18DC_11B1,
    0x39F3_258B_A550_EB7C,
    0x1CE8_1848_7043_6163,
);

// 2026-09-23 Drive/Ship Process path request (behavior, not composition):
// Drive/Ship orders from every producer (player, pursuit, rally, miners) are
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
const GLOBAL_HARNESS_FINAL_HASH: u64 = 0x501E_B454_CEF9_365B;

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
         [E1]\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
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
    RuleSet::from_ini(&ini).expect("harness rules should parse")
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

/// Build the recorded scenario into `sim`. Spawn order fixes stable ids
/// 1..=7 (war factory, refinery, harvester, Allied tank, Allied infantry,
/// Soviet tank, Soviet infantry).
fn seed_scenario(
    sim: &mut Simulation,
    rules: &RuleSet,
    heights: &BTreeMap<(u16, u16), u8>,
    overlays: &OverlayTypeRegistry,
) {
    // Flat map cells and the playfield, as a map load installs them before
    // placing objects: Unlimbo's Unit `Can_Enter_Cell` reads both, and the
    // harvester's ore scan (`FootClass::Is_Cell_Harvestable @ 0x004DCE80`)
    // admits only playfield cells of LandType 5.
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(64));
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -64,
        off_100: -1,
        off_104: 128,
        off_108: 65,
    });
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
    sim.spawn_from_map(
        &[
            unit("Americans", "GAWEAP", 3, 3, EntityCategory::Structure), // 1
            unit("Americans", "GAREFN", 3, 10, EntityCategory::Structure), // 2
            unit("Americans", "HARV", 8, 12, EntityCategory::Unit),       // 3
            unit("Americans", "MTNK", 10, 8, EntityCategory::Unit),       // 4
            unit("Americans", "E1", 11, 9, EntityCategory::Infantry),     // 5
            unit("Soviet", "MTNK", 40, 8, EntityCategory::Unit),          // 6
            unit("Soviet", "E1", 41, 9, EntityCategory::Infantry),        // 7
        ],
        Some(rules),
        heights,
    );
    // Seed the native CellClass overlay authority near the harvester.
    let tib01 = overlays.id_for_name("TIB01").expect("harness TIB01");
    let mut overlay_grid = OverlayGrid::new(64, 64);
    let terrain = sim.resolved_terrain.as_mut().expect("installed above");
    for (rx, ry) in [(12, 13), (13, 13), (12, 14), (13, 14)] {
        overlay_grid.place_overlay(rx, ry, tib01, 11);
        // RecalcAttributes: LandType 5 and its [Tiberium] speed row.
        overlay_grid.recalculate_runtime_cell(
            terrain,
            overlays,
            (rx, ry),
            crate::sim::overlay_grid::NavigationPublication::FrameBoundary,
        );
    }
    overlay_grid.take_dirty_cells();
    sim.overlay_grid = Some(overlay_grid);
}

/// Scripted commands keyed by `execute_tick` (fires when tick+1 == execute_tick).
fn harness_script() -> Vec<(u64, Command)> {
    vec![
        (
            2,
            Command::Move {
                entity_id: 4,
                target_rx: 24,
                target_ry: 8,
                queue: false,
            },
        ),
        (
            40,
            Command::AttackMove {
                entity_id: 4,
                target_rx: 38,
                target_ry: 8,
                queue: false,
            },
        ),
        (
            120,
            Command::Move {
                entity_id: 6,
                target_rx: 28,
                target_ry: 10,
                queue: false,
            },
        ),
        (300, Command::Stop { entity_id: 4 }),
        (
            320,
            Command::Move {
                entity_id: 4,
                target_rx: 8,
                target_ry: 8,
                queue: false,
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
fn global_skirmish_replay_is_deterministic_and_baseline_stable() {
    let rules = harness_rules();
    let overlays = harness_overlays();
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let grid = PathGrid::new(64, 64);
    let script = harness_script();

    // ---- Record pass: build a ReplayLog through the live advance_tick path. ----
    let mut rec = Simulation::with_seed(HARNESS_SEED);
    seed_scenario(&mut rec, &rules, &heights, &overlays);
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
    let mut miner_engaged = false;
    // AT-8 stream pins: per-stream cursor fingerprints captured at checkpoint
    // ticks during record, re-asserted in replay. Total-hash equality can mask
    // a draw routed to the wrong stream when a compensating error exists;
    // per-stream checkpoints catch misrouting directly.
    let mut recorded_streams: Vec<(u64, u64, u64, u64)> = Vec::new();
    let mut first_uncommitted_frame = None;
    for tick in 0..HARNESS_TICKS {
        let due = due_commands(&rec, &script, tick);
        let result = rec.advance_tick(
            &due,
            Some(&rules),
            &heights,
            Some(&grid),
            Some(&overlays),
            HARNESS_TICK_MS,
        );

        if !result.frame_committed {
            first_uncommitted_frame.get_or_insert(tick);
        }

        if rec.substrate.entities.get(3).is_some_and(|h| {
            h.miner.as_ref().is_some_and(|m| m.harvesting) || h.navigation.nav_com.is_some()
        }) {
            miner_engaged = true;
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
    // The movement pass keeps its owner block sets current from the entity
    // store's touch log. Anything that hands out every entity mutably each
    // frame (`values_mut`) would quietly turn that back into a whole-world
    // read per frame, with correct results and no other symptom.
    let world_reads = rec.movement_pass_cache.block_index_world_rebuilds();
    // The blocker plane follows the same log. It is rebuilt from the whole map
    // only when the terrain epoch or the wall plane moves; this fixture's grid is
    // a legacy one without a retained wall plane, so every overlay write
    // counts, which this script does a couple of dozen times; a rebuild per
    // moving object's turn would be thousands.
    let plane_reads = rec.movement_pass_cache.blocker_plane_world_rebuilds();
    assert!(
        plane_reads <= 60,
        "the blocker plane was rebuilt from the whole map {plane_reads} times"
    );
    assert!(
        world_reads <= 2,
        "the block index read every entity {world_reads} times; look for a new all-entity mutable walk"
    );
    assert!(
        miner_engaged,
        "the miner system must engage the harvester (head for or cut ore) — \
         else miner-component creation or Mission_Harvest state 0 regressed"
    );

    // ---- Replay pass: fresh sim, real ReplayRunner, assert tick-by-tick.
    // The registry-aware entry uses the SAME master-frame path as the legacy
    // convenience entry, chunked at the stream checkpoints so the per-stream
    // cursors can be pinned between chunks. ----
    let mut rep = Simulation::with_seed(HARNESS_SEED);
    seed_scenario(&mut rep, &rules, &heights, &overlays);
    let mut replayed: Vec<u64> = Vec::with_capacity(log.ticks.len());
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
            &heights,
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
            &heights,
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
    let final_hash = *replayed.last().expect("at least one tick recorded");
    println!(
        "[global parity] final_hash={final_hash:016X} \
         streams={final_scen:016X},{final_main:016X},{final_mapgen:016X} \
         first_uncommitted_frame={first_uncommitted_frame:?}"
    );
    assert_eq!(
        (final_scen, final_main, final_mapgen),
        FINAL_STREAM_STATES,
        "AT-8 absolute per-stream pin at tick 599: a stream's committed \
         fingerprint moved. If a real behavior change shifted it, re-baseline \
         ONCE with a one-line documented reason (paste this `left` tuple into \
         FINAL_STREAM_STATES); the shifted component tells you WHICH stream \
         consumed differently — a lone shift in one stream with an unchanged \
         total-hash baseline is a misroute, never a re-baseline."
    );

    // Schema166 and older final projections folded an independently mutable
    // detached curve. Their archived receipts (removed after 148327c3) cannot
    // be regenerated from an active retained class. Current full-hash, actual
    // replay, terminal coverage, miner engagement and unchanged absolute RNG
    // pins remain gates.
    for id in rep.substrate.entities.keys_sorted() {
        let entity = rep.substrate.entities.get(id).unwrap();
        println!(
            "[global owner] id={id} cell=({},{}) sub=({},{}) health={} mission={:?} queued={:?} nav={:?} path={:?} drive={:?} speed={:?}",
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
    // Tank 4's attack-move acquires Soviet tank 6 at tick 281 and fires; tank 6
    // retaliates. After the Stop (300) and the Move home (320) a hit from tank
    // 6 turns tank 4 back (ShouldRetaliate 0x007087C0: no Target, and Move
    // keeps the constructor's Retaliate=yes). On the fixture's map cells
    // (2026-09-25; see GLOBAL_HARNESS_FINAL_HASH) tank 4 takes its seventh hit
    // and dies at tick 586..590. The same-call track continuation this block
    // used to follow is covered by track_path_continuation_tests on
    // production rows.
    assert_eq!(
        rep.substrate
            .entities
            .get(4)
            .map(|tank| tank.health.current),
        None,
        "the retasked tank dies in its duel"
    );
    assert_eq!(
        rep.substrate
            .entities
            .get(6)
            .map(|tank| tank.health.current),
        Some(12),
        "tank 6 takes six 105mm hits (65 * 75% heavy)"
    );

    assert_eq!(
        final_hash, GLOBAL_HARNESS_FINAL_HASH,
        "committed global-harness baseline drifted. Do not copy the observed value: \
         first prove whether behavior, RNG routing, or intentional hash composition \
         changed, and document reproducible baseline provenance"
    );
}

const DENSE_SEED: u64 = 0x00BA771E_5EED;
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
/// ordered-then-idle — the case the finished-mission bridge releases back to
/// Guard — so they now acquire each other on arrival, fire, and kill. The
/// position fingerprint below therefore covers engagement churn as well as
/// arrival churn.
/// Shared construction for the dense converging-battle fixture (20 tanks, two
/// facing columns converging on x=25; per-owner Move script due on tick 2).
/// Used by the churn measurement and the S2 position fingerprint below.
#[allow(clippy::type_complexity)]
fn dense_converging_setup() -> (
    Simulation,
    RuleSet,
    BTreeMap<(u16, u16), u8>,
    PathGrid,
    Vec<(u64, crate::sim::intern::InternedId, Command)>,
) {
    let rules = harness_rules();
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let grid = PathGrid::new(64, 64);

    let mut sim = Simulation::with_seed(DENSE_SEED);
    let mut roster: Vec<MapEntity> = Vec::new();
    for i in 0..DENSE_ROWS {
        roster.push(unit("Americans", "MTNK", 10, 5 + i, EntityCategory::Unit));
        // ids 1..=10
    }
    for i in 0..DENSE_ROWS {
        roster.push(unit("Soviet", "MTNK", 40, 5 + i, EntityCategory::Unit)); // ids 11..=20
    }
    sim.spawn_from_map(&roster, Some(&rules), &heights);

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
                queue: false,
            },
        ));
        script.push((
            2,
            soviet,
            Command::Move {
                entity_id: 11 + i,
                target_rx: 25,
                target_ry: y,
                queue: false,
            },
        ));
    }
    (sim, rules, heights, grid, script)
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
/// written up at `FINAL_STREAM_STATES`.
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
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/drive_fresh_turn.json"
    ))
    .unwrap();
    for rot in [0, 5] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\nROT={rot}\n\
             Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(DENSE_SEED);
        let heights = BTreeMap::new();
        let grid = PathGrid::new(64, 64);
        sim.spawn_from_map(
            &[unit("Americans", "MTNK", 40, 5, EntityCategory::Unit)],
            Some(&rules),
            &heights,
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
                        queue: false,
                    },
                )]
            } else {
                Vec::new()
            };
            sim.advance_tick(
                &due,
                Some(&rules),
                &heights,
                Some(&grid),
                None,
                HARNESS_TICK_MS,
            );
        }
        let entity = sim.substrate.entities.get(1).unwrap();
        let call = &native["calls"][0];
        assert_eq!(
            u64::from(entity.body_facing_byte(sim.session.binary_frame - 1)),
            call["sampled_after"].as_u64().unwrap() >> 8,
            "ROT={rot}: same-frame native sample"
        );
        let drive = entity.drive_locomotion.as_ref().unwrap();
        assert_eq!(
            drive.track.turn_index, -1,
            "turn must return before admission"
        );
        assert!(drive.head_to.is_none());
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
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        for tick in 4..=35 {
            for world in [&mut sim, &mut restored] {
                world.advance_tick(
                    &[],
                    Some(&rules),
                    &heights,
                    Some(&grid),
                    None,
                    HARNESS_TICK_MS,
                );
            }
            assert_eq!(
                sim.state_hash(),
                restored.state_hash(),
                "ROT={rot}, tick={tick}"
            );
            if tick == 4 {
                let entity = sim.substrate.entities.get(1).unwrap();
                assert_eq!(
                    entity.drive_locomotion.as_ref().unwrap().head_to.is_some(),
                    rot == 0
                );
            }
        }
    }
}

#[test]
fn s2_dense_scenario_position_fingerprint_stable() {
    use std::hash::{Hash, Hasher};
    let (mut sim, rules, heights, grid, script) = dense_converging_setup();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for tick in 0..DENSE_TICKS {
        let due: Vec<CommandEnvelope> = script
            .iter()
            .filter(|(t, _, _)| *t == tick + 1)
            .map(|(t, owner, c)| CommandEnvelope::new(*owner, *t, c.clone()))
            .collect();
        let _ = sim.advance_tick(
            &due,
            Some(&rules),
            &heights,
            Some(&grid),
            None,
            HARNESS_TICK_MS,
        );
        for (id, e) in sim.substrate.entities.iter_sorted() {
            (
                id,
                e.position.rx,
                e.position.ry,
                e.position.sub_x,
                e.position.sub_y,
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
