# Phase 3 House anger-node lifecycle design

**Status:** Implemented and approved after independent read-only review on 2026-08-30.

## Goal

Close the independently active `HouseClass` anger-node arithmetic and
100-frame decay mechanism discovered under GSI-04.05, while creating the exact
shared writer required later by Strategy's AI-hate acquisition and defeated-
enemy cleanup. This slice does not schedule Strategy, arm `AIHateDelays`, or
invent a designated enemy before the verified Strategy chain exists.

## Native contract

Active-retail evidence is
[PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md), sections
7.6.4 and 10, backed by the live functions below.

- The registered House constructor cross-appends one zero-score
  `AngerStruct { HouseClass*, i32 }` for every other House in global creation
  order. House identities remain registered for the scenario lifetime.
- `HouseClass__UpdateAngerNodes @ 0x00504790` wrapping-adds the signed delta
  only to an existing matching peer node. It then scans the full vector in
  stored order, starts best score at zero, accepts only strict-greater scores,
  and excludes defeated, self/same-index, invalid-index, and allied peers.
  The first greatest positive eligible peer becomes `House+0x5600`; otherwise
  it becomes `-1`.
- `HouseClass__Update @ 0x004F8440`, before the AI-activation latch block and
  long before Strategy, tests signed `current_frame % 100 == 0`. On qualifying
  frames it decrements every peer score greater than one by exactly one in
  stored order. It does not recompute `House+0x5600`.
- Scores use wrapping signed 32-bit arithmetic. The frame authority is
  `ScenarioSession::binary_frame`, interpreted as signed `i32`; `session.tick`
  is a distinct Rust ordinal and must not drive this mechanism. Negative exact
  multiples of 100 and wrapped frame zero also qualify.

## Rust ownership and representation

`HouseState.grudge_scores` remains the sparse identity-keyed representation
already serialized and hashed. An absent registered peer is exactly native
score zero. This is equivalent because `ScenarioSession::house_order` is the
authoritative constructor order, Houses are not deleted during a scenario,
and every selection/decay walk uses that order rather than `BTreeMap` key
order. Zero deltas do not materialize absent sparse entries; an already
materialized entry remains present when it returns to zero, matching the
existing snapshot/hash contract.

Move the current exact damage-only helper from `combat` to
`sim::house_strategy` as the single shared `update_anger_nodes` authority.
Combat continues to call it at the same exact-zero/receiver-feedback seam.
Future Strategy acquisition and defeated-enemy cleanup must call this same
function; no second selector or score writer is allowed.

Add `decay_anger_scores(house, house_order, current_frame)`. It walks registered
peers in `house_order`, skips self and missing sparse entries, and wrapping-
subtracts one only from values greater than one. It never changes
`enemy_house`, never creates a sparse entry, and has no House-control,
passivity, defeat, alliance, or RuleSet gate.

## Scheduler integration

Extend the existing live forward House update pass so each represented House
runs decay before its current AI-activation transition. The pass reloads
`house_order.len()` after every House, skips missing registry entries, and
captures `session.binary_frame as i32` before the loop. Decay runs even in
rules-less Rust fixtures; activation still requires `RuleSet` as today.

Add an owner-tagged test-only trace inside the loop, with separate decay and
activation events. The test seam may inject one appended fixture House after a
chosen owner has completed so an acceptance test can prove the production loop
reloads the live `house_order.len()` rather than iterating a snapshot. This
hook is compiled out of non-test builds and may not become gameplay state.

This per-House order is deliberate. The later complete Strategy slice can be
inserted into the same House update owner without first replacing a phase-wide
bulk decay. It also pins the executable order `decay -> activation`; neither
defeat processing nor late `tick_ai` may move ahead of it.

## Exclusions

- Do not activate the AI-hate timer, first-peer acquisition, defeated-enemy
  cleanup, superweapon dispatch, emergency actions, production planning, or
  Strategy reschedule RNG. Native orders those as one later Strategy
  transaction and implementing a middle fragment would perturb state/RNG.
- Do not explicitly materialize all constructor-zero peers merely to resemble
  native storage; that would change the established Rust snapshot/hash stream
  without changing behavior.
- Do not recompute `enemy_house` during decay, remove zero entries, saturate
  scores, sort by identity, or fall back to nearest enemy.

## Acceptance tests

1. Shared writer: registered-peer validation, wrapping addition, forward
   `house_order` strict-first ties, positive-only winner, and self/defeated/
   allied/missing exclusions.
2. Sparse equivalence: zero delta to an absent registered peer remains absent;
   an existing entry updated to zero remains materialized; both still trigger
   a full enemy rescan.
3. Decay boundaries: binary frames `99`, `100`, `101`, `-100`, and wrapped
   frame zero; only exact signed multiples qualify. Scores `i32::MIN`, `0`,
   `1`, and `2` prove the strict `> 1` gate. Integration fixtures with
   `tick=100/binary_frame=99` and the inverse prove that `tick` is ignored.
4. Decay preserves the already selected `enemy_house`, including when another
   score would win if selection were recomputed.
5. Full-frame integration: an owner-tagged trace for two Houses separated by a
   missing registry slot must be `decay(A), activation(A), decay(B),
   activation(B)`, not phase-wide bulk events. A test-only post-A injection
   appends C and proves the same invocation reaches C by reloading the live
   length. Rules-less execution emits decay only. The outer trace also retains
   `House update -> defeat -> tick_ai` order.
6. Existing damage feedback, snapshot compatibility, and hash-discrimination
   tests remain green. Focused validation uses only `cargo test -p vera20k
   --lib <filter>` after confirming Cargo is idle.

## Decision

Proceed only if a fresh read-only design critic confirms that this bounded
slice is exact and does not preclude the later complete Strategy transaction.
Any uncertain ordering or representation mismatch keeps it open.
