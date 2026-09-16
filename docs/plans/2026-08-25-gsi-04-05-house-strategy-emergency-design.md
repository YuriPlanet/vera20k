# GSI-04.05 House Strategy Emergency-State Design

**Date:** 2026-08-25
**Status:** self-approved for the first implementation slice only
**Evidence:** [docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md)

## Scope

This design adds the exact House-owned state and callable action seam needed by
the `HouseClass__AI_Building_Strategy @ 0x004FD500` emergency block. It does not
claim to close GSI-04.05, activate the ordinary 106..112-frame strategy loop, or
substitute for the still-missing AI-hate, synchronous superweapon, build-need,
Manage, and BasePlan mechanisms.

The first coherent implementation slice is deliberately limited to:

- snapshot/hash-covered constructor state for `House+0x250`, `House+0x249`, and
  the signed last-Building-attack frame;
- the exact state `0/1/3/4` transition function, including its possible second
  wallet query and wrapping signed `last_attack + 900` comparison;
- the exact persistent All-To-Hunt target-bias decision helper, retained for
  the later exact candidate evaluator rather than translated into VERA's known-
  approximate nearest-first ranking;
- exact, named callable entry points for trigger action 9's state-four write and
  trigger action 6's direct All-To-Hunt request, but only when their downstream
  actions are closed in a later slice.

Activation of ordinary no-factory Fire Sale/All-To-Hunt remains gated. Running
that priority before the preceding AI-hate and synchronous superweapon stages
would change shared-Scenario RNG and object mutation order.

## Native requirement ledger

| Player-visible behavior | Native owner/order | Design disposition |
|---|---|---|
| Low total wallet enters state 1 below 25 and clears at 25 or above | Strategy, after `AI_TryFireSW` | Exact pure transition; wallet provider remains a required later connection |
| An initial/attacked base is protected from abandonment until the strict `last_attack + 900 < frame` boundary | Strategy emergency state 3 | Exact wrapping signed transition and asymmetric equality |
| State 4 performs Fire Sale then All-To-Hunt and stays 4 | Strategy direct block | State represented now; action execution remains gated until both callbacks are exact |
| No active non-limbo Factory can invoke the same action pair again | Strategy priority four | Later activation at the mutable House tail; no Health gate may be added |
| All-To-Hunt permanently biases non-designated-enemy target scores to exactly 1 | `TechnoClass__Evaluate_Candidate` | Exact persistent House latch and decision helper in this slice; connection waits for the exact evaluator |
| Campaign action 9 writes state 4; action 6 directly invokes All-To-Hunt | Trigger runtime | Data-proven campaign entries; connect only to exact House APIs, never to generic sell/attack commands |
| Team opcode 30 writes state 4 | TeamClass script VM | Data-proven executable path but absent from all retail scripts; later exact writer, no inferred ordinary-skirmish use |
| Fire Sale queues and commences Selling only for eligible Buildings | House owned-object forward order | Later action slice using mission authority, not `Command::SellBuilding` |
| All-To-Hunt reverse-scans global Techno order, detaches Teams, queues Hunt, evacuates occupied Buildings, and handles permanent-MC Insignificant objects first | House callback `0x00501400` | Later action slice after permanent-MC and Team membership are represented exactly |

## Existing architecture

- `HouseState` is the authoritative serializable per-House owner. Its manual
  fold in `world/world_hash.rs` is the lockstep hash boundary. This is the only
  suitable owner for constructor-persistent emergency state and the permanent
  target-bias latch.
- `Simulation::run_late_region` is the mutable House-tail boundary after the
  factory sweep and Team script pass. It is the eventual strategy-orchestrator
  seam.
- `ai::tick_ai` owns a separate eight-frame generic chooser and emits deferred
  commands from an immutable `Simulation`. It cannot preserve the native
  106..112-frame strategy cadence, synchronous effects, or Scenario-RNG order.
- `EntityStore` plus `LogicVector` provide deterministic entity storage and live
  global object order. All-To-Hunt requires the reverse global Techno order;
  Fire Sale requires a separate forward owner-object view.
- `mission::authority` already owns Queue/Commence semantics. Fire Sale and Hunt
  must use that authority rather than write mission fields directly.
- `production::production_sell` already has the occupant evacuation primitive
  used by `BuildingClass__SellBuilding(1,0)`, but its public `sell_building`
  wrapper also refunds/removes the Building and is therefore not reusable for
  All-To-Hunt.
- `TeamScriptVm` owns member lists but currently exposes no exact remove-member
  operation and treats opcode 30 as unsupported.
- `combat_targeting::acquire_best_target` is explicitly a nearest-first project
  substitute, not native `Evaluate_Candidate`. `calculate_ai_threat_score`
  models a different native function used only by retaliation. Neither is a
  valid place to translate the latch until the exact expanding-ring candidate
  evaluator lands.
- `GameEntity::mind_controlled` does not distinguish permanent mind control.
  The CaptureManager stores producer capacity, not the victim-side permanent
  fact required by the native precedence branch.

## Approaches considered

### 1. Extend `AiPlayerState` and emit commands

Rejected. `AiPlayerState` is not the native House authority, `tick_ai` has the
wrong cadence, and deferred commands would run Fire Sale, launch effects, and
the unconditional reschedule draw in a different order.

### 2. Add a mutable House strategy module at the late House tail

Selected. Persistent scalars live in `HouseState`; exact state transitions are
small pure helpers; authoritative callbacks accept `&mut Simulation` and run
synchronously from `run_late_region` once every preceding stage is connected.
This preserves one future call order without merging the independent
eight-frame chooser into the strategy timer.

### 3. Refactor the complete House tail in one change

Rejected for this slice. The complete path includes AI-hate, ready-Super order,
several target algorithms, nested Scenario RNG, build-need, Manage, and
BasePlan. A single patch would be difficult to review and would make failures
non-local. The architecture still reserves one orchestrator so incremental
slices cannot become competing schedulers.

## State model

Add a small serialized `HouseStrategyEmergencyState` value to `HouseState`:

- `mode: i32`, constructor/default `0`, preserving unknown signed values;
- `all_to_hunt_bias: bool`, constructor/default `false`, never cleared by the
  callback;
- `last_building_attack_frame: i32`, constructor/default `0`.

The wrapper receives `#[serde(default)]` for non-bincode construction paths, but
adding it changes the bincode record layout. Snapshot version 95 must bump to 96
so older bytes are rejected rather than misread; no unsafe old-save migration is
claimed. All three fields are folded into `hash_houses` in a fixed order.

Do not put the 106..112 strategy timer, AI-hate timer, or chooser mode inside
this value. They have separate native owners/cadences and will be added as
separate House strategy state when their complete mechanisms are implemented.

## Exact state-transition contract

The transition helper takes current state, signed current frame, signed last
Building attack frame, and a wallet-query closure. It returns the updated state
plus ordered action requests.

1. State 4 emits `FireSale`, then `AllToHunt`, stays 4, and skips wallet/deadline
   logic.
2. State 0 queries wallet. Below 25 it becomes 1 and immediately follows the
   state-1 path, causing a second query. At 25 or above it remains 0.
3. State 1 queries wallet and clears to 0 only at 25 or above.
4. State 3 computes `deadline = last_attack.wrapping_add(900)`. It clears only
   when `deadline < current_frame`; at equality it remains 3.
5. Any state other than 3 becomes 3 only when
   `current_frame < last_attack.wrapping_add(900)`. At equality it does not arm.

The helper does not execute actions, inspect factories, or draw RNG. This keeps
it testable without accidentally activating an incomplete ordinary-strategy
path.

## Target-score decision seam

Expose a pure House-state helper. If its All-To-Hunt latch is set and it has a
designated enemy, a candidate whose owner is not that exact enemy returns
`Some(1)`; otherwise it returns `None`. The rule follows later `enemy_house`
changes, does nothing while the enemy is `None`, and is not a hard rejection.

Do not wire this helper into VERA's nearest-first `(distance², class, stable id)`
ranking or its separate retaliation threat score. Either translation would
pretend two unrelated score domains were native-equivalent. The later exact
expanding-ring `Evaluate_Candidate` implementation will consume the helper
before its ordinary weighted calculation.

## Later authoritative action seam

The eventual `house_strategy` module will expose:

- `set_emergency_state_four(owner)` for trigger action 9 and Team opcode 30;
- `all_to_hunt(owner, rules)` for trigger action 6 and strategy execution;
- `fire_sale(owner, rules)`;
- `advance_strategy_house(owner, ...)`, called synchronously in House order from
  `run_late_region` after AI-hate and `AI_TryFireSW` are exact.

Those callbacks must use snapshots of the required order, then re-resolve each
entity before mutation. All-To-Hunt uses reversed global Techno order. Fire Sale
uses forward owner-object order. The later code must add a victim-side permanent
mind-control fact, exact Team removal, and an evacuation-only garrison API before
All-To-Hunt is considered complete.

## Change impact

- Save compatibility: snapshot version 96 rejects pre-field bincode layouts;
  current snapshots round-trip the additive House state.
- Lockstep: new state is manually hashed; no live target selection changes in
  the first slice because the current evaluator is known approximate.
- Runtime cost: no new live lookup or per-tick scan in the first slice.
- UI/render/audio: no direct dependency or new authority.
- Ordinary skirmish: no abandonment activation in the first slice. This avoids
  introducing a visibly wrong partial strategy while still closing the state
  and target-score substrate.

## Validation

Focused `--lib` tests must prove:

- serde round-trip and default migration shape for the new House state;
- lockstep hash changes for each field;
- state 0 below 25 performs two ordered wallet queries;
- thresholds at 24/25 and the state-3 `<`, `==`, `>` deadline boundaries;
- signed wrapping deadline behavior;
- state 4 emits exactly two ordered requests and remains 4;
- the target-bias helper is dormant with latch false or enemy absent, follows a
  changed designated enemy, and returns exactly 1 for other-owner candidates.

No full `cargo test -p vera20k --lib` run belongs to this intermediate slice;
the phase goal reserves it for the final phase-wide certification.

## Adversarial self-review and approval

Why approve: the design puts persistent state in the existing House save/hash
authority, reserves the correct mutable late-tail orchestrator, and refuses to
activate ordinary behavior before its preceding RNG-sensitive stages exist.

Largest remaining risk: the current target picker is not the native evaluator.
This review changes the design to keep the exact bias as a pure decision seam
and forbids a drive-by translation into the approximate ranking.

What could still make ordinary skirmish wrong: until the complete strategy
scheduler is activated, VERA still lacks native emergency abandonment and its
106..112-frame cadence. This document records that residual and does not call
the GSI row closed.

Most expensive later rework avoided: no new scheduler or generic command
translation is introduced. Later action and scheduling slices attach to the
same House state and late-tail seam.

**Decision:** self-approved under the autonomous goal for the first substrate
slice. GSI-04.05 remains open after that slice.
