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
    0x670E_F1DD_13DA_B92B,
    0x39F3_258B_A550_EB7C,
    0x1CE8_1848_7043_6163,
);

// Immutable pre167 receipt from5777c115. Its active detached curve cannot be
// reconstructed after the authority migration. These constants and comments
// describe that historical execution; they are not current-state projections.
// Keep the receipt distinct from the current full-hash and absolute RNG gates.
#[rustfmt::skip]
mod schema166_receipt {
/// Committed final-hash baseline. Captured from the first green run. Re-baselines
/// at most once per behavior-bearing change, with a one-line documented reason.
/// Baselined for Slice 8 (initial commit of the global parity harness).
/// S2 (dispatch-time mission authority) left this UNSHIFTED — verified empirically:
/// this scenario's movers are engaged or miners (never pure-Move scoped) on their
/// divergence ticks, so tail authority still wrote every hashed mission value. The
/// S2 hash delta is exercised by the arrival-tick tests in techno_ai.rs instead.
/// S3 facing flip (per-object pre-death barrel read) ALSO left this unshifted —
/// no Unit kill/retarget tick changes a barrel destination in this scenario.
/// Re-baselined ONCE for S3 idle→Guard: every idle machine-less Unit now hashes
/// mission Guard(5) instead of the legacy None placeholder (the gamemd idle
/// selector for ground vehicles) — a hashed-representation fidelity fix, not a
/// behavior drift; movement/combat outputs are byte-identical.
/// Re-baselined for SC-2: session identity (seed, map name, theater, bounds,
/// MP start table, slot->house) folded into the state hash — every absolute
/// hash shifts once by composition; the tick-by-tick rec-vs-replay equality
/// and the per-stream cursor pins prove no behavioral movement.
/// Re-measured at the S3 × SC-2 merge (both deltas combined; value from the
/// merged tree's green run — neither side's pre-merge value can be correct).
/// Re-baselined for S4b: the hashed `damage_particle_live_until` `+0x308`-
/// equivalent field folds an extra 0 per entity — a composition shift, NOT a
/// behavior drift. Proven: with the fold line disabled this baseline held its
/// prior value (so S4b moved zero RNG and changed no committed scenario), and
/// the tick-by-tick rec-vs-replay equality below still passes.
/// W1 (mission-cadence: G5/G6/L20 + L9/L10 RandomRanged(0,2) draws) left this
/// UNSHIFTED — verified empirically (this baseline + the per-stream cursor pins
/// held their values, and the harness runs deterministically 2×). The harness
/// harvester (id 3) acquires an ore target but never completes the refinery dock
/// handshake — the coverage tripwire below only asserts ore-target acquisition,
/// and the dock/unload cadence paths that carry the new draws are never reached
/// in this scenario. The W1 cadence + RNG-draw determinism is covered by the
/// dedicated miner-dock suite (accepted_face_sync_handoff_draws_one_scenario_rng,
/// state_four_exit_draws_and_applies_resume_jitter, et al.) instead.
/// Re-baselined after MapGen became an independent fresh Seed(0) stream. The
/// value was identical in two focused runs; this is a Rust regression ratchet,
/// not a gamemd parity reference.
/// The later aircraft-RTB rationale was invalid: this fixture contains no
/// aircraft. A pristine `fafc0ba5` run reproduced the preceding
/// `7340892273004731329` baseline, proving the committed replacement was captured
/// from a contaminated worktree.
/// Re-baselined for lockstep hash completeness: body-facing presence,
/// damage-fire state/animation IDs, locomotor hover/altitude state, per-house
/// difficulty, Spark state, and `AnimStore` now join the hash. A current-tree
/// legacy-schema probe reproduced `7340892273004731329` exactly; record/replay
/// tick equality and the absolute RNG pins also remained unchanged. This shift
/// is therefore composition-only and remains a Rust regression ratchet, not
/// gamemd parity evidence.
/// Re-baselined for snapshot/hash schema v28: independent lifecycle axes,
/// lifecycle bookkeeping, and ordered pending deletion now join the hash. The
/// current-tree legacy-schema probe reproduces the prior value, record/replay
/// equality remains exact, and all three absolute RNG pins are unchanged.
/// Re-baselined after the reviewed outbound and far-return miner Drive
/// authority changes (`932fc5e8`, `3ff8f43c`). Parent/child isolation proved
/// that each change moved this fixture's hashed navigation/Drive state while
/// record/replay equality and all three absolute RNG pins stayed unchanged.
/// This is a behavior-bearing Rust regression ratchet, not gamemd parity
/// evidence.
/// Re-baselined for the Mission authority flip: MissionCom is verb-owned
/// (commands queue via the event-execute shape; the object-AI host ticks
/// `+0xC4` for every live category and promotes queued missions
/// Ready→Commence; the per-tick legacy projection is deleted). Every hashed
/// mission field changes value — including under the legacy pre-v29
/// composition, which folds the reduced mission subset — so all three
/// constants shift together while record/replay equality and all three
/// absolute RNG pins stay unchanged (the verbs draw nothing).
/// Re-baselined for the Harvest handler absorption (A1): the miner FSM now
/// dispatches from the per-object AI host BEFORE Phase-1 ground movement (the
/// native handler→locomotion order) instead of the late production phase, the
/// FSM cursor moved from the hashed miner block into
/// `MissionCom.handler_state`, and every miner's dispatch timer advances per
/// dispatch (post-handler epilogue write). This shifts the harness harvester's
/// hashed mission/miner state under every schema composition — including the
/// legacy reconstructions — so all three constants move together. All three
/// absolute RNG stream pins and record/replay tick equality held unchanged
/// (this scenario's miner never docks, so no draw moved).
/// Re-baselined with the native same-tick drive-arrival owner clear: the
/// harness harvester's NavCom now clears on the arrival tick itself (no
/// deferred pass), so its dispatch resumes one tick earlier — a
/// behavior-bearing timing shift in hashed navigation/mission/position
/// state. All three absolute RNG stream pins held their values (the arrival
/// clear draws nothing), and record/replay tick equality still holds.
/// Re-baselined with the native Mission_Harvest per-path dispatch delays:
/// a behavior-bearing shift (dispatch cadence + scenario-stream draws), so
/// the legacy-schema probes move together with the live hash.
/// Re-baselined for the Phase-0 native-frame and persistence authority changes
/// documented at `FINAL_STREAM_STATES`: the admitted-frame cadence changes the
/// harness harvester's behavior, while the common hash composition also drops
/// `total_sim_ms`, hashes only the retail-persisted Scenario RNG, and adds the
/// newly persisted deterministic fields. Therefore both legacy-schema probes
/// and the live hash move together.
// Re-baselined 2026-08-02. Provenance: the mover is 190490ba "match retail cell
// occupation lifecycle", identified by bisecting dev..HEAD against the slice6
// pre-v28 probe. That commit adds src/sim/occupancy.rs (+771) and rewrites the
// substrate/snapshot cell-occupation model, so hashed STATE CONTENT changed.
//
// Not composition-only, and proven so rather than assumed: swapping in the
// merge-base (6f78bac7) world_hash.rs while keeping all branch behaviour put
// this probe at 0x89FC9D5B5BFDC1F2 — a third value, matching neither the old
// baseline nor the branch. RNG routing is unchanged: FINAL_STREAM_STATES passes
// untouched, and record/replay cursor consistency plus the per-tick intra-run
// hash asserts all pass.
// MERGE 2026-08-03: both branches re-baselined these independently (dev:
// passive acquire + spawner; foundations: Move cadence + hashed runtime
// state). Neither side's values describe the merged tree; re-derived below
// from the merged tree's own output in the same merge commit.
// Native Move mission cadence now mutates existing MissionCom and Scenario RNG.
// Re-baselined after hashing the newly persisted YR runtime-contract state.
// Re-baselined 2026-08-02 for passive/opportunity target acquisition. BOTH
// probes move here, and that is the correct outcome rather than a warning
// sign: this change is behaviour-bearing by construction. The two E1 riflemen
// (ids 5 and 7) are never ordered at all, so they scan from frame 45 for the
// whole run and engage whatever comes into range; the Soviet MTNK (id 6) sits
// unordered until its tick-120 Move. New targets, new fire events, new deaths —
// every position, facing and health field the legacy compositions fold changes
// with them.
//
// The composition half of the delta is separately isolated and is behaviour-
// free: `slice6_retask_tests` gained the same three hashed fields
// (`passive_scan_timer`'s armed-at-construction value plus the new
// `last_target_scan_frame` / `passively_acquired_target`), all inside the v29
// block, and BOTH of its legacy probes reproduced their committed values
// exactly while only its current-schema hash moved. That fixture runs 16 ticks,
// short of the 45-frame first-scan delay, so no scan fires in it at all. So the
// new fields contribute composition only, and everything moving here is
// behaviour.
//
// Record/replay tick-by-tick equality and the per-stream cursor consistency
// check both still pass, and only stream 0 moved. Rust regression ratchet, not
// gamemd parity evidence.
//
// Re-measured in the same slice when the passive block was extended to the
// Infantry leaf: this fixture's Allied E1 (id 5) and Soviet E1 (id 7) now
// acquire on their own too, so the behaviour delta widens. `slice6_retask_tests`
// still reproduces BOTH of its legacy probes unchanged across the whole slice —
// its 16-tick fixture never reaches the first scan — so the composition half
// remains isolated and behaviour-free, and everything moving here is behaviour.
//
// Re-baselined 2026-08-05 for the Drive cell-admission slice, and this time the
// attribution is MEASURED rather than argued. `DriveLocomotionRuntime` gained
// `occupation_handoff: Option<DriveOccupationFootprint>`, and the whole struct
// is hashed by its derived `Hash` (`world_hash.rs`, `entity.drive_locomotion`),
// so an `Option` discriminant enters the fold for every vehicle even while the
// field is `None`.
//
// The experiment, run in this tree with no `world_hash.rs` change: KEEP the
// field (so the schema delta is fixed), neutralise every behaviour writer that
// landed with it — the fresh-selection admission gate, the chained-curve refusal
// on the two temporary codes, the handoff mark at both install sites, and the
// forced-track pre-clear — and re-run this fixture. Result, byte-identical to
// the full change on every printed value:
//
//     final_hash=F221E97E407676CA  pre-v28=5E01CF58F7998106  pre-v29=56B366E67991E8A4
//     streams=2FE2AAAE97044CEC,39F3258BA550EB7C,1CE8184870436163
//
// So this fixture's entire shift is SCHEMA and none of it is behaviour: the
// harness's one miner and small unit count never reach the admission lane.
// `FINAL_STREAM_STATES` is byte-identical across all three streams, which —
// because the generator advances as a pure function of its own state — proves
// the DRAW COUNT per stream is unchanged, not the schedule; the intra-run
// determinism assertion passes on top of that. Still a Rust-vs-prior-Rust
// regression ratchet, not gamemd parity evidence.
// Re-baselined 2026-08-11 with the production OverlayGrid/Tiberium fixture.
// Both legacy-schema probes deliberately hash production and overlay state, so
// they move with the same measured authority correction as the current hash.
// Record/replay equality and the Scenario-only stream shift remain the guards
// against nondeterminism or cross-stream routing.
// Re-baselined 2026-08-11 for the v69 combined serialized/hash substrate. The
// exact Scenario/Main/MapGen stream tuple below remains byte-identical and the
// record/replay comparison remains exact, proving this shift is composition,
// not RNG routing or committed simulation behavior.
// Re-baselined 2026-08-13 after GSI-05.01 consolidated ProjectileStore,
// WaveStore, and ProductionState allocation into the already-hashed global
// `ObjectSubstrate::next_stable_object_id`. Removing the three obsolete local
// counter folds moves every hash composition; the exact stream tuple and every
// record/replay tick comparison remain unchanged.
// Re-baselined 2026-08-14 for the v77 GSI-13.06 SHP body-cadence state.
// The persisted body counter and signed Drive/Ship owner-speed/runtime bytes
// are folded outside the old v28/v29 blocks, so both named probes must include
// them. Record/replay remained equal at every tick and FINAL_STREAM_STATES is
// byte-identical, ruling out nondeterminism or an RNG-stream routing change.
// Re-baselined 2026-08-15 for the terminal-score Scenario draw documented at
// `FINAL_STREAM_STATES`. These probes exclude the v44 animation and v46 score-
// snapshot blocks but deliberately retain Scenario RNG, so both move with that
// one-stream behavior change. Record/replay remained exact; Main and MapGen did
// not move. The native bonus formula and score traversal remain UNCHECKED.
// Re-baselined 2026-08-19 for the GSI-08.12 veterancy accumulator: every object
// now carries the raw `VeterancyClass` float (as bits) and the rank cache, and
// both are hashed, so the state hash moves on composition alone. FINAL_STREAM_STATES
// is byte-identical across all three streams — the award consumes no RNG, which is
// what `VeterancyClass::Add @ 0x0074FF50` does — and record/replay stayed equal at
// every tick, ruling out both nondeterminism and a routing change.
// Re-baselined for TechnoClass::TechnoClass @ 0x006F2B90: all seven authored
// Technos consume the raw Scenario words stored at 0x006F3254. That
// behavior-bearing Scenario shift reaches both historical schema probes;
// record/replay remains exact and Main plus MapGen remain unchanged.
// Re-baselined 2026-08-30 for GSI-04.03 Drive/Ship slope payload ownership.
// The payload is part of the common locomotor fold, so both historical probes
// move. All three RNG fingerprints and tick-for-tick record/replay remain exact,
// isolating this to the intentional hash-owner composition change.
// Re-baselined 2026-09-02 for the native tiberium queue store (OQ-38, bridge transaction 3
// slice D): every class now carries the native entry array, float min-heap, capacity, and
// `native_rect`, rebuilds walk `CellIterator` order, and spread admission applies the
// `FirstObject` occupier gate. This is behavior-bearing on every fixture with ore, so the
// historical probes move as well; the RNG stream tuple and tick-for-tick record/replay
// equality remain exact.
// 2026-09-02 veterancy effects (GSI-08.12): `TechnoClass+0x13C` is now written by the
// first `AI_Update` promotion sample (-1 -> GetVeterancyLevel code), so every live
// object's hashed `veterancy_rank_cache` moved. Composition of the hash is unchanged
// and the RNG stream pins held (FINAL_STREAM_STATES), so no cadence or draw moved.
// Merge 2026-09-02: main's veterancy re-baseline and this branch's queue-store
// re-baseline both move the same constants, so the values below are the composed
// measurement taken on the merged tree, not either side's number.
// Re-baselined 2026-09-03 for GSI-08.01, native target acquisition: the passive
// scan is now `TechnoClass::Greatest_Threat @ 0x006F8DF0` -- an expanding-ring
// cell walk, one candidate per cell, ranked by
// `TechnoClass::Calculate_Threat_Score @ 0x0070CD10` and cut short at the first
// band that finds anything -- in place of VERA's own
// `(distance^2, threat_class, stable_id)` nearest-first key, and the human
// building gate at `TechnoClass::Evaluate_Candidate @ 0x006F85AB` now refuses an
// enemy building that has no weapon or `ThreatPosed=0`. Objects in this fixture
// therefore engage different victims from the first contact on, which moves
// positions, health and mission state for the rest of the run.
// Ceremony: `FINAL_STREAM_STATES` passes UNCHANGED -- all three streams
// byte-identical -- so no draw was added, removed or rescheduled (the scan takes
// its one timer-jitter draw either way, and the native evaluation's own
// `RandomRanged(0, 99)` at `0x006F8525` is unreachable in YR). Record/replay
// tick equality holds. This is a target-choice shift, and all seven constants
// below move with it.
// Re-baselined 2026-09-05 for GSI-09.03 harvest bite size (see
// `FINAL_STREAM_STATES`): `Harvest_Ore_Tick` @ 0x0073D450 takes one density
// level per gate, so the harness harvester fills ~36 gates later and its
// return/dock/deposit state at tick 599 differs. Behavior-bearing: Scenario
// moved, Main/MapGen and record/replay equality unchanged; all seven
// constants move together.
// 2026-09-08 ramp-height writers: all existing probes already fold exact Z.
// Only entities 3/4 gain Some(0) in the final state. Clearing ONLY those exact-Z
// values reproduces all eight parent (588f4079) probes; full RNG states are equal.
// No hash formula changed. See RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md, replay provenance.
// 2026-09-12: raw Drive/Ship head delivery intentionally changes these
// current-state projections, which all fold Drive runtime even under older
// hash schemas. Against exact6580e4c8, replacing only the measured mover leaves
// reproduced all ten new hashes; every original baseline pin also passed.
// The global trace first changes curve selection at tick352: the remaining W
// direction adopts Raw6 at entry16-1 with retained budget15, while the old
// physical-path reader keeps Raw5. Raw5[45] and Raw6[16] meet at the same world
// coordinate. Position first differs at354; final movement differs by9 leptons.
// All600 record/replay hashes and all three RNG stream pins still agree.
// The final six differing leaves are headX, Drive/curve cursors and residuals,
// and subcellX. Native same-pass timing remains open; this is a Rust regression
// ratchet, not whole-movement parity. See the bridge walker report's raw-head section.
const GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH: u64 = 0x7501_533B_163D_06EC;
const GLOBAL_HARNESS_PRE_MISSION_V29_HASH: u64 = 0xAC7D_1867_E493_C2EE;
// Snapshot/hash schema v29 originally added the exact Mission/readiness state.
// Its schema shift was composition-only; the later behavior-bearing Drive,
// authority-flip, and Harvest-absorption re-baselines are documented above.
// All remain Rust regression ratchets, not gamemd evidence.
/// Re-baselined with the tube-gate fix (see FINAL_STREAM_STATES): the
/// harvester's outbound Drive moves now survive their issue tick, changing
/// positions, mission/miner state, and scenario-stream consumption in this
/// fixture. Both schema probes shift with it (movement diverges from early
/// ticks). Record/replay tick equality still holds.
/// Re-baselined with the native Mission_Harvest per-path dispatch delays
/// (see FINAL_STREAM_STATES): the harvester's dispatch cadence and
/// scenario-stream consumption changed in this fixture, shifting positions
/// and timers from the first return leg on. Record/replay tick equality
/// still holds.
/// Re-baselined 2026-07-29 for the locomotion S2 readiness producers (twice:
/// Drive/Ship/Teleport/Jumpjet, then Walk and Hover) —
/// **composition-only, proved three ways.** (1) `FINAL_STREAM_STATES` is
/// UNCHANGED, so RNG routing and draw counts are identical. (2) Both
/// legacy-schema probes above are unchanged, so only the current-schema hash
/// moved. (3) Re-running with the readiness gate forced to ignore the produced
/// state — producers still writing, behaviour identical to before the slice —
/// yielded exactly this value, so the deferral change contributes nothing in
/// this fixture. The delta is `mission_ready_state` moving `None → Some` for
/// Drive/Ship/Teleport/Jumpjet. Record/replay tick equality still holds.
///
/// Re-baselined 2026-07-30 when the readiness inputs stopped being stored on the
/// locomotor and became derived at the Mission gate. gamemd's readiness virtual
/// makes a fresh locomotor call at every one of its ~two dozen call sites with no
/// cached per-frame flag on that path, so a per-tick cache served nearly all of
/// them stale state.
///
/// **Composition-only, proved four ways this time.** (1) `FINAL_STREAM_STATES` is
/// UNCHANGED — RNG routing and draw counts identical, so no unit commenced on a
/// different tick. (2) `GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH` unchanged.
/// (3) `GLOBAL_HARNESS_PRE_MISSION_V29_HASH` unchanged — and that probe still
/// hashes every position, facing and movement field, so a timing change would
/// have moved it. (4) Record/replay tick equality still holds. Only the
/// current-schema hash moved, and its delta is exactly the removed readiness
/// bytes leaving the hash.
///
/// This fixture does not cover the paths the change exists for — a same-tick stop
/// followed by a mid-tick queue-and-commence — so "behaviour-neutral here" is a
/// statement about this scenario, not about the engine.
///
/// Re-baselined 2026-07-30 for S3b: the installed LocomotorSlot joins the hash.
/// **Composition-only, proved by neutralisation** — the ceremony this file
/// normally uses cannot decide it, because the locomotor block is hashed
/// unconditionally, so BOTH schema probes move with the live value. Instead the
/// new hash line was commented out and the whole suite re-run: all three
/// constants returned to their previous committed values exactly, so the
/// primary_kind -> slot retype changed no behaviour and no other hashed state,
/// and the entire delta is the one new byte. The absolute per-stream RNG pins
/// and the dense-scenario position fingerprint were unchanged throughout.
/// Re-baselined 2026-07-30 for S5: the locomotor `powered` flag joins the hash.
/// Composition-only, proved by neutralisation (the probe ceremony cannot decide
/// it — the locomotor block is hashed unconditionally, so both probes move with
/// the live value). With the new hash line commented out, all three constants
/// returned to their S3b values exactly, which also proves the three power edges
/// wired in this slice (deploy-begin off, undeploy-complete on, destination-
/// accepted on) changed no other hashed state in these fixtures. The absolute
/// per-stream RNG pins held throughout.
// MERGE 2026-08-03: both branches re-baselined these independently (dev:
// passive acquire + spawner; foundations: Move cadence + hashed runtime
// state). Neither side's values describe the merged tree; re-derived below
// from the merged tree's own output in the same merge commit.
/// Re-baselined 2026-08-02 for passive/opportunity target acquisition — the
/// behaviour-bearing shift documented at `GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH`
/// above. Idle units now engage on their own, so this fixture's committed
/// scenario genuinely changes; both legacy-schema probes move with the live
/// hash, and only the scenario RNG stream moved.
/// Re-measured in the same slice when the passive block was extended to the
/// Infantry leaf — same rationale, same ceremony, still a lone stream-0 shift.
/// Re-baselined 2026-08-04 with FINAL_STREAM_STATES for the same
/// constructed-`Rate` change; see the provenance note there.
/// Re-baselined again 2026-08-04 for the GSI-07.04 base-mission default arm:
/// objects on a mission whose native slot is the 450-frame `Mission_Default`
/// stub now write a dispatch-timer pair that VERA previously left untouched.
/// `FINAL_STREAM_STATES` passes UNCHANGED across this shift -- all three
/// streams byte-identical -- so no draw was added, removed or rescheduled;
/// this is hashed dispatch-timer state only. Attribution measured, not argued:
/// forcing the default arm to write nothing, with every other change in the
/// batch live, turns all six constants green again.
/// Re-baselined 2026-08-04 (third and last time this session) for the Phase 5
/// movement/pathfinding batch: terrain sub-cell occupation, close-on-generation
/// search semantics, whole-cell-list blocker classification, the retail corner
/// smoother, the code-2 grace repath and the Drive rest fraction all change
/// which cells units traverse. `FINAL_STREAM_STATES` passes UNCHANGED across
/// this shift -- all three streams byte-identical -- and the intra-run
/// determinism assertion passes, so no draw was added, removed or rescheduled;
/// this is a route/position change. `POSITION_FINGERPRINT` and the slice6
/// constants also held.
/// Re-baselined 2026-08-04, FOURTH time this session, for the final Phase 5
/// batch. ATTRIBUTION IS COARSE and deliberately recorded as such: three
/// file-disjoint builders landed together, so the shift carries the infantry
/// A* wiring (which made the terrain sub-cell fix reach the search for the
/// first time), the terrain-speed clamp removal and re-ordering, the Move
/// arrival hook, and the Sight=0 reveal gate. No single-cause experiment was
/// run. What IS established: `FINAL_STREAM_STATES` is byte-identical across
/// all three streams and the intra-run determinism assertion passes, so RNG
/// routing is unchanged and this is a route/position shift.
/// A reviewer flagged that four prose-justified re-baselines in one session
/// erodes the ratchet. That criticism is recorded and stands: these constants
/// are a Rust-vs-prior-Rust regression ratchet, not parity evidence, and they
/// need a machine-derived oracle before they can carry more weight than that.
/// Re-baselined 2026-08-05 for the Drive cell-admission slice. Attribution
/// measured, not argued: see the experiment written out at
/// `GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH`. With the new
/// `occupation_handoff` field present and every behaviour writer neutralised,
/// this fixture produces this exact value, so the shift is entirely the hash
/// schema and none of it is behaviour.
/// Re-baselined 2026-08-11 with `FINAL_STREAM_STATES` after the harness gained
/// its production OverlayGrid/Tiberium authority. The changed miner/resource
/// state is replay-identical and the stream movement is Scenario-only.
/// Re-baselined 2026-08-13 for the same GSI-05.01 global-ID composition change
/// documented at the legacy probes above; RNG cursors held and record/replay
/// hash equality remained exact.
/// Re-baselined 2026-08-14 for the same v77 authoritative-state composition
/// documented at the legacy probes above. The absolute RNG tuple remains the
/// committed value and record/replay equality remains exact.
/// Re-baselined 2026-08-14 for v44 entity-animation hash authority. The legacy
/// probes and absolute RNG tuple remain unchanged; record/replay equality proves
/// the measured delta is the intentional hash-schema addition.
/// Re-baselined 2026-08-15 for the combined committed authority changes:
/// `14e096ff` supplies RuleSet-owned animation timing to headless/replay frames,
/// advancing v44 entity-animation state, and `167527ac` latches the terminal
/// score plus its Scenario draw before hashing. The exact stream tuple and
/// record/replay comparisons distinguish the RNG movement from nondeterminism.
/// This remains a Rust regression ratchet, not new gamemd parity evidence.
// Re-baselined 2026-08-19 for the GSI-08.12 veterancy accumulator: every object
// now carries the raw `VeterancyClass` float (as bits) and the rank cache, and
// both are hashed, so the state hash moves on composition alone. FINAL_STREAM_STATES
// is byte-identical across all three streams — the award consumes no RNG, which is
// what `VeterancyClass::Add @ 0x0074FF50` does — and record/replay stayed equal at
// every tick, ruling out both nondeterminism and a routing change.
// Re-baselined 2026-08-20 for the v87 TechnoClass+0x3D5 membership byte. Both
// historical schema probes and all three committed RNG stream fingerprints
// remained byte-identical, isolating this to current-schema hash composition.
// Re-baselined 2026-08-20 for the v88 deposited-sensor presence discriminator.
// This fixture has no sensor-bearing type, so every deposit is absent; both
// historical schema probes, all three RNG streams, and tick-for-tick replay
// equality remained byte-identical. The move is hash composition only.
// Re-baselined 2026-08-24 for snapshot/hash schema v90: current hashing now
// folds the exact serialized real `CellClass+0x140 & 0x1180` value authority
// once behind its schema tag. Both historical probes and all three final RNG
// stream states remain byte-identical, while record/replay remains exact, so
// this is composition-only and does not rebaseline historical provenance.
// Re-baselined 2026-08-26 for snapshot/hash schemas v92-v102: Phase 3 adds
// empty/default base-reservation, House strategy/base-defense, TeamScript VM,
// AIMD/TeamType/TaskForce, and typed AITrigger authority to the current hash.
// Both historical probes and all three final RNG fingerprints remain exact,
// and tick-for-tick record/replay equality passes, proving composition-only.
// Re-baselined 2026-08-26 for snapshot/hash schema v103: the empty
// `active_wave_links` map and empty persisted destroyable-cliff mutation map
// now join the current hash. Both historical probes, all three final RNG
// fingerprints, and tick-for-tick record/replay equality remain exact, proving
// this measured shift is composition-only.
// The current schema additionally folds the persistent Techno constructor
// fields; the historical probes above isolate the shared RNG-behavior shift.
// Re-baselined 2026-08-30 for the same GSI-04.03 slope payload ownership.
// The historical probes move consistently, the absolute RNG tuple is unchanged,
// and record/replay equality remains exact: this is hash composition, not route drift.
// Re-baselined 2026-08-30 for v107's Spark shared-dummy tag plus level/slope
// folds. Both historical probes and all three RNG streams remain byte-identical,
// isolating the shift to current-schema composition.
// Re-baselined 2026-08-30 for v110's unconditional ordered BasePlan authority.
// The dedicated pre-v110 probe reproduces the prior baseline exactly; the RNG
// tuple and tick-for-tick replay remain exact, proving composition-only drift.
// Re-baselined 2026-09-01 for v114's unconditional raw 256-slot crate authority.
// The dedicated pre-v114 probe reproduces the prior current baseline exactly;
// the RNG tuple and tick-for-tick replay remain exact, proving composition-only drift.
// Re-baselined 2026-09-01 for v115's retained wall-neighbor count authority mode and
// shared-dummy overlay identity/state folds. The dedicated pre-v115 probe reproduces the
// prior current baseline exactly; this fixture builds a legacy `None`-count grid, so only
// current-schema composition moved.
// Merge 2026-09-02: main's veterancy re-baseline and its queue-store re-baseline
// both moved these constants, so the three historical probes below are main's
// composed measurement, unchanged by this branch.
// Re-baselined 2026-09-05 for GSI-09.03 harvest bite size (behavior-bearing,
// see `FINAL_STREAM_STATES`); the historical probes move with the final hash.
const GLOBAL_HARNESS_PRE_BASE_PLAN_V110_HASH: u64 = 0x7BCD_CAB2_3774_3468;
const GLOBAL_HARNESS_PRE_CRATE_AUTHORITY_V114_HASH: u64 = 0x0A47_288E_DF5A_1E28;
const GLOBAL_HARNESS_PRE_WALL_RUNTIME_V115_HASH: u64 = 0x2CAA_F65E_2F68_01A4;
// Re-baselined 2026-09-02 for v117's disguise-detect folds (FogState's
// `CellClass+0xAC[house]` counter plane and the cached `DetectDisguiseRange=`
// deposit radius). The dedicated pre-v117 probe reproduces main's committed
// current baseline exactly; this fixture stamps no disguise circle, so only
// current-schema composition moved. The three RNG stream pins are unchanged.
const GLOBAL_HARNESS_PRE_DISGUISE_DETECT_V117_HASH: u64 = 0x77F8_D1C5_1106_FE73;
// Baselined 2026-09-06 for the v135 credit-income folds (GSI-09.01): the
// `BuildingClass+0x6D0` ProduceCash timer and the `TechnoClass+0x1CC/+0x1D0`
// drain link on every entity. The dedicated pre-v135 probe reproduces the
// prior committed current baseline exactly; this fixture has no derrick, no
// DrainWeapon and no capture, so only current-schema composition moved (every
// object folds its dead constructor timer and two `None`s). RNG stream pins
// unchanged.
const GLOBAL_HARNESS_PRE_CREDIT_INCOME_V135_HASH: u64 = 0x752E_A4CB_F018_3992;
// Re-baselined 2026-09-05 for GSI-09.03 harvest bite size (one density level
// per Harvest_Ore_Tick gate, 0x0073D450); see `FINAL_STREAM_STATES`.
// Re-baselined 2026-09-06 for the v135 credit-income folds (GSI-09.01),
// composition-only: `GLOBAL_HARNESS_PRE_CREDIT_INCOME_V135_HASH` holds the
// prior value and every historical probe and stream pin is unchanged.
const GLOBAL_HARNESS_PRE_INFANTRY_TERMINAL_V136_HASH: u64 = 0xD279_8C6F_6721_BAD2;
// v136 adds the Infantry terminal-policy fold. The pre-v136 assertion below
// reproduces the ramp branch's current baseline exactly; older probes and RNG pins
// are unchanged. This is Rust hash-composition provenance, not native parity.
// 2026-09-08 integration of main 33f36d79 with ramp branch 5f41f2ea:
// the pre-v136 probe and all older probes pass, isolating this new current
// value to main's retained InfantryTerminal fold; replay and RNG pins hold.
// v142 retains selected shroud knowledge, per-viewer source/gap admissions,
// Map240 and Foot sight clocks. The pre-v142 probe remains71EC0BD6ED9D45CC;
// every older probe, replay tick and all three RNG streams pass unchanged.
// Receipt: .local/shroud-current-sight-broader-v1-3.log. This is Rust hash
// composition provenance, not a native full-simulation comparison.
// 2026-09-13 ordinary TrackProcess host: reviewed current-cursor payment,
// residual and synchronous arrival timing; historical projections also include
// migrated Foot owners. See docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md
// for baseline/candidate observations and native scope. These are Rust pins.
const GLOBAL_HARNESS_FINAL_HASH_PRE_CELL_MEMBERSHIP_V159: u64 = 0x0CA9_0B9A_0F92_172D;
// Schema159 adds actual ordered Cell membership and exact Sight0 metadata.
// The immediately preceding composition is asserted below against the old
// current pin; all historical, replay/path and RNG tripwires remain intact.
// This is a Rust hash-composition ratchet, not a new native golden.
const GLOBAL_HARNESS_FINAL_HASH_PRE_FOOT_PATH_RUNTIME_V160: u64 = 0xADC6_8CA2_217A_6DDA;
// Schema160 moves the two Foot path timers, blocked latch and dword retry count
// into NavigationState::path_runtime and hashes that owner instead of the former
// positional MovementTarget fields. The immediately preceding composition is
// asserted below against the old current pin; every older probe, replay/path
// and RNG tripwire remains intact. Rust hash-composition ratchet, not a native golden.
// Re-pinned 2026-09-15 for the live bridge repair integration: raw infantry
// occupation owners are the mark-time House index (InfantryClass::
// MarkCellOccupancy 0x005217C0 -> Infantry virtual +0x38 -> House+0x30), no
// longer the Rust entity id, which re-encodes the raw occupation fold under
// every schema and moves every projection at once. Composition-only proof:
// the owner-excluded probe asserted below equals main 595e3a88's value for
// this fixture (receipt .local/harness-repin-20260915/owner-probe.txt).
const GLOBAL_HARNESS_FINAL_HASH: u64 = 0xA26C_6393_4D59_667C;
const GLOBAL_RAW_OWNER_EXCLUDED_V161_HASH: u64 = 0xEFB3_B35E_D792_E94B;
const GLOBAL_PRE_SUSTAINED_SIGHT_V142_HASH: u64 = 0x4E6E_0CFE_23A8_03A7;
}

// Schema171: fresh-turn admission/residual clearing and retained-owner hashes.
// See TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md, PR415 causal attribution.
const GLOBAL_HARNESS_FINAL_HASH_PRE_RETIRED_TIBERIUM_STATE_V174: u64 = 3996170898638906741;
// Schema174 removes folds instead of adding them: OreGrowthState's node-era
// scanner cursor, candidate lists and sample counters, and ProductionState's
// fallback ore overlay id. The pre-174 projection folds the values those fields
// held IN THIS FIXTURE (zero, empty, None): its node-era scan never advanced,
// and it never calls the spawner seeding, the one path that set the fallback
// id. It is not a general reconstruction; a scenario finalized by the map
// loader held Some(first TIB* id). The projection must still equal the previous
// current pin, asserted below. Rust hash-composition ratchet, not a native golden.
const GLOBAL_HARNESS_FINAL_HASH_PRE_CRATE_SPEED_V181: u64 = 10323268363979260645;
// v181 adds the default Foot+580 factor to every entity's hash. The pre-181
// assertion below retains the previous entire fixture state/RNG ratchet.
const GLOBAL_HARNESS_FINAL_HASH_PRE_DISPLAY_LAYERS_V182: u64 = 0x64D8_FAA2_4448_3B4B;
// Snapshot182 adds ordered display vectors. The pre-182 projection below
// must reproduce the previous whole-fixture hash, including all RNG/state.
// Schema186 removes the always-None release-tail byte from each entity. This
// fixture contains no aircraft; the pre186 projection below retains the old
// full hash, and every position/replay/RNG assertion remains unchanged.
// Schema187: object burst index replaces the obsolete target remaining count.
// The pre187 assertion below reproduces the preceding full fixture hash.
// Schema189 folds retained Techno+3D4; Before(189) below reproduces v188.
// v190 adds saved Foot neighbor history. Before(190) reproduces the full v189
// fixture; its legacy grid has no retained plane. Route/RNG pins are unchanged.
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
const GLOBAL_HARNESS_FINAL_HASH: u64 = 0x514D_7BA5_3EFC_0D88;
const GLOBAL_HARNESS_FINAL_HASH_PRE_NATIVE_ORE_FIELD_V207: u64 = 0x2360_9B20_F501_8DC1;
const GLOBAL_HARNESS_FINAL_HASH_PRE_RETIRED_DOCK_PHASE_V206: u64 = 0x78B4_DE6C_76A7_3020;
const GLOBAL_HARNESS_FINAL_HASH_PRE_REARM_TIMER_V202: u64 = 0x126B_C4A8_9BAA_FF2F;
const GLOBAL_HARNESS_FINAL_HASH_PRE_AIRCRAFT_RELEASE_V186: u64 = 12653442097862425252;

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
    for (rx, ry) in [(12, 13), (13, 13), (12, 14), (13, 14)] {
        overlay_grid.place_overlay(rx, ry, tib01, 11);
    }
    overlay_grid.take_dirty_cells();
    sim.overlay_grid = Some(overlay_grid);
    // The playfield map load installs: the harvester's ore scan
    // (`FootClass::Is_Cell_Harvestable @ 0x004DCE80`) admits only cells in it.
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -64,
        off_100: -1,
        off_104: 128,
        off_108: 65,
    });
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
                group_id: None,
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
                group_id: None,
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
                group_id: None,
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
    // system — it acquires an ore target via the SearchOre path. (Physical
    // movement to ore and the full dock handshake need movement world-setup
    // beyond this generic harness; the dedicated miner-dock suite owns that
    // coverage. This guards that miner-component creation + the acquisition
    // path stay wired and contribute to the hash.)
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
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(207)),
        GLOBAL_HARNESS_FINAL_HASH_PRE_NATIVE_ORE_FIELD_V207,
        "the pre-207 ore-field composition moved"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(206)),
        GLOBAL_HARNESS_FINAL_HASH_PRE_RETIRED_DOCK_PHASE_V206,
        "v206 only removes the retired miner dock folds"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(202)),
        GLOBAL_HARNESS_FINAL_HASH_PRE_REARM_TIMER_V202,
        "v202 only folds the object's rearm timer in place of the target's counters"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(190)),
        0xFD01_E512_23DC_F620,
        "v190 changes only the Foot neighbor-history hash composition in this fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(189)),
        0xEDBC_716C_0C88_E276,
        "v189 adds only the retained Techno+3D4 hash fold"
    );
    let before_burst_hash = rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(187));
    assert_eq!(
        before_burst_hash, 0x2691_9D35_924C_2278,
        "schema187 only replaces zero remaining-shot fields with the retained index in this fixture"
    );
    let before_release_hash =
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(186));
    // Schema 172 removed the refinery dock registry's hash folds. They were
    // unframed, so an empty registry contributed no bytes: the pin below holds
    // across that removal exactly when the refinery (id 2) holds no contact at
    // the pinned tick.
    assert!(
        rep.substrate
            .entities
            .get(2)
            .expect("harness refinery")
            .radio_contacts
            .is_empty(),
        "a held refinery contact at the pinned tick would have moved the pin"
    );
    // Bounded full08 causal probe: no AnimRuntime pause fold or legacy gap
    // sample occurs in this fixture. Omit ONLY the newly inserted replay-array
    // fold and Building operational/stuff markers; keep current state intact.
    assert_eq!(
        rep.substrate.anims.len(),
        0,
        "power hash probe excludes Anim layouts"
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
            .all(|e| e.building_storage == Default::default()
                && e.aircraft_ammo.is_none()
                && e.aircraft_mission.is_none()),
        "power composition probe has no storage or aircraft state"
    );
    assert!(
        rep.production.airfield_docks.is_empty(),
        "power composition probe has no dock reservation registry"
    );
    let before_power_hash =
        rep.state_hash_with_schema(super::hash_schema::HashSchema::BeforeBuildingPowerIntegration);
    println!(
        "[global power composition] current={final_hash:016X} before_power={before_power_hash:016X}"
    );
    // 2026-09-23: moved by the Drive/Ship order and first-Process behavior
    // change (see GLOBAL_HARNESS_FINAL_HASH), not by a hash owner.
    // 2026-09-25: moved by the ore-field chain's two causes (same place).
    assert_eq!(
        before_power_hash, 10916856755737007740,
        "full08 projection moved: investigate behavior or another hash owner; do not rebaseline"
    );

    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(174)),
        GLOBAL_HARNESS_FINAL_HASH_PRE_RETIRED_TIBERIUM_STATE_V174,
        "the pre-174 composition must reproduce the previous current pin"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(181)),
        GLOBAL_HARNESS_FINAL_HASH_PRE_CRATE_SPEED_V181,
        "excluding only Foot+580 must preserve the pre-181 fixture"
    );
    assert_eq!(
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(182)),
        GLOBAL_HARNESS_FINAL_HASH_PRE_DISPLAY_LAYERS_V182,
        "excluding only display vectors must preserve the pre-182 fixture"
    );
    println!(
        "[schema168 global] pre168={:016X}",
        rep.state_hash_with_schema(super::hash_schema::HashSchema::Before(168))
    );
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
    // detached curve. The archived receipt above cannot be regenerated from
    // an active retained class. Current full-hash, actual replay, terminal
    // coverage, miner engagement and unchanged absolute RNG pins remain gates.
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
    // keeps the constructor's Retaliate=yes). With the map's playfield
    // installed (2026-09-25; see GLOBAL_HARNESS_FINAL_HASH) the duel ends
    // with both tanks at 12 HP at tick 599. The same-call track continuation
    // this block used to follow is covered by track_path_continuation_tests
    // on production rows.
    assert_eq!(
        rep.substrate
            .entities
            .get(4)
            .map(|tank| tank.health.current),
        Some(12),
        "the retasked tank survives its duel at 12 HP"
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
        before_release_hash, GLOBAL_HARNESS_FINAL_HASH_PRE_AIRCRAFT_RELEASE_V186,
        "restoring only the absent release-tail fold must reproduce the prior fixture"
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
                group_id: None,
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
                group_id: None,
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
/// `GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH`), this fixture returns to exactly its
/// previous committed value `0x0FC6_3769_AADD_1F8A`. So its shift is 100%
/// behaviour and 0% schema — the mirror image of the global harness above.
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
                        group_id: None,
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
            u64::from(entity.facing),
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
                u64::from(
                    entity
                        .body_facing
                        .as_ref()
                        .unwrap()
                        .current(sim.session.binary_frame - 1)
                ),
                call["sampled_after"].as_u64().unwrap(),
                "full16-bit native sample, before the next binary frame"
            );
            assert_eq!(
                entity.body_facing.as_ref().unwrap().timer_start_frame(),
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
