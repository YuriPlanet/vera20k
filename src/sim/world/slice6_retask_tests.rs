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
    }
}

// Immutable historical receipt from5777c115. The final active track had a
// detached geometry/cursor payload that cannot be recovered from the retained
// class. Old comments and pins below refer to that historical execution only;
// current tests must not fabricate it or relax the pre167 projection guard.
#[rustfmt::skip]
mod schema166_receipt {
/// The pre-slice baseline. Captured from `dev` BEFORE the Slice-6 edits (run the
/// gate once, read the failure's `left:` value, paste it here). Slice 6 is
/// behavior-preserving, so this constant MUST NOT change for a Slice-6 *behavior*
/// reason. It DOES shift when a later slice adds a new field to the state hash:
/// the scripted scenario has no bunkers, so the value moved only because the
/// tank-bunker lifecycle state (`bunker_link`, `bunker_runtime`) now joins the
/// hash for every entity at its default — a hash-composition change, not a
/// behavior drift. Re-baselined for Slice 7b, then Slice 8 (MissionCom folded
/// into state_hash — every entity now contributes its default mission bytes;
/// composition change, not a behavior drift). Re-baselined for S3 idle→Guard:
/// idle machine-less Units hash mission Guard(5) instead of the legacy None
/// placeholder (hashed-representation fidelity fix; the retask behavior under
/// test is unchanged). Re-baselined for SC-2 (session identity — seed, map
/// name, theater, bounds, MP start table — folded into the hash; composition
/// change, not a behavior drift). Re-measured at the S3 × SC-2 merge (both
/// deltas combined; value from the merged tree's green run). Re-baselined for S4b
/// (the hashed `damage_particle_live_until` `+0x308`-equivalent field — every
/// entity now folds an extra 0; composition change, NOT a behavior drift, proven
/// by the baseline holding unchanged with the fold line disabled). Re-baselined
/// for startup authority: fresh MapGen now uses the verified native Seed(0)
/// logical state instead of the former synthetic all-zero object. `state_hash`
/// already folds MapGen and this fixture does not consume it, so this is the
/// expected corrected initial-state delta, not Slice-6 retask drift. Two no-edit
/// focused probes reproduced this exact value.
/// Re-baselined for lockstep hash completeness: body-facing presence, damage-fire
/// state/animation IDs, locomotor hover/altitude state, and the empty `AnimStore`
/// marker now join the hash. A current-tree legacy-schema probe reproduced the
/// prior `10575654478637980762` value exactly, proving this shift is composition
/// only rather than retask behavior drift.
/// Re-baselined for snapshot/hash schema v28: independent Object lifecycle axes,
/// deterministic lifecycle bookkeeping, and the ordered pending-delete queue
/// now join the lockstep hash. The current-tree legacy-schema probe below still
/// reproduces the prior value exactly; this is a Rust regression ratchet, not
/// gamemd parity evidence.
// Re-baselined for the Phase-0 native-tail and persistence contracts.
// EventClass now dispatches after the object/global walk, so each scripted
// retask first affects object AI on the following frame, as Main_Tick does.
// The common hash also drops diagnostic `total_sim_ms`, hashes only the
// retail-persisted Scenario RNG, and includes the newly persisted deterministic
// fields. This is an intentional behavior-bearing retail correction, so both
// provenance probes move with the current hash.
// Re-baselined 2026-08-02, same provenance as the global harness constants:
// the mover is 190490ba "match retail cell occupation lifecycle", found by
// bisecting dev..HEAD against this very probe (it is this test's FIRST assert,
// and this fixture has no ore, no resource_nodes, no overlay grid and no
// RNG-consuming path, which is what makes it a clean isolator).
//
// The composition-only hypothesis was tested and REFUTED: with the merge-base
// (6f78bac7) world_hash.rs swapped in and all branch behaviour kept, this probe
// read 0xFEEA0679D9429547 — neither the old baseline nor the branch value. So
// hashed state content changed, not just which fields are folded.
// MERGE 2026-08-03: both branches re-baselined these independently (dev:
// passive acquire + spawner; foundations: Move cadence + hashed runtime
// state). Neither side's values describe the merged tree; re-derived below
// from the merged tree's own output in the same merge commit.
// Native Move mission cadence now advances MissionCom and Scenario RNG.
// Re-baselined after hashing the newly persisted YR runtime-contract state.
/// Re-baselined 2026-08-04 for the GSI-07.02 constructed-`Rate` default
/// (0 -> 14 frames when a mission section or its `Rate=` key is absent).
/// `slice6_rules()` declares no mission sections, so every mission in this
/// fixture left the zero sentinel. Provenance note in
/// global_parity_harness_tests.rs at FINAL_STREAM_STATES.
// Re-baselined 2026-08-05 for the Drive cell-admission slice, with the schema
// and behaviour halves SEPARATED BY MEASUREMENT rather than argued.
// `DriveLocomotionRuntime` gained `occupation_handoff`, and the whole struct is
// hashed by its derived `Hash`, so an `Option` discriminant enters the fold for
// every vehicle even while the field is `None`.
//
// Experiment, no `world_hash.rs` change: keep the field, neutralise every
// behaviour writer that landed with it (the fresh-selection admission gate, the
// chained-curve refusal on the two temporary codes, the handoff mark at both
// install sites, the forced-track pre-clear), re-run. Three points, all
// observed in this tree:
//
//   committed (pre-change) : pre-v28 661EF70FF1847F63  pre-v29 599A096466E0970A  current 5BC6E9E7EEA3E80D
//   schema only            : pre-v28 2A9B661C382F5DC9  pre-v29 AD6A5FDD5EA76570  current EC8407239DC80358
//   schema + behaviour     : pre-v28 728141E6E7EE9CBA  pre-v29 5EA7E13419353AB0  current 22D855782E72D55E
//
// So unlike the global harness — whose entire shift is schema — this 16-tick
// scripted fixture carries BOTH: the field's discriminant moves it once, and
// the admission gate moves it again because its Move/AttackMove script does
// reach the selection lane. Rust regression ratchet, not gamemd evidence.
// Re-measured 2026-08-11 after the integrated GSI-04 authority changes. All
// three probes moved together, so this is state-content/behavior drift rather
// than a live-schema-only fold; values are from the same current-tree run.
// Re-baselined 2026-08-11 after the v69 merge folded the serialized projectile
// collision, Wave, Cell infantry-owner, fog/sensor, and cloak-owner substrates
// into every hash schema. This fixture does not exercise those producers; the
// shift is the intentional explicit-zero/empty composition change.
// Re-baselined 2026-08-13 after GSI-05.01 replaced the ProjectileStore,
// WaveStore, and ProductionState local next-ID sources with the shared global
// `ObjectSubstrate::next_stable_object_id`. The shared source remains hashed;
// removing the three obsolete local counter folds shifts all compositions.
// Re-baselined 2026-08-14 for the v77 GSI-13.06 SHP body-cadence state.
// `body_frame_counter` now contributes one persisted dword per entity, while
// the derived Drive/Ship runtime hashes include the persisted signed owner-
// speed carrier used by the native moving predicate. These folds sit outside
// the v28/v29 gates, so both named legacy probes intentionally move with the
// live composition; filtering them out would create an undocumented hybrid
// schema rather than reproduce either probe's stated contract.
// Re-baselined 2026-08-18: a mid-curve re-order now keeps the in-flight Drive
// curve and anchors the new path at its committed head cell (`TechnoClass::
// Set_Destination` @ 0x00741970 never touches the track cursor; see
// movement_commands.rs). This fixture retasks a moving MTNK at ticks 1/3/5/7,
// so its trajectory legitimately changes — previously each retask restarted
// the curve at the cell lead-in, teleporting the body backward. Behavior-
// bearing, NOT composition-only: position/movement state is hashed in every
// schema, so all three probes move together. Rust regression ratchet; the
// kept-curve contract is exercised by
// `movement_tests::test_reissue_mid_curve_keeps_track_and_anchors_path_at_head`.
// Re-baselined 2026-08-19 with the GSI-08.12 veterancy accumulator: the raw
// float bits and the rank cache are now hashed per object, a composition-only
// shift. No RNG draw is added — `VeterancyClass::Add @ 0x0074FF50` consumes
// none — and record/replay stayed equal at every tick.
// Re-baselined for TechnoClass::TechnoClass @ 0x006F2B90: all three authored
// Technos now consume the raw Scenario word stored at 0x006F3254. That
// behavior-bearing Scenario shift reaches every schema probe, and record/replay
// equality remains exact.
// Re-baselined 2026-08-30 for GSI-04.03 Drive slope payload ownership. The
// common locomotor fold reaches both historical probes; the scripted retasks
// and record/replay equality remain exact, so this is composition-only.
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
// 2026-09-08 ramp-height writer: final entity 3 gains exact Z Some(0).
// Clearing ONLY that value recovers every parent (588f4079) probe, with identical
// final entity state otherwise and equal full RNG states. No schema fold changed.
// See RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md, replay provenance.
// 2026-09-12: raw Drive/Ship head delivery intentionally changes these
// current-state projections, which all fold Drive runtime even under older
// hash schemas. Against exact6580e4c8, replacing only the measured mover leaves
// reproduced all ten new hashes; every original baseline pin also passed.
// Slice6 changes only entity1's committed headXY (6272,640 -> 1408,1152)
// and Stop's abandoned direction cursor (0 -> 19). Retasking no longer replaces
// the in-flight head with the latest destination. This is a behavior ratchet,
// not a native whole-movement golden; see the bridge walker report's raw-head section.
const SLICE6_PRE_LIFECYCLE_V28_HASH: u64 = 0x60FA_3998_AF7C_B1D7;
const SLICE6_PRE_MISSION_V29_HASH: u64 = 0x7D16_06A7_1D5B_D210;
// Snapshot/hash schema v29 adds lossless Mission dwords, readiness leaves,
// suspended Target/falling state, and raw locomotor-ready inputs. The two
// schema probes below must prove the shift is composition-only before updating
// this live regression value.
// Re-baselined (all three constants) for the Mission authority flip:
// commands queue through the exact authority and the host promotes; the
// legacy per-tick projection is deleted, so every hashed mission value —
// including the reduced subset the legacy pre-v29 composition folds —
// changes together. Behavior-bearing Rust ratchet, not gamemd evidence.
//
// Re-baselined 2026-07-29 for the locomotion S2 readiness producers, twice:
// first for Drive/Ship/Teleport/Jumpjet, then again when Walk and Hover joined
// them so all six live families now write `mission_ready_state`. The ceremony this
// file requires — the two schema probes proving the shift is composition-only —
// is satisfied: BOTH probes above are unchanged and green, so only the
// current-schema hash moved, i.e. the delta is the hashed field going
// `None → Some` and not a behaviour divergence. Independently confirmed by
// neutralising the behaviour path (making the readiness gate ignore the
// produced state while the producers still ran): the hash was identical to
// this new value, so the deferral change contributes nothing here.
//
// Re-baselined 2026-07-30 when the readiness inputs stopped being stored on the
// locomotor and became derived at the Mission gate instead. gamemd's readiness
// virtual performs a fresh locomotor call at every one of its ~two dozen call
// sites, with no cached per-frame flag anywhere on that path, so a per-tick
// cache answered nearly all of them with stale state; verified from the Infantry
// and Unit readiness overrides and the queue-then-commence caller.
//
// Composition-only here, and this file's ceremony proves it: BOTH schema probes
// above are unchanged and green. The pre-v29 probe still hashes every position,
// facing and movement field, so if the derivation had changed *when* any unit
// commenced, that probe would have moved too. It did not — only the
// current-schema hash did, and its delta is exactly the removed readiness bytes.
// (Behaviour-neutral in THIS fixture is not behaviour-neutral in general: the
// paths the change exists for — dock, unlink, unload and deploy handoffs that
// stop a unit and queue-and-commence in the same tick — are not covered here.)
// Rust regression ratchet, not gamemd evidence.
//
// Re-baselined 2026-07-30 for S3b: the installed LocomotorSlot joins the hash.
// **Composition-only, proved by neutralisation** — the ceremony this file
// normally uses cannot decide it, because the locomotor block is hashed
// unconditionally, so BOTH schema probes move with the live value. Instead the
// new hash line was commented out and the whole suite re-run: all three
// constants returned to their previous committed values exactly, so the
// primary_kind -> slot retype changed no behaviour and no other hashed state,
// and the entire delta is the one new byte. The absolute per-stream RNG pins
// and the dense-scenario position fingerprint were unchanged throughout.
// Re-baselined 2026-07-30 for S5: the locomotor `powered` flag joins the hash.
// Composition-only, proved by neutralisation (the probe ceremony cannot decide
// it — the locomotor block is hashed unconditionally, so both probes move with
// the live value). With the new hash line commented out, all three constants
// returned to their S3b values exactly, which also proves the three power edges
// wired in this slice (deploy-begin off, undeploy-complete on, destination-
// accepted on) changed no other hashed state in these fixtures. The absolute
// per-stream RNG pins held throughout.
// MERGE 2026-08-03: both branches re-baselined these independently (dev:
// passive acquire + spawner; foundations: Move cadence + hashed runtime
// state). Neither side's values describe the merged tree; re-derived below
// from the merged tree's own output in the same merge commit.
//
// Re-baselined 2026-08-02 for passive/opportunity target acquisition.
// **Composition-only, but NOT for the reason this file's usual ceremony would
// suggest.** Both schema probes above run with the v29 block excluded, and every
// field this slice adds or changes — `passive_scan_timer`,
// `last_target_scan_frame`, `passively_acquired_target` — lives inside that
// block. So the probes are structurally incapable of moving for this change and
// prove nothing about it either way. Do not read their staying green as
// evidence.
//
// What actually isolates it is the fixture: it runs 16 ticks, short of the
// object's 45-frame initial scan delay, so no scan fires, no draw is consumed
// and no target is installed. The entire delta is therefore the three v29-block
// fields moving off their old values — `passive_scan_timer` armed at the
// construction frame instead of left unarmed, plus the two new fields folded at
// their defaults. Rust regression ratchet, not gamemd evidence.
/// Re-baselined 2026-08-05 for the Drive cell-admission slice; the measured
/// schema/behaviour split for all three of this fixture's constants is written
/// out at `SLICE6_PRE_LIFECYCLE_V28_HASH`.
// Current-schema value from the same 2026-08-11 measurement above.
// Re-baselined 2026-08-13 for the same global-ID hash-composition change above.
// Re-baselined 2026-08-14 for the same v77 authoritative-state composition
// described at the two legacy probes above.
// Re-baselined 2026-08-14 for v44 entity-animation hash authority. Both legacy
// probes remain exact, isolating this to the current-schema composition change.
// Re-baselined 2026-08-15 for `14e096ff`: authoritative animation timing now
// comes from the RuleSet in headless/replay frames, so this fixture's E1 advances
// the already-hashed v44 entity-animation cursor. Both legacy probes remain
// exact, isolating the change from the retask behavior under test. This is a
// Rust regression ratchet; the native sequence-timing sources are documented
// by that commit, while non-stock READY/GUARD precedence remains UNCHECKED.
// Re-baselined 2026-08-18 for the kept in-flight Drive curve on mid-curve
// re-orders; see the dated comment at the two legacy probes above.
// Re-baselined 2026-08-19 with the GSI-08.12 veterancy accumulator: the raw
// float bits and the rank cache are now hashed per object, a composition-only
// shift. No RNG draw is added — `VeterancyClass::Add @ 0x0074FF50` consumes
// none — and record/replay stayed equal at every tick.
// Re-baselined 2026-08-20 for the v87 TechnoClass+0x3D5 membership byte.
// The pre-v28 and pre-v29 probes above remained byte-identical, isolating this
// to the intentional current-schema hash composition change.
// Re-baselined 2026-08-20 for v88's deposited-sensor presence discriminator.
// None of this fixture's types has SensorsSight; both historical probes stayed
// byte-identical and the same-run hash is stable, proving a composition-only
// current-schema move.
// Re-baselined 2026-08-24 for snapshot/hash schema v90: current hashing now
// folds the exact serialized real `CellClass+0x140 & 0x1180` value authority
// once behind its schema tag. The pre-v28 and pre-v29 probes above remain
// byte-identical, and record/replay remains exact, proving composition-only.
// Re-baselined 2026-08-26 for snapshot/hash schemas v92-v102: Phase 3 adds
// empty/default base-reservation, House strategy/base-defense, TeamScript VM,
// AIMD/TeamType/TaskForce, and typed AITrigger authority to the current hash.
// Both historical probes remain byte-identical; this 16-tick fixture creates
// no Teams or AI registries, so the shift is current-schema composition only.
// Re-baselined 2026-08-26 for snapshot/hash schema v103: the empty
// `active_wave_links` map and empty persisted destroyable-cliff mutation map
// now join the current hash. Both historical probes remain byte-identical;
// this fixture creates no Wave or cliff mutation, so the measured shift is
// current-schema composition only.
// Techno constructor RNG also persists in the current-schema Techno fields;
// the two legacy probes above isolate the shared behavior-bearing portion.
// Re-baselined 2026-08-30 for the same slope payload move; both historical
// probes shift consistently and the replay remains deterministic.
// Re-baselined 2026-08-30 for v107's Spark shared-dummy tag plus level/slope
// folds. Both historical probes remain byte-identical, so only current-schema
// composition moved.
// Re-baselined 2026-08-30 for v110's unconditional ordered BasePlan authority.
// The dedicated pre-v110 probe reproduces the prior baseline exactly, isolating
// the measured shift to current-schema composition.
// Re-baselined 2026-09-01 for v114's unconditional raw 256-slot crate authority.
// The dedicated pre-v114 probe reproduces the prior current baseline exactly;
// this fixture's behavior and record/replay equality remain unchanged.
// Re-baselined 2026-09-01 for v115's retained wall-neighbor count authority mode and
// shared-dummy overlay identity/state folds. The dedicated pre-v115 probe reproduces the
// prior current baseline exactly; this fixture builds a legacy `None`-count grid, so only
// current-schema composition moved.
// Merge 2026-09-02: main's veterancy re-baseline and its queue-store re-baseline
// both moved these constants, so the three historical probes below are main's
// composed measurement, unchanged by this branch.
// Re-baselined 2026-09-02 for v117's disguise-detect folds: FogState's
// `CellClass+0xAC[house]` counter plane and the cached `DetectDisguiseRange=`
// deposit radius. The dedicated pre-v117 probe reproduces main's committed
// current baseline exactly; this fixture stamps no disguise circle, so only
// current-schema composition moved.
const SLICE6_PRE_BASE_PLAN_V110_HASH: u64 = 0x3B3D_D5F6_E92F_C6AD;
const SLICE6_PRE_CRATE_AUTHORITY_V114_HASH: u64 = 0x751E_0B76_E5D3_1037;
const SLICE6_PRE_WALL_RUNTIME_V115_HASH: u64 = 0xC029_4908_06B6_8666;
const SLICE6_PRE_DISGUISE_DETECT_V117_HASH: u64 = 0xE1B8_A1E6_930B_3EDA;
// Re-baselined 2026-09-06 for the v135 credit-income folds (GSI-09.01): the
// `BuildingClass+0x6D0` ProduceCash timer and the `TechnoClass+0x1CC/+0x1D0`
// drain link pair join the per-entity fold. The dedicated pre-v135 probe
// reproduces the prior current baseline exactly and every older probe holds;
// this script has no derrick, DrainWeapon or capture, so only composition
// moved (each object folds its dead constructor timer and two `None`s).
const SLICE6_PRE_CREDIT_INCOME_V135_HASH: u64 = 0x780B_B9D9_432A_D2DD;
const SLICE6_PRE_INFANTRY_TERMINAL_V136_HASH: u64 = 0xA7D2_F760_23D6_E5DE;
// v136 adds the Infantry terminal-policy fold. The pre-v136 assertion below
// reproduces the ramp branch's current baseline exactly; older probes and RNG pins
// are unchanged. This is Rust hash-composition provenance, not native parity.
// 2026-09-08 integration of main 33f36d79 with ramp branch 5f41f2ea:
// the pre-v136 probe and all older probes pass, isolating this new current
// value to main's retained InfantryTerminal fold; existing command checks hold.
// v142 adds retained shroud knowledge/admissions, Map240 and Foot sight clocks.
// The dedicated pre-v142 assertion below retains this fixture's prior current
// pin; older probes and existing replay/stream checks remain unchanged.
// Receipt: .local/shroud-current-sight-full-v1.log (composition-only candidates).
// 2026-09-13 ordinary TrackProcess host: reviewed current-cursor payment,
// residual and synchronous arrival timing; historical projections also include
// migrated Foot owners. See docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md
// for baseline/candidate observations and native scope. These are Rust pins.
const SLICE6_BASELINE_HASH_PRE_CELL_MEMBERSHIP_V159: u64 = 0x4461_4A89_0BC9_D32D;
// Schema159 adds actual ordered Cell membership and exact Sight0 metadata.
// The immediately preceding composition is asserted below against the old
// current pin; all historical, replay/path and RNG tripwires remain intact.
// This is a Rust hash-composition ratchet, not a new native golden.
const SLICE6_BASELINE_HASH_PRE_FOOT_PATH_RUNTIME_V160: u64 = 0x1D31_3ED8_9A2D_044A;
// Schema160 moves the two Foot path timers, blocked latch and dword retry count
// into NavigationState::path_runtime and hashes that owner instead of the former
// positional MovementTarget fields. The immediately preceding composition is
// asserted below against the old current pin; every older probe, replay/path
// and RNG tripwire remains intact. Rust hash-composition ratchet, not a native golden.
// Re-pinned 2026-09-15 for the live bridge repair integration. Two changes move
// every projection of this fixture at once: (1) raw infantry occupation owners
// are the mark-time House index (InfantryClass::MarkCellOccupancy 0x005217C0 ->
// Infantry virtual +0x38 -> House+0x30), no longer the Rust entity id, which
// re-encodes the raw occupation fold under every schema; (2) the E1 Attack
// pursuit at tick 9 now follows Walk natively: FindPath runs in the next
// Process (0x75AFC5) one frame after acceptance, the route goal is the target's
// own Cell, and the head sub-cell selection draws Scenario RNG (0x004ACA10).
// Attribution: per-tick dumps of main 595e3a88 versus this tree diverge only at
// E1 from tick 9 (receipt .local/harness-repin-20260915/slice6-dump.diff);
// the owner-excluded probe below therefore differs from main here. Rust pins.
const SLICE6_BASELINE_HASH: u64 = 0x1B35_F16E_945D_5812;
const SLICE6_RAW_OWNER_EXCLUDED_V161_HASH: u64 = 0x1906_B698_79B5_95DE;
const SLICE6_PRE_SUSTAINED_SIGHT_V142_HASH: u64 = 0x3378_724A_9514_52B4;
}

// Schema171: live type acceleration preserves retasked track progression;
// fresh turning defers admission. Old receipts above remain historical only.
// See docs/research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md, PR415 attribution.
const SLICE6_BASELINE_HASH_PRE_RETIRED_TIBERIUM_STATE_V174: u64 = 0x088D_D2CB_E558_B04C;
// Schema174 removes folds instead of adding them: OreGrowthState's node-era
// scanner cursor, candidate lists and sample counters, and ProductionState's
// fallback ore overlay id. The pre-174 projection folds the values those fields
// held IN THIS FIXTURE (zero, empty, None): its node-era scan never advanced,
// and it never calls the spawner seeding, the one path that set the fallback
// id. It is not a general reconstruction; a scenario finalized by the map
// loader held Some(first TIB* id).
// Paid Walk (2026-09-21) changes both projections through E1 alone: native
// polar coordinates, FacingClass timer refresh and the Foot speed cache. The
// generic direction cache is no longer its movement authority. Replacing only
// E1 with ec27dc26's final state reproduces old current90DA2A8E0C06D5E3 and
// pre174221E77F911A4FB24 exactly; tanks and RNG match on all16 frames. See
// docs/research/COMBAT_WALK_REPLAY_ATTRIBUTION.md. Rust pins, not native goldens.
const SLICE6_BASELINE_HASH_PRE_CRATE_SPEED_V181: u64 = 0x89B4_0F1C_9568_FFA5;
// v181 folds Foot+580, including default1.0. The pre-181 assertion below
// proves this fixture's shift comes only from the added hash field.
const SLICE6_BASELINE_HASH_PRE_DISPLAY_LAYERS_V182: u64 = 0xF2F4_BA44_25AB_B002;
// Snapshot182 adds ordered display vectors. The pre-182 projection below
// must reproduce the previous whole-fixture hash, including all RNG/state.
const SLICE6_BEFORE_INFANTRY_ROT_HASH: u64 = 10367437667977646885;
// Infantry ctor517BBD supplies PrimaryFacing ROT127. The comparison below
// changes only that retained rate back to0 and reproduces every previous pin.
// Schema186 removes each entity's always-None aircraft release-tail fold. The
// pre186 assertion retains this fixture's preceding current hash, independently
// of the existing constructor-rate projection and native paid-Walk witnesses.
const SLICE6_BASELINE_HASH_PRE_AIRCRAFT_RELEASE_V186: u64 = 0xD7C6_B3FA_0CBC_4644;
// Schema187: the pre187 projection below preserves the preceding full pin.
// Schema189 folds retained Techno+3D4; Before(189) below reproduces v188.
// v190 adds saved Foot neighbor history. Before(190) reproduces the full v189
// fixture; it has no retained counter plane. Route/RNG pins are unchanged.
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
const SLICE6_BASELINE_HASH: u64 = 0xBC54_5E9B_173C_AEC2;
const SLICE6_BASELINE_HASH_PRE_AIRCRAFT_CRASH_V208: u64 = 0x2A32_AF12_7767_94D1;
const SLICE6_BASELINE_HASH_PRE_REARM_TIMER_V202: u64 = 0xE873_DCE2_1778_7464;

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
                group_id: None,
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
                u64::from(
                    infantry
                        .body_facing
                        .unwrap()
                        .current(sim.session.binary_frame)
                ),
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
    let hash = sim.state_hash();
    println!(
        "[schema168 slice6] pre168={:016X}",
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(168))
    );
    println!(
        "[slice6] current={hash:016X} streams={:016X},{:016X},{:016X}",
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
    let infantry_facing = sim.substrate.entities.get(3).unwrap().body_facing.unwrap();
    assert_eq!(infantry_facing.rot_per_frame(), 0x7F00);
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .body_facing
        .as_mut()
        .unwrap()
        .set_rot(0);
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(186)),
        SLICE6_BEFORE_INFANTRY_ROT_HASH,
        "only the corrected Infantry constructor rate may differ from the preceding baseline"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(174)),
        SLICE6_BASELINE_HASH_PRE_RETIRED_TIBERIUM_STATE_V174,
        "pre-174 projection drifted from the documented paid Walk behavior"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(181)),
        SLICE6_BASELINE_HASH_PRE_CRATE_SPEED_V181,
        "excluding only Foot+580 must preserve the pre-181 fixture"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(182)),
        SLICE6_BASELINE_HASH_PRE_DISPLAY_LAYERS_V182,
        "excluding only display vectors must preserve the pre-182 fixture"
    );
    sim.substrate.entities.get_mut(3).unwrap().body_facing = Some(infantry_facing);
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(186)),
        SLICE6_BASELINE_HASH_PRE_AIRCRAFT_RELEASE_V186,
        "restoring only the absent release-tail fold must reproduce the prior fixture"
    );
    assert_eq!(
        sim.state_hash(),
        hash,
        "restore the unmodified live facing state"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(187)),
        0xB071_E051_E20C_947B,
        "schema187 only replaces zero remaining-shot fields with the retained index in this fixture"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(189)),
        0x6FDD_EEBD_9E69_B3FB,
        "v189 adds only the retained Techno+3D4 hash fold"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(190)),
        17671192503384378705,
        "v190 changes only the Foot neighbor-history hash composition in this fixture"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(202)),
        SLICE6_BASELINE_HASH_PRE_REARM_TIMER_V202,
        "v202 only folds the object's rearm timer in place of the target's counters"
    );
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(208)),
        SLICE6_BASELINE_HASH_PRE_AIRCRAFT_CRASH_V208,
        "v208 only folds the crash latch, its AI edge and the Fly fall counter"
    );
    assert_eq!(
        hash, SLICE6_BASELINE_HASH,
        "Slice 6 scripted-retask state hash drifted. Treat this as behavior drift \
         unless a documented native behavior change or hash-composition change \
         is causally demonstrated"
    );
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
            group_id: None,
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
