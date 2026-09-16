# Slice S3 — UnitClass post-Foot ordering (Fire→Facing kill-tick coupling + idle→Guard) Design

**Status:** MERGED TO DEV (2026-06-10, merge commit `073c5ac4`; dev lib suite 3865/0 green).
Landed via worktree branch `s3-postfoot-ordering` (now deleted) off dev `7b79a186`, merged
through three concurrent slices (radiation Slice 7, SC-1/SC-2 ScenarioSession) — the
SNAPSHOT_VERSION ladder resolved to 23 and both goldens were re-measured at the combined
merge. A 19-agent adversarial review confirmed all ledger items had homes; it produced the
`facing_apply_point_equivalence_no_kill` pin, the dying-projection window correction, and the
verb-layer idle-sentinel trap note (commit `13d58f24`). Out-of-scope finding handed to the
radiation-slice owner: f64 math + f64-bit hashing in lockstep state (combat/mod.rs Phase 3.5,
world_hash.rs hash_radiation) violates the no-float rule. Commits `4f752ce3..7dbfdd88`
(T1 plumbing → T2 P2-window compute → T3 apply extraction → T4 facing flip → T5 idle→Guard →
T6 SNAPSHOT_VERSION 20→21 → T7 kill-tick round-trip + full suite 3838/0). Golden outcomes:
T4 facing flip left the global harness UNSHIFTED (no Unit kill/retarget divergence ticks in
the committed scenario); T5 idle→Guard re-baselined the global harness + slice6-retask
baselines with documented reasons. Design-review findings were resolved in v2 before
implementation.
**Date:** 2026-06-10
**Rule:** Rust-native structure, gamemd-native semantics. Second hash-affecting object-AI slice.
**Ladder position:** Slice **S3** in [docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md)
§9 (4th of S0–S8). Builds on landed S2 (dispatch-time mission authority, `32f9ef36..7b79a186`)
and the landed L2 ai-shell tasks (per-object Unit fire via `resolve_attacker_fire` in live order;
per-Unit facing via `unit_post::tick_unit_facing`, hash-neutral flip).

## Goal

Make the per-object **Fire→Facing coupling gamemd-faithful at the kill boundary** (a Unit whose
target dies this tick keeps aiming at it this tick; idle-return starts next tick), and make a
machine-less idle **Unit's hashed mission `Guard(5)`** instead of the port-artifact `None` —
the two S3 deltas that are observable/hashed. Establish `unit_post` as the named owner of the
verified post-Foot slot order (Fire → Facing → guard-terrain → HarvestBrain → anim/ammo →
SpawnManager) with explicit, named status for each not-yet-implemented slot. Hash-affecting →
`SNAPSHOT_VERSION` 20→21 + gamemd-cited golden re-baseline.

## What the ladder asked vs. what is already landed

Ladder S3 says: couple Fire→Facing per-object for scoped UnitClass; retire the global attacker
snapshot + turret sweep for in-scope units. **Most of that landed in the L2 ai-shell tasks**
(2026-06-04, before this slice):

- Fire is already per-attacker, in **live LogicVector order**, via `resolve_attacker_fire`
  ([combat/mod.rs:1772](../../src/sim/combat/mod.rs); P2 snapshots sorted by live index
  `:1510-1515`, loop `:1523-1530`).
- Unit facing is already per-Unit (`unit_post::tick_unit_facing`,
  [unit_post.rs:39](../../src/sim/world/unit_post.rs)); the global `tick_turret_rotation` sweep
  skips Units ([turret.rs:149](../../src/sim/movement/turret.rs)). Flip was shadow-proven
  hash-neutral (L2 Task 2: 3615/0 agreement).
- Fire-before-facing per object already holds at phase granularity (combat Phase 5 runs before
  the facing pass), and fire reads the **P1 attacker snapshot's** barrel state — last-tick
  facing — so "no same-tick rotate-and-fire" holds. `[asm 0x007365E1 → 0x007365E8;
  UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md §7, ledger #14]`

**What is NOT yet faithful** (found this session, drives this slice):

1. **Kill-tick facing drift — fires on every kill, player-visible.** The death batch
   (`clear_targets_on_dead_entity`, [combat/mod.rs:1128](../../src/sim/combat/mod.rs)) clears
   every attacker's `attack_target` targeting the dead entity, and `tick_unit_facing` runs
   **after** that batch ([world/mod.rs:2287](../../src/sim/world/mod.rs)). So on the tick a
   target dies, the killer's (and every co-attacker's) barrel destination snaps back to body
   facing **that same tick**. In gamemd the facing step runs inside the unit's own AI pass:
   `Fire_At_Target` launches a munition (no same-tick HP), `Facing_Update` then reads the
   still-live TarCom, and the bullet's AI runs **after** the firing unit's pass (LogicClass
   re-reads the vector count per call). TarCom is cleared only at actual death via the
   PointerExpired sweep — first visible to the attacker's facing **next** pass. So the
   gamemd-faithful kill-tick barrel destination is **the target, not body facing**.
   `[L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md: Fire_At @0x006fdd50 launches munition,
   HP at BulletDetonation @0x00468d80; UNITCLASS_GHIDRA_REPORT.md §10: LogicClass loop
   0x0055AFB0 re-reads count after each vtable+0x5C call; DETACH_FROM_ALL_LISTS_*: PointerExpired
   clears TarCom at death]` — `barrel_facing` is hashed
   ([world_hash.rs:681-684](../../src/sim/world/world_hash.rs)) → hash-affecting.
2. **Idle mission value is a port artifact.** A machine-less Unit derives `(None, 0)`
   ([game_entity.rs:558](../../src/sim/game_entity.rs)); gamemd has no "None" mission — an idle
   ground vehicle sits in **Guard(5)**, dispatched at `[Guard] Rate=.030` (~27 frames). S2
   explicitly deferred this mapping to S3 (S2 design "Out-of-scope idle→Guard … are S3"; the
   committed S2 test `arrival_tick_mission_is_move_not_sleep` asserts the post-arrival value is
   `None` with the comment "S3 owns idle→Guard"). `mission.current` is hashed (Slice 8) →
   hash-affecting. Without it, S4's passive-acquire gate (missions {2,10,5} only, verified
   `decompile 0x006F9E50`) can never fire for idle units — S3 unblocks S4.
   `[S2_MISSION_DISPATCH_VS_PASSIVE_ACQUIRE_ORDERING.md; FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md
   §6 ([Guard] Rate=.030, dispatch case 2 = Move ⇒ Move=2, Guard=5 in the shifted enum);
   S2 design §Impact ("decompile 0x004D4200 this session": arrival tick stays Move, →Guard
   only after the post-arrival dispatch)]`

## Architecture Context

- Phase 5 today ([world/mod.rs:2232-2292](../../src/sim/world/mod.rs)): pursuit → C4/capture →
  `tick_combat_with_fog` (P1 snapshot build → P2 per-attacker fire in live order → P3-6 batched
  damage/death apply, including `clear_targets_on_dead_entity`) → `tick_turret_rotation`
  (non-Units) → `unit_post::tick_unit_facing` (Units, `keys_sorted` order).
- `desired_turret_facing` ([turret.rs:80](../../src/sim/movement/turret.rs)) is the shared pure
  read both sweeps use; its doc already anticipates "the per-object Fire→Facing host".
- `resolve_attacker_fire` can auto-retarget mid-resolution (acquire calls at
  [combat/mod.rs:1870/1950/1967](../../src/sim/combat/mod.rs)); retargets are emitted and
  applied in the batch, not inline.
- `derived_mission` ([game_entity.rs:531](../../src/sim/game_entity.rs)) priority: miner →
  aircraft → dock → attack → move → **None fall-through**. Aircraft already map idle →
  `Guard`; the fall-through is what Units hit when idle.
- S2's in-loop dispatch commits `mission.current` at dispatch time for scoped movers; the tail
  `refresh_mission_shadow_except` projects everyone else
  ([world/mod.rs:919-927/2660](../../src/sim/world/mod.rs)). Load trusts serialized `MissionCom`
  (S2 T4). `SNAPSHOT_VERSION` is 20 (S2 T5).

## Impact Analysis — the two hash deltas

1. **Kill-tick facing (D1).** Moving each Unit's facing *read point* to its per-object position
   (pre-death-batch, post-own-fire) changes the barrel **destination** only on ticks where the
   unit's target died (or was retargeted) that tick. `FacingClass` state is hashed → golden
   moves on combat scenarios with kills. Justified by gamemd evidence (above), not tolerated
   drift. All other ticks: bit-identical by a read-set argument — between the P2 fire loop and
   today's post-batch facing pass, nothing writes any Unit's `barrel_facing`, position, or
   (except the death batch) `attack_target`; fire emissions don't mutate entities mid-loop
   (verified L2 Task 1: zero `get_mut` in the P2 body).
2. **Idle→Guard (D2).** Machine-less, non-passenger, live Units derive `(Guard, 0)` instead of
   `(None, 0)`. Hash moves on **every tick with any idle Unit** — broad golden re-baseline, not
   churn-window-only like S2. No behavior system reads `mission.current` today (S2 P2 finding:
   only `world_hash` + debug shadows) → no movement/combat output change; this is hashed
   representation brought to the gamemd value, and the S4 enabler.
3. **No phase moves.** Phase 5's internal order (combat → sweeps) and `advance_tick` phase order
   are untouched (invariant #2). The facing *write* point stays where it is — only the *read*
   (destination computation) moves into the per-object window. `FacingClass::set` is anchored to
   `binary_frame`, and no writer touches `barrel_facing` between the new read point and the old
   write point (gate G4 re-verifies at plan time), so for non-kill ticks the result is
   bit-identical by construction.
4. **Blast radius.** `combat/mod.rs` (emit facing destinations from the per-attacker window),
   `unit_post.rs` (split compute/apply), `world/mod.rs` (apply call site), `game_entity.rs`
   (derive fall-through), S2 test updates (`arrival_tick_mission_is_move_not_sleep` post-arrival
   expectation None→Guard), `SNAPSHOT_VERSION`, golden. Non-Unit categories: untouched
   (`tick_turret_rotation` keeps its post-batch read for Aircraft/Buildings until S7/S8 — their
   kill-tick drift **remains and is named**, deferred to their slices).

## Chosen Approach

**A "compute-per-object, apply-at-site" facing host + a category-gated Guard fall-through.**

For **D1**: inside `tick_combat_with_fog`, immediately after each Unit attacker's
`resolve_attacker_fire` returns, compute that unit's desired barrel destination
(`desired_turret_facing`, adjusted for an own-retarget emitted by that very resolution — the
gamemd analog is upstream same-pass acquisition aiming the same tick) and push `(id, desired)`
into the combat emit. After the P2 loop **and strictly BEFORE Phase 3** (the placement is
semantic — Phases 3–6 clear `attack_target` on finished attackers `:1654-1661` and on dead
targets `:985`), a residual pass computes destinations for every other live Unit not in the
snapshot set: target-less Units (idle return — inputs untouched by P2; P1's only mutation is
cooldown advance, which facing never reads) and **in-transport Units holding a target**
(excluded from snapshots at `:1424`). The destinations are **applied** (barrel `set_rot` +
`set`) at the existing `unit_post` call site after the batch — `unit_post::apply_unit_facing`.
Result: destination inputs are per-object pre-death (gamemd-faithful), write point unchanged,
phase order unchanged.

For **D2**: `derived_mission`'s fall-through becomes `(Guard, 0)` for
`category == Unit && !passenger_role.is_inside_transport()`; passengers and all other
categories keep today's mapping (named placeholders for their slices). Arrival timing is
preserved by construction: the arrival tick still hashes dispatch-time `Move` (S2); the *next*
tick's projection now yields `Guard` instead of `None` — exactly the verified gamemd sequence.

For the **slot scaffold**: `unit_post.rs` gains the explicit ordered slot list as code
structure + doc (fire → facing → guard_terrain → harvest_brain → update_animation →
spawn_manager) where the last four are documented markers with named status (below), so later
slices fill slots instead of re-deriving the order.

## Tiny-Detail Ledger

- **Post-Foot order:** Fire(§3m) → Facing(§3n) → guard-terrain(§3o) → HarvestBrain(§3p) →
  UpdateAnimation `vtable+0x424`(§3q) → SpawnManager(§3r) → [AI auto-hunt(§3s), stuck
  rescue(§3t) — AI-house]. `[UNITCLASS_GHIDRA_REPORT.md §3, §10 ordered list; asm 0x007365E1 →
  0x007365E8 (GRIZZLY_TURRET_ROT_BODY_FIRE_SPLIT §4; re-confirmed live in the L2-T3 session
  decompile 0x00736df0)]`
- **Fire uses last-tick facing** — fire gate reads the P1 snapshot barrel state; rotation
  started this tick affects next tick's gate. Preserved (snapshot mechanism untouched).
  `[UNITCLASS_TURRET_TRACKING §7 crit-1, §9 #14]`
- **Kill-tick barrel destination = the dying target,** idle-return starts next tick. Composition
  of three verified facts: deferred munition damage `[0x006fdd50 → 0x00468d80, L2 verdict doc]`;
  bullet AI runs after the firing unit's pass `[LogicClass 0x0055AFB0 re-read contract]`;
  TarCom cleared at death via PointerExpired `[DETACH_FROM_ALL_LISTS_*]`. Composition itself =
  G1 (spot-check when Ghidra is up).
- **Idle return destination** for a target-less turreted Unit = `body_facing << 8`. Unchanged.
  `[UNITCLASS_TURRET_TRACKING §5.1; turret.rs:110-115]`
- **FacingClass math untouched:** ROT byte `<<8`, clamp >0x7E→0x7F, snap when `abs(diff)/ROT<1`,
  ftol truncation. `[UNITCLASS_TURRET_TRACKING §9 #1-#6; facing_class.rs]`
- **Apply-point equivalence:** `FacingClass::set(dest, binary_frame)` outcome is independent of
  where within the tick it is called, given no intervening barrel writes and the same
  `binary_frame` — G4 audits "no intervening writes"; a test pins it.
- **Idle Unit mission = Guard(5), substate 0** (shifted enum: Sleep=0, Attack=1, Move=2,
  QMove=3, Retreat=4, **Guard=5**; values < Harvest(10) unaffected by the Eaten shift).
  `[FOOTCLASS_MISSION_MOVE §3.3 (case 2 = Move); D2/D7 mission table; passive-acquire gate
  {2,10,5} decompile 0x006F9E50; [Guard] Rate=.030 rulesmd.ini]` Exact ground-unit idle
  assigner not yet traced = G2 (YELLOW); the *value* is multiply corroborated.
- **Arrival sequence:** arrival tick hashes `Move` (S2, unchanged); next tick `Guard` (was
  `None`). `[S2 design: decompile 0x004D4200 2026-06-10 — Move on arrival, →Guard after the
  post-arrival dispatch; NAVCOM_LIFECYCLE §5.2]`
- **Passengers in transports:** gamemd mission value UNCHECKED (candidates Sleep(0)/Guard(5) —
  do NOT guess); S3 keeps their `None` derive as a **named unfaithful placeholder**.
  `[UNKNOWN — needs RE: trace the enter-transport mission commit]`
- **Guard-terrain check (§3o):** Guard(5) + invalid terrain + has-sight → kills passengers,
  destroys self. Slot marker only; predicate/exactness UNCHECKED — behavior lands only after a
  live decompile of the branch in `0x007360C0`. `[UNITCLASS §3o; UNKNOWN — needs RE]`
- **HarvestBrain (§3p):** gate `Harvester=`/`Weeder=` (`type+0xE18/+0xE19`); decides idle→
  Harvest; contains RNG seeds (`Random(0,2)*30` bale jitter `[UNIT_0x3E §3.3]`). Owned by the
  miner substrate (S2 scope excluded miners; L5 seam routes Harvest). S3 places the slot marker;
  relocating miner logic is explicitly NOT this slice.
- **UpdateAnimation (`vtable+0x424`, §3q):** UnitClass slot target unresolved (address unknown —
  do not invent); body-anim frame updates; sim-relevant state unknown. Slot marker, UNCHECKED.
  `[UNITCLASS §3q; UNKNOWN — needs RE]`
- **SpawnManager (§3r):** gate `SpawnDelay > 0` (`type+0x5E0`), skipped when destroyed or
  mission==Sleep; Carrier/Dreadnought/Osprey spawn system **absent in Rust** — inert slot,
  named feature gap (blocked on the missing SpawnManager system, not silently dropped).
  `[UNITCLASS §3r, §11]`
- **TurretAI idle scan (§3j):** runs BEFORE fire; `(frame & 0x80000007)==0` cadence, 1-cell
  radius, `TurretScansNearby` gate — absent in Rust; belongs with S4's acquisition work (it is
  pre-Foot-adjacent acquisition, not post-Foot), named gap, NOT silently absorbed here.
  `[UNITCLASS_TURRET_TRACKING §9 #7-#9]`
- **Non-Unit kill-tick drift remains** (Aircraft/Building barrels still read post-batch) —
  deferred to S7/S8, named. `[turret.rs:149 Unit skip]`
- **Dying units:** whether the tail projection rewrites a dying Unit's mission to Guard is a
  plan-time decision (G5) — gamemd does not re-derive a corpse's mission; pick the variant that
  matches and pin with a test. (Today `refresh_mission_shadow_except` iterates `values_mut()`
  with NO dying filter — [world/mod.rs:927-937](../../src/sim/world/mod.rs).)
- **In-transport Units:** excluded from the attacker snapshot (`combat/mod.rs:1424`) but NOT
  from today's facing pass — they receive idle/target facing updates while inside. gamemd
  limbos passengers (no AI pass → barrel frozen). S3 retains current behavior via the residual
  pass (named gap: passenger barrel-freeze belongs to the substrate presence/limbo model, not
  this slice). `[combat/mod.rs:1424; unit_post.rs:52 (category filter only)]`

## Design

### Components
- `combat/mod.rs`: P2 loop emits `unit_facing: Vec<(u64, u16)>` — per Unit attacker, computed
  right after its `resolve_attacker_fire` (own-retarget visible; deaths not yet applied);
  residual live-order pass for all other live Units after the loop. Emitted on the combat
  result, NOT applied inside combat.
- `unit_post.rs`: `tick_unit_facing` splits into `compute_*` (now living in/fed by the combat
  window) and `apply_unit_facing(entities, updates, rules, interner, binary_frame)` (the
  current Phase-2 apply body, unchanged math). Module doc gains the six-slot post-Foot order
  with per-slot status.
- `game_entity.rs`: `derived_mission` fall-through `(None,0)` → `(Guard,0)` gated on
  `category == Unit && !passenger_role.is_inside_transport()` (+ G5's dying decision).
- `world/mod.rs`: the `tick_unit_facing` call site becomes `apply_unit_facing(combat_result.unit_facing, …)`.
- `snapshot.rs`: `SNAPSHOT_VERSION` 20→21. Golden re-baselined with documented justification.

### Interfaces / Contracts
- The facing destinations ride the existing combat emit/result pattern (same shape as the 16
  P2 emit vecs — L2 Task 1's `CombatEmit` precedent).
- `desired_turret_facing` stays the single destination oracle; the only new logic is "if this
  attacker's own resolution emitted a retarget, compute toward the new target".
- No new phase, no `dyn`, `match category` only (invariant #3).

### Data Flow
```
Phase 5: pursuit → C4/capture →
  tick_combat_with_fog:
    P1 snapshot build (last-tick barrel state)            [unchanged]
    P2 per-attacker (live order): resolve_attacker_fire   [unchanged]
        └─ Unit? → push (id, desired_facing | own-retarget) → emit.unit_facing
    residual: every other live Unit → push (id, desired)  [new; inputs untouched by P2]
    P3-6 batched damage/deaths/clears                     [unchanged — runs AFTER reads]
  tick_turret_rotation (non-Units)                        [unchanged]
  apply_unit_facing(emit.unit_facing)                     [same site as today's tick_unit_facing]
tail: refresh_mission_shadow_except → idle Units project Guard   [D2]
state_hash
```

### Error Handling
Absent/dying entities skipped at apply (ids may die between compute and apply — apply guards
with `get_mut` as today). No new fallible paths; no RNG.

### Testing Strategy
- `kill_tick_barrel_holds_target_facing` — A kills T this tick: A's barrel destination this
  tick is toward T; next tick it is body facing. (The D1 fidelity pin.)
- `facing_apply_point_equivalence_no_kill` — scenario without deaths/retargets: per-tick
  `barrel_facing` state sequence bit-identical to pre-S3 (read-set argument measured).
- `co_attacker_facing_matches_killer` — two attackers on T; on T's death tick both hold T's
  facing destination.
- `idle_unit_derives_guard` / S2's `arrival_tick_mission_is_move_not_sleep` updated: arrival
  tick `Move`, next tick `Guard` (not `None`).
- `passenger_derive_unchanged_placeholder` — in-transport Unit still derives `None` (named
  placeholder; flips with the RE'd value later).
- `non_unit_categories_facing_unchanged` — Aircraft/Building barrel sequences bit-identical.
- `s2_tick_counter_increments_exactly_once` (existing) must stay green — D2 must not disturb
  dispatch/tail exactly-once accounting.
- Save/load round-trip on a kill tick (barrel + mission divergence window) → identical hash.
- Golden: re-baseline `global_skirmish_replay_is_deterministic_and_baseline_stable`; replay ==
  record bit-exact; document that D2 moves the hash broadly (every idle-unit tick), D1 on kill
  ticks.
- **Existing-test updates (enumerated):** `derived_mission_idle_when_no_machine_active`
  ([game_entity.rs:1079](../../src/sim/game_entity.rs) — test entity is a Unit → expectation
  None→Guard); S2's `arrival_tick_mission_is_move_not_sleep`
  ([techno_ai.rs:991](../../src/sim/world/techno_ai.rs) — post-arrival None→Guard + comment);
  stale doc comments naming the None fall-through (`mission/dispatch.rs:45`,
  S2 comments in `movement_tick.rs` near `:1047`).

## Architectural Decisions
- **Read point moves; write point stays.** The minimal change that makes the hashed outputs
  gamemd-faithful. The full physical per-object walk (movement+fire+facing in one pass) remains
  S4/S5 per the ladder; `unit_post`'s module doc already names S4 as the fire-seam relocation.
- **No facing math changes, no acquisition changes** — S4 owns passive acquire + TurretAI scan.
- **Slot scaffold as code structure + named gaps** rather than speculative implementations
  (no invented predicates for §3o/§3q/§3r).

## Alternatives Considered
- **True physical interleave now** (facing applied inside the P2 loop per attacker + residual
  pass): output-identical to the chosen approach (no cross-entity reads), but applies barrel
  writes mid-combat-loop — new borrow structure in the hottest combat path for zero output
  difference. Rejected; revisit as part of S4/S5's physical unification.
- **Idle→Guard only (skip D1):** leaves a player-visible every-kill drift verified this
  session. Rejected.
- **Map passengers to Sleep(0) now:** plausible but unverified — violates no-invented-facts.
  Named placeholder instead.
- **Absorb HarvestBrain/anim/spawn slots now:** miner substrate ownership conflict; +0x424
  target unresolved; SpawnManager system absent. Slots land as named markers.

## Open / Pre-implementation gates
- **G1 (composition spot-check):** kill-tick aim-hold composition (deferred munition + vector
  re-read + PointerExpired-at-death) — re-verify live when a Ghidra instance is available;
  docs are individually verified, the composition is this design's inference.
- **G2 (YELLOW):** exact ground-unit idle→Guard assigner (Enter-Idle equivalent) untraced;
  value Guard(5) multiply corroborated. Trace before S5 (which makes mission *handlers* real).
- **G3:** read `resolve_attacker_fire`'s three acquire sites at plan time; specify own-retarget
  facing exactly (same-tick aim at the new target = gamemd upstream-acquisition analog).
- **G4:** code-audit that nothing writes `barrel_facing` between P2 and the apply site
  (deploy/passenger/bridge systems run in other phases — confirm).
- **G5:** dying-unit projection treatment (rewrite to Guard vs freeze) — decide at plan time
  with a test; gamemd does not re-derive corpse missions.
- **Scope boundary check:** confirm S2's in-loop dispatch (scoped movers) and the S1 shadow
  asserts are unaffected by D2 (in-scope units derive Move by construction).

## Design-Review Findings (2026-06-10, same session) — verdict REVISE — ALL RESOLVED in v2

Verified against current code (`combat/mod.rs`, `world/mod.rs`, `unit_post.rs`,
`facing_class.rs`, `dispatch.rs`, `game_entity.rs`), not only the cited docs:

- **[CONFIRMED] Kill-tick clear-before-facing**: attacker-side clear `:1654-1661` (Phase 5) and
  dead-target sweep `clear_targets_on_dead_entity` `:985→:1128` (Phase 6) both run inside
  `tick_combat_with_fog`, before the facing pass at `world/mod.rs:2287`. D1's premise holds.
- **[CONFIRMED] Apply-point equivalence basis**: `FacingClass::set/current/is_rotating` are
  pure in `(state, binary_frame)` (`facing_class.rs:85/123/163`); equivalence needs only the
  G4 no-intervening-writes audit.
- **[CONFIRMED] `DispatchSlot::Guard` exists** (`dispatch.rs:24/56`) — D2 cannot route a live
  Unit to `Skip`; the S2 debug asserts stay valid.
- **[CONFIRMED] Tail projection has no dying filter** (`world/mod.rs:927-937`) — G5 is a real
  decision, recorded in the ledger.
- **[RESOLVED] Residual-pass placement was under-specified** — now stated as strictly pre-P3
  with the reason (P3-6 clears) and the in-transport snapshot exclusion (`:1424`) folded in.
- **[RESOLVED] In-transport facing treatment** — new ledger item; current behavior retained,
  passenger barrel-freeze named as a substrate-presence gap, not silently absorbed.
- **[RESOLVED] Missing test-update enumeration** — `derived_mission_idle_when_no_machine_active`
  (a Unit-category fixture) and the S2 arrival test + stale comments are now listed in Testing.
