# Ordered Reveal / Conceal / UnInit Lifecycle Authority Implementation Plan

**Date:** 2026-07-21  
**Status:** READY FOR IMPLEMENTATION for the bounded foundation; full lifecycle parity remains PARTIAL/BLOCKED  
**Authority:** `docs/contracts/2026-07-21-ordered-reveal-conceal-uninit-lifecycle-authority-implementation-contract.md`  
**Historical design:** `docs/plans/2026-05-28-logicclass-object-lifecycle-spine-design.md`, used only where it does not conflict with the July contract  
**Protected worktree scope:** preserve every pre-existing dirty path. In particular, `src/sim/world/techno_ai.rs` contains user-owned test-only work and must remain byte-for-byte untouched. The shared worktree changed while this plan was written, so implementation must trust its own execution-start status rather than this document's transient dirty-file snapshot.

## Goal

Create the smallest Rust-native lifecycle authority that preserves the verified active-YR ordering which is currently unblocked:

1. Represent native-alive, limbo, cell/Mark membership, logic membership, and pending deletion as independent facts.
2. Put every production Reveal, Object Conceal, Techno Limbo, and UnInit transition behind one world-owned transaction boundary.
3. Commit Reveal coordinates and Mark/cell state before eligible LogicVector registration, including native-shaped failure behavior.
4. Broadcast Techno BREAK synchronously in contact-slot order before Object Conceal.
5. Route immediate combat deaths and carried passengers through ordered UnInit instead of pre-clearing common lifecycle state.
6. Replace movement crush's raw store removal with an ordered lifecycle request committed by `Simulation`.
7. Run the ordinary pending-delete drain after the binary-frame commit, with a dead gate and one finalization per stable ID; delete the second app-owned flush.
8. Preserve coherent Rust snapshots and hashes for the independent lifecycle state and exactly-once owner-count bookkeeping.

This plan does **not** certify the whole lifecycle contract. It deliberately stops before complete removal listeners, ordinary Infantry death-sequence ownership, native save/load reconstruction, complete display/Anim/Voc handoffs, late-tail skip flags, or class destructor ladders.

## Compact Task Contract

| Item | Contract |
|---|---|
| Goal | Land the unblocked lifecycle-authority foundation and remove the known movement/raw-delete and ordinary-drain-order bypasses. |
| Necessary scope | Lifecycle state, LogicVector transactions, world lifecycle module, Reveal/Conceal/Techno Limbo/UnInit, represented call-site migration, immediate combat handoff, carried-passenger recursion, BREAK routing, movement requests, pending-delete drain, snapshot/hash/test updates. |
| Parity constraint | Preserve verified gamemd branch and mutation order. Any unresolved caller, upper-layer effect, listener, destructor, scheduler, or save/load behavior remains explicitly DRIFT/UNCHECKED rather than approximated. |
| Smallest validation | Focused `lifecycle_authority_` tests, existing LogicVector/radio/passenger/bunker/paradrop/crush/snapshot tests, direct-removal and direct-registration audits, then one serial `cargo check -q -p vera20k`. |
| Stop condition | The bounded tests and audits pass, the protected `techno_ai.rs` file hash is unchanged from implementation start, and no blocked behavior was silently claimed or activated. |

## Grounding Summary

- Active `gamemd.exe` keeps Object alive (`+0x90`), InLimbo, cell Mark/list membership, LogicVector membership (`+0x98`), and pending-delete membership as separate facts. Current Rust's `Presence::{Limbo, InCell, Dying}` collapses several of them.
- `ObjectClass::Reveal @ 0x005F4EC0` clears InLimbo for the attempt, commits the adjusted coordinates, calls Mark(PUT), restores InLimbo on Mark failure without restoring the old coordinates, and performs eligible logic registration only after Mark succeeds.
- Registration helper `0x0055BAA0` checks the membership byte, appends to the DynamicVector, and sets the byte only after append succeeds. Removal helper `0x0055BAE0` removes/compacts first and then clears the byte, including the flagged-but-missing case.
- `ObjectClass::Conceal @ 0x005F4D30` deselects first, removes Mark/cell state, performs its upper-layer work, unregisters eligible logic membership, conditionally dirties the tactical rect, unconditionally clears drawn state, and sets InLimbo near the end while leaving native-alive unchanged.
- Techno Limbo reaches Broadcast BREAK before Object Conceal. Broadcast at `0x0065ACE0` visits contact capacity by ascending slot and re-reads each slot. The existing Rust radio bus already clears the sender's matching slot before receiver dispatch, but receiver common clearing and non-Structure BREAK handling are incomplete.
- `ObjectClass::UnInit @ 0x005F65F0` runs class/common pre-work and removal notification, invokes virtual Limbo/Conceal while the target is still alive, clears native-alive only afterward, then appends pending delete.
- The verified base passenger hook walks carried objects in cargo order and recursively invokes each passenger's UnInit before the carrier's own removal notification, Conceal, alive clear, and queue append. Exact Capture/chrono/manager families remain outside that represented cargo slice.
- The ordinary pending-delete drain at `0x00725C70` follows the native frame increment, checks readiness/dead state, removes all duplicate queue entries for the chosen object, then finalizes/frees it once. Exact late-skip flags and concrete destructor behavior remain blocked.
- Current Rust bypasses that authority only in production movement crush: it clears contacts and calls `EntityStore::remove` directly after movement. Combat already has a world-consumed immediate-UnInit request precedent.
- Current Rust drains pending delete before committing `binary_frame`; the verified native ordinary path commits the frame first.
- The research-index validation for this topic passed. The May design is structurally useful but behaviorally stale wherever it equates logic membership with cell presence, treats Reveal/Conceal as only vector operations, or treats inline removal as output-equivalent.
- Fresh read-only decompilation spot-checks on 2026-07-21 confirmed the load-bearing anchors above. No contrary binary fact was found and no research document needed correction.

## Design Gate and Superseded Assumptions

The May design contains the required architecture context and impact analysis, and the user-approved July implementation contract supplies the current behavioral design. The following May assumptions are explicitly superseded and must not leak into implementation:

| Stale May assumption | Current required interpretation |
|---|---|
| Store presence plus LogicVector membership is enough to encode Limbo/active. | Stored, alive, limbo, cell-marked, logic-member, and pending-delete are independent axes. |
| Reveal is register; Conceal is unregister. | Reveal owns coordinate commit and Mark-before-register. Object Conceal owns deselect, unmark, logic removal, and final limbo transition. |
| Inline removal is equivalent when no current reader is known. | UnInit leaves a dead-limbo object resolvable until the ordered pending-delete drain. |
| No explicit result/error type is needed. | Reveal needs typed early-reject and Mark-failure outcomes, and failed placement retains the stored object. |
| A phased snapshot scheduler proves native visitor behavior. | Existing LogicVector mechanics are retained, but full scheduler parity remains unverified. |
| Rust save/load reconstruction is native-equivalent. | This plan makes Rust snapshots internally coherent only; native save/load remains blocked. |

## Architecture Context and Impact Analysis

The lifecycle writer belongs in a new `src/sim/world/lifecycle.rs` child module with `impl Simulation`. That module can mutate `ObjectSubstrate`, `EntityStore`, `OccupancyGrid`, `LogicVector`, and radio state without creating a dependency from `sim/` to render, UI, sidebar, audio, or net. Transaction APIs are `pub(crate)` only where another sim module must request a complete lifecycle operation. Logic append/remove, cell Mark/unmark, alive/limbo byte writes, and queue mutation remain private implementation helpers.

Data-only movement requests live in a small low-level `src/sim/lifecycle_request.rs` module. Movement may emit a request without depending on `Simulation`; the world owner consumes it immediately after movement returns. A reusable `Vec` on `Simulation` avoids a new per-tick allocation.

The following boundary is intentional:

| Surface | This plan changes | This plan does not claim/change |
|---|---|---|
| GameEntity lifecycle | Independent serialized alive, limbo, cell-marked fields; logic flag remains separate; `dying` remains death-sequence state; caller-supplied dirty-rect eligibility and exactly-once owner-count release are explicit facts | Exact native type `+0xAC` mapping, ordinary Infantry production sequencing, or a universal migration of every gameplay gate to native `IsAlive` |
| LogicVector | Fallible append; membership set after success; first-match compacting remove before flag clear | Full native scheduler/pass interleaving |
| Reveal | Coordinate commit, Mark/cell add, display-boundary marker, eligible logic append, failure rollback | Exact `Can_Enter_Cell`, editor/game-mode gates, or live failed-GACNST caller plumbing |
| Conceal/Limbo | Deselect, unmark, release-visible ordered upper-layer outputs, logic unregister, conditional dirty-rect handoff, unconditional drawn-state clear, limbo set; Techno BREAK before common Conceal | Exact type `+0xAC` eligibility mapping and DisplayClass, attached Anim, Voc, AlphaShape/LineTrail, dirty-rect pixel/audio consumers |
| UnInit | Explicit represented class-pre, carried-passenger recursion, removal-notify boundary, virtual Limbo, alive clear, queue append | Complete listener roster and Capture/Temporal/Spawn/DiskLaser managers |
| Pending delete | Serialized/hashed queue authority; dead gate; duplicate collapse; one common physical removal; ordinary frame-before-drain order | Native exceptional skip flags, alive-byte restore before concrete destructors, leaf destructors/finalizers |
| AnimClass | Separate logic-only reveal/conceal path sharing LogicVector and pending-delete drain | Techno radio/cell semantics or complete attached-owner Anim lifetime |
| Movement crush | Ordered UnInit request; no raw store/contact removal; world applies immediately after movement | Exact native within-object crush timing or moving the whole movement loop into the LogicClass pass |
| App/Infantry | Remove caller-side occupancy cleanup and the second app flush; app completion may request central UnInit temporarily | Ordinary Infantry production sequence ownership and removal of the final app-owned UnInit request |

## Key Technical Decisions

| Decision | Confidence and source |
|---|---|
| Replace `Presence` with three independent authoritative fields: `object_alive`, `in_limbo`, and `cell_marked`; retain `in_logic_vector`, `dying`, and pending queue as separate facts. | HIGH; lifecycle, cell-writer, registration, UnInit, and pending-delete reports; July contract lines 154 and 167 |
| Keep `GameEntity::is_active()` as an explicitly transitional Rust gameplay gate (`object_alive && !dying`) for this slice; add `is_object_alive()` for native-alive checks. Do not activate ordinary Infantry semantics without its production host. | BOUNDED safety decision; July contract lines 160 and 199; current app-owned death path remains blocked |
| Leave the current `is_alive()` health predicate behavior unchanged and document that it is not the native Object alive byte. Lifecycle transitions use `is_object_alive()`/`object_alive`, never positive health as a substitute. | HIGH; native alive and health-zero death-sequence state are independently observable |
| Serialize and hash the pending-delete queue instead of assuming it is empty at every snapshot boundary. | HIGH for Rust snapshot coherence; required independent pending fact and pending-boundary acceptance. This is not a native save/load claim. |
| Use a result-bearing Reveal request containing committed coordinates, caller-supplied placement evidence, and caller-supplied logic eligibility. | HIGH for ordering; BOUNDED for the supplied admission/eligibility inputs because the exact placement/type/mode oracle is blocked |
| A successful Mark followed by a failed logic append remains successfully revealed/marked; it is not rolled back. | HIGH; registration helper result is not a Reveal rollback condition |
| Add a test-only fallible LogicVector append seam after `try_reserve`, set membership only after append succeeds, and remove one first match before clearing membership. | HIGH; helpers `0x0055BAA0` and `0x0055BAE0` |
| Iterate BREAK with `for slot in 0..capacity`, re-reading `Contacts::slot(slot)` immediately before each transmit. | HIGH; Broadcast `0x0065ACE0`; preserves mutation visibility and avoids a sorted/BTreeMap scan |
| Run class-specific represented BREAK effects before one common receiver-slot clear for every GameEntity category. | HIGH for represented order; Building GrandOpening and conditional `0x19` remain blocked |
| Preserve current Rust-only cleanup fragments behind a named represented UnInit pre-hook; do not relabel their exact order as native. | BOUNDED; prevents broad behavior churn while the listener/destructor contracts remain incomplete |
| Append duplicate UnInit queue IDs without suppression; the drain removes all duplicates for the first ready occurrence and finalizes once. | HIGH; native queue append and drain behavior |
| Keep movement's current sound-before-health/request order and current pre-removal of crushed occupancy as a named UNCHECKED timing exception. | BOUNDED; raw physical removal is proven wrong, but exact native crush interleaving is not yet verified |
| Commit `total_sim_ms`/`binary_frame` before the ordinary drain, while leaving the relative placement of Rust's separate `session.tick` assignment unchanged. | HIGH for native frame-before-drain; UNCHECKED for Rust `session.tick`, which has no direct native equivalent |
| Emit release-visible, serde-skipped `LifecycleOutput` values at upper-layer slots and use a separate `#[cfg(test)]` ledger for internal state order. Outputs expose the exact handoff sequence but do not implement their blocked consumers. | HIGH for the ordered seam; explicitly not pixel/audio parity certification |
| Keep tactical dirtying and drawn-state clearing as two distinct outputs: dirtying is conditional on an explicit represented eligibility fact, while drawn-state clearing is unconditional. | HIGH for order/conditionality from `ObjectClass::Conceal`; BOUNDED for the supplied eligibility fact because exact native type `+0xAC` mapping is blocked |
| Add serialized/hashed `owned_count_released` bookkeeping and funnel every owner-count decrement through `release_owned_count_once`. Deferred animation may release at its existing start-time handoff; later UnInit becomes a no-op for that count. | BOUNDED timing preservation plus an exact Rust exactly-once invariant; this does not claim the full native owner-count sub-order |

## Open Questions and Explicit Non-Claims

These do not block the bounded implementation, but none may be silently answered by convenience:

- **Placement oracle — BLOCKED.** Exact `Can_Enter_Cell`, coordinate adjustment, editor, mode, and type gates need a separate contract. Current callers pass their already-computed admission result into Reveal.
- **Logic eligibility — BOUNDED INPUT.** Reveal accepts the eligibility result; it does not invent the exact native `+0x234`/type/mode predicate.
- **Failed GACNST redeploy caller — FOLLOW-UP.** The transaction supports and tests retained alive-limbo Mark failure. Rewiring the full building-down/redeploy production caller is outside this foundation.
- **Upper-layer Conceal consumers — BLOCKED.** This plan emits release-visible display/attached-Anim/Voc/optional-dirty/unconditional-drawn-clear/redraw outputs at the verified slots, but it does not implement their pixel/audio consumers or infer the native type `+0xAC` dirty-eligibility oracle.
- **Removal listeners — BLOCKED.** The production removal-notify stage initially has only an ordered seam and already represented cleanup. No blanket ID scan or speculative field clearing is allowed.
- **Passenger class census/managers — PARTIAL.** Represented `PassengerCargo` recursion lands now. The exact AbstractFlags class inventory plus CaptureManager/chrono/deploy and other Foot wrapper objects remain blocked.
- **Ordinary Infantry — BLOCKED.** `src/sim/world/techno_ai.rs` remains protected and untouched. Production Mission/Foot sequence ownership and exact death-animation cadence remain outside this plan. Combat and `src/app_sim_tick.rs` receive only the surgical lifecycle-boundary edits named below: remove immediate precleanup/second drain and route the existing completion through central UnInit. The temporary logic-only unregister used by the app-owned sequence remains explicitly DRIFT.
- **App-owned Infantry completion — PARTIAL.** This plan removes caller-side occupancy mutation and the second app flush. The app still requests central UnInit after its current animation completion until Mission/Foot production authority lands.
- **Destructor/finalizer behavior — BLOCKED.** The new drain has a named common-finalization boundary but does not restore native alive or emulate concrete class destructors.
- **Late skip flags — BLOCKED.** The ordinary path is corrected; the four native flags that skip both frame increment and drain remain unrepresented.
- **Native save/load — BLOCKED.** Snapshot v28 is a coherent Rust snapshot format, not evidence of native vector/list reconstruction.
- **Crush interleaving — UNCHECKED.** Requests are committed after the complete movement call, not at the exact native object's AI/locomotor point.
- **Direct non-Reveal LogicVector callers — OUT OF SCOPE.** OpenTopped, WaveClass, BuildingLightClass, and other absent/unrepresented classes need separate contracts.

## File Map

### Add

- `src/sim/world/lifecycle.rs`
  - Lifecycle transaction types and `impl Simulation` for logic membership, Mark/unmark, Reveal, Object Conceal, Techno Limbo, Anim reveal/conceal, staged UnInit, and pending-delete processing.
- `src/sim/world/lifecycle_tests.rs`
  - Focused ordered state-product, Reveal, Conceal/BREAK, UnInit, drain, and tail-order tests using the common prefix `lifecycle_authority_`.
- `src/sim/lifecycle_request.rs`
  - Data-only, allocation-free `LifecycleRequest`/reason vocabulary shared by movement and world.

### Modify

- `src/sim/mod.rs`
  - Declare the data-only lifecycle-request module.
- `src/sim/game_entity.rs`
  - Remove `Presence`; add the independent lifecycle state, caller-supplied dirty-rect eligibility, and exactly-once owner-count-release bookkeeping; split native-alive query from the transitional gameplay-active query; update defaults and unit tests.
- `src/sim/world/mod.rs`
  - Declare/re-export lifecycle APIs; add release lifecycle outputs, reusable request storage, and test trace storage; remove inline lifecycle primitives; split immediate versus deferred combat handoff; consume movement requests; rebuild only derived logic membership; commit frame before ordinary drain; update invariants/comments.
- `src/sim/world/substrate.rs`
  - Make pending-delete queue serialized authority and correct its stale transient/Presence comments.
- `src/sim/world/logic_vector.rs`
  - Add fallible tail append, test failure seam, first-match compacting removal, and matching serde/default behavior.
- `src/sim/world/world_spawn.rs`
  - Split store/construction from result-bearing placement; remove `active: bool`; use Mark-before-register Reveal.
- `src/sim/occupancy.rs`
  - Rebuild only entities whose lifecycle says they are cell-marked; preserve enter order.
- `src/sim/world/world_hash.rs`
  - Hash independent lifecycle fields, death-sequence state, dirty-rect eligibility, owner-count-release state, and pending-delete order.
- `src/sim/snapshot.rs`
  - Bump v27 to v28; replace Presence tests with independent-state, bookkeeping-state, and pending-boundary round trips.
- `src/sim/radio/mod.rs`
  - Add ascending-slot BREAK broadcast; retain sender-clear-before-dispatch behavior; add test trace points.
- `src/sim/radio/receive.rs`
  - Move common receiver contact clearing after represented class effects and apply it to Unit/Infantry/Aircraft receivers too.
- `src/sim/entity_store.rs`
  - Relabel the BTreeMap scrub as a legacy non-lifecycle cleanup only; lifecycle code may no longer call it.
- `src/sim/anim_class.rs`
  - Use Anim-specific reveal/conceal transactions; preserve shared pending-delete storage.
- `src/sim/passenger.rs`
  - Add ordered cargo extraction for recursive UnInit; route both boarding paths through Techno Limbo; route unload/eject placement through result-bearing Reveal.
- `src/sim/aircraft/drop_payload.rs`
  - Replace success-path manual occupancy plus skeletal Reveal with the transaction. Retain separately classified failure cleanup until its contract exists.
- `src/sim/docking/bunker_link.rs`
  - Route install hide through Techno Limbo and release placement through Reveal.
- `src/sim/production/production_sell.rs`
  - Route garrison eject placement through Reveal; detach destroyed-garrison cargo before building UnInit; route no-exit occupant removal through UnInit.
- `src/sim/combat/mod.rs`
  - Stop pre-clearing common lifecycle state and transport passengers; leave immediate objects intact for world-owned UnInit while retaining only the blocked animated-death marker/sequence selection.
- `src/sim/combat/combat_tests.rs`
  - Replace pre-UnInit cleanup expectations with immediate-request and deferred-animation expectations.
- `src/app_sim_tick.rs`
  - Remove caller-side occupancy deletion and the second pending-delete flush; request central UnInit on current animation completion and drain/acknowledge lifecycle outputs without implementing blocked consumers.
- `src/sim/movement/mod.rs`
  - Re-export/use the lifecycle request vocabulary as needed without putting a `Vec` inside `MovementTickStats`.
- `src/sim/movement/movement_tick.rs`
  - Emit a deduplicated crush UnInit request instead of clearing contacts/removing storage.
- `src/sim/movement/movement_tests.rs`
  - Replace raw-removal expectations with request-production and world-consumption expectations.
- `src/sim/movement/prone_speed_tests.rs`
  - Thread the explicit lifecycle-request sink through the direct movement entry-point fixture.
- `src/sim/miner/miner_tests.rs`
  - Thread the explicit lifecycle-request sink through every direct `tick_movement` / `tick_movement_with_grid` fixture.
- `src/sim/world/world_tests.rs`
  - Update lifecycle/drain tests and stale frame/drain comments.
- Production and test fixtures in `src/sim/docking/bunker_install.rs`, `src/sim/world/world_commands.rs`, and relevant inline test modules
  - Replace test-only calls to removed raw registration/occupancy helpers with lifecycle fixtures or explicit `#[cfg(test)]` helpers.

### Read-only / protected

- `src/sim/world/techno_ai.rs`
  - Existing uncommitted test-only work. Do not edit, format, stage, or use as production authority.
- `ini/rules.ini`, `ini/rulesmd.ini`, `ini/art.ini`, `ini/artmd.ini`
  - No direct lifecycle key drives the common mechanisms. Failed-redeploy stock keys are evidence fixtures only, not new hardcoded behavior.

## Planned Interfaces

Names may be adjusted for Rust style during implementation, but the state and ordering semantics may not change.

### Independent entity state

In `src/sim/game_entity.rs`:

```rust
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    serde::Serialize, serde::Deserialize,
)]
pub struct ObjectLifecycle {
    pub object_alive: bool,
    pub in_limbo: bool,
    pub cell_marked: bool,
}

impl Default for ObjectLifecycle {
    fn default() -> Self {
        Self {
            object_alive: true,
            in_limbo: true,
            cell_marked: false,
        }
    }
}
```

`GameEntity` keeps:

```rust
pub in_logic_vector: bool; // derived from serialized LogicVector after load
pub lifecycle: ObjectLifecycle;
pub dying: bool;           // death-sequence/render state, not native IsAlive
#[serde(default)]
pub dirty_rect_eligible: bool; // caller-supplied type fact; exact +0xAC oracle blocked
#[serde(default)]
pub owned_count_released: bool; // Rust exactly-once destruction bookkeeping
```

Queries are deliberately separate:

```rust
pub fn is_object_alive(&self) -> bool {
    self.lifecycle.object_alive
}

/// Transitional Rust system gate until ordinary Infantry authority migrates.
pub fn is_active(&self) -> bool {
    self.lifecycle.object_alive && !self.dying
}
```

Fresh construction initializes both added bookkeeping facts to `false`. `dirty_rect_eligible` may be set only from an explicit represented caller/type fact; it must not be guessed from category, voxel/SHP representation, selection, or current render state. Until the native type `+0xAC` source is mapped, any constructor without positive evidence leaves it false and remains an explicit upper-layer parity gap. Because both fields affect future deterministic lifecycle behavior, they are serialized and hashed.

No method may derive limbo or cell membership from `in_logic_vector`, store presence, health, or `dying`. `owned_count_released` is not a substitute for native-alive or `dying`; it guards only the existing Rust owner-count decrement.

### Result-bearing Reveal

In `src/sim/world/lifecycle.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementEvidence {
    RejectedEarly,
    MarkFailed,
    MarkSucceeded,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RevealPosition {
    /// Isometric map-cell coordinates, not screen axes.
    pub rx: u16,
    pub ry: u16,
    /// Current Rust height level, not pixels or leptons.
    pub z: u8,
    /// Lepton offsets inside the cell (256 leptons per cell).
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RevealRequest {
    pub position: RevealPosition,
    pub placement: PlacementEvidence,
    pub logic_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealFailure {
    MissingObject,
    NotAlive,
    RejectedEarly,
    MarkFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealOutcome {
    Revealed { logic_registered: bool },
    AlreadyRevealed,
    Failed(RevealFailure),
}
```

The transaction signature is:

```rust
pub(crate) fn try_reveal_entity(
    &mut self,
    stable_id: u64,
    request: RevealRequest,
) -> RevealOutcome;
```

Its required algorithm is:

1. Reject missing/dead objects and return idempotently for an already non-limbo object.
2. Return `RejectedEarly` before changing limbo or coordinates.
3. Set `in_limbo=false` for the attempt.
4. Commit the supplied adjusted position and refresh cached screen coordinates.
5. On `MarkFailed`, restore `in_limbo=true`, retain the new coordinates, and return with no cell/display/logic membership.
6. On `MarkSucceeded`, add all entity cells, assign enter order, then set `cell_marked=true`.
7. Emit the release-visible `RevealDisplay` output and matching test event. Do not implement an upper-layer effect consumer in this slice.
8. If eligible, attempt fallible LogicVector append and set `in_logic_vector=true` only after success.
9. Return `Revealed { logic_registered }`. Logic append failure does not undo coordinates, Mark, or Reveal success.

### LogicVector transaction

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicInsertError {
    Capacity,
    ForcedTestFailure,
}

pub(crate) fn try_push(&mut self, id: u64) -> Result<(), LogicInsertError> {
    #[cfg(test)]
    if std::mem::take(&mut self.fail_next_insert) {
        return Err(LogicInsertError::ForcedTestFailure);
    }
    self.order
        .try_reserve(1)
        .map_err(|_| LogicInsertError::Capacity)?;
    self.order.push(id);
    Ok(())
}

pub(crate) fn remove_first(&mut self, id: u64) -> bool {
    let Some(index) = self.order.iter().position(|&candidate| candidate == id) else {
        return false;
    };
    self.order.remove(index);
    true
}
```

The lifecycle helper checks `in_logic_vector` first. On add it calls `try_push` and sets the flag afterward. On removal it calls `remove_first` and clears the flag afterward even if the ID was flagged but absent.

### Object Conceal, Techno Limbo, and Anim path

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcealOutcome {
    Concealed,
    AlreadyConcealed,
    MissingOrDead,
}

pub(crate) fn object_conceal(&mut self, stable_id: u64) -> ConcealOutcome;
pub(crate) fn techno_limbo(&mut self, stable_id: u64) -> ConcealOutcome;
pub(crate) fn reveal_anim(&mut self, stable_id: u64) -> bool;
pub(crate) fn conceal_anim(&mut self, stable_id: u64) -> bool;
pub(crate) fn legacy_unregister_logic_only_for_app_death(&mut self, stable_id: u64);
```

`object_conceal` orders the represented common transition as:

1. dead/already-limbo gate;
2. `selected=false`;
3. unmark/remove occupancy if `cell_marked`, then clear that fact;
4. emit `DisplayRemove`, `DetachAttachedAnims`, and `StopVoc` release outputs in that order;
5. compact LogicVector membership, then clear `in_logic_vector`;
6. if the entity's explicit `dirty_rect_eligible` fact is true, emit `DirtyTacticalRect`;
7. unconditionally emit `ClearDrawnState`;
8. set `in_limbo=true`;
9. emit `ClearRedraw` last.

`techno_limbo` broadcasts BREAK while the sender is still alive and marked, then delegates to `object_conceal`. `AnimClass` uses only `reveal_anim`/`conceal_anim`; it never receives cell or Techno radio behavior.

`legacy_unregister_logic_only_for_app_death` exists solely to preserve the current blocked app-owned animated-death handoff at `world/mod.rs`'s combat-result consumer. It compacts LogicVector and clears only `in_logic_vector`, leaving native-alive, limbo, and cell-marked facts untouched. This accurately records the current transitional state without exposing a raw vector primitive or pretending it matches native ordinary Infantry behavior. It must have exactly one production caller and must disappear with the later Infantry/Mission-authority slice.

### Release-visible upper-layer handoff

Add a data-only output vocabulary in `world/lifecycle.rs` and a serde-skipped reusable vector on `Simulation`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleOutput {
    RevealDisplay { stable_id: u64 },
    DisplayRemove { stable_id: u64 },
    DetachAttachedAnims { stable_id: u64 },
    StopVoc { stable_id: u64 },
    DirtyTacticalRect { stable_id: u64 },
    ClearDrawnState { stable_id: u64 },
    ClearRedraw { stable_id: u64 },
}

#[serde(skip)]
pub(crate) lifecycle_outputs: Vec<LifecycleOutput>,
```

Re-export `LifecycleOutput` from `world/mod.rs` for the app boundary. Reveal emits `RevealDisplay` after Mark succeeds and before logic append. Object Conceal emits `DisplayRemove`, `DetachAttachedAnims`, and `StopVoc`, then—after logic unregister—optionally emits `DirtyTacticalRect`, always emits `ClearDrawnState`, sets limbo, and finally emits `ClearRedraw`. `src/app_sim_tick.rs` drains these outputs in order with an exhaustive, explicitly no-op match until each blocked render/audio consumer has its own contract. The output stream is release-visible ordering infrastructure, not evidence that the pixel/audio effects exist.

### Movement lifecycle request

In `src/sim/lifecycle_request.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UninitReason {
    Crush,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleRequest {
    Uninit { stable_id: u64, reason: UninitReason },
}
```

`Simulation` owns:

```rust
#[serde(skip)]
pending_lifecycle_requests: Vec<LifecycleRequest>,
```

`tick_movement_with_grids`, `tick_movement_with_grid`, and `tick_movement` all receive an explicit `&mut Vec<LifecycleRequest>` sink. No wrapper may create and discard a temporary sink. Movement pushes after the current crush sound and health-zero writes, and never removes storage or bulk-clears radio. `Simulation` applies the requests immediately after `tick_movement_with_grids` returns with a borrow-safe `mem::take`/restore pattern:

```rust
let mut requests = std::mem::take(&mut self.pending_lifecycle_requests);
for request in requests.drain(..) {
    self.apply_lifecycle_request(request);
}
debug_assert!(requests.is_empty());
self.pending_lifecycle_requests = requests; // restore retained capacity
```

### Staged UnInit and drain

The new UnInit implementation has these named stages:

```rust
pub(crate) fn uninit(&mut self, stable_id: u64) {
    // 1. Existing represented class/Rust bookkeeping hook, including
    //    release_owned_count_once.
    // 2. Snapshot/detach carried cargo and recursively UnInit each passenger.
    // 3. Removal-notification boundary (test-visible; roster blocked).
    // 4. Virtual Techno Limbo -> BREAK then Object Conceal.
    // 5. object_alive=false; keep dying as the transitional system gate.
    // 6. Append pending delete last, with no duplicate suppression.
}
```

Owner-count release has one authority:

```rust
pub(crate) fn release_owned_count_once(&mut self, stable_id: u64);
```

The helper returns for a missing entity or when `owned_count_released` is already true. Otherwise it captures the represented owner/category inputs, sets `owned_count_released=true` before mutating the house count, and performs the existing decrement exactly once. Immediate UnInit calls it from the represented pre-hook. The still-blocked deferred animated-death handoff calls it at the existing animation-start timing before its legacy logic-only unregister; eventual UnInit calls the same helper and therefore cannot decrement twice. Recursive passenger UnInit also reaches this authority. No owner-count decrement may remain outside this helper, and `dying` must never be used as the guard.

The represented cargo hook uses `std::mem::take` on `PassengerCargo.passengers`, resets `total_size` and `garrison_fire_index`, and preserves the cargo's configured `capacity`/`size_limit`, then walks the captured IDs in stored cargo order. For each existing passenger it clears only a matching represented `Inside { transport_id }` relation, sets health to zero as the existing Rust carrier-death outcome, and calls that passenger's own `uninit` before continuing the carrier. It must not clear selection, attack target, movement target, radio, cell state, alive, or logic outside the passenger's lifecycle transaction. Garrison destruction must detach/eject its occupant cargo before the building reaches this hook, because garrison occupants use the separately verified SellBuilding-style path rather than transport-death recursion.

The ordinary drain uses queue order and one-finalization-per-ID:

```rust
pub(crate) fn process_pending_delete(&mut self) {
    let mut index = 0;
    while index < self.substrate.pending_delete.len() {
        let id = self.substrate.pending_delete[index];
        if !self.pending_object_is_ready(id) {
            index += 1;
            continue;
        }

        self.substrate.pending_delete.retain(|&queued| queued != id);
        self.finalize_and_remove_common(id);
    }
}
```

`pending_object_is_ready` means `!entity.lifecycle.object_alive` for a GameEntity, `anim.runtime.inactive` for an Anim, and defensively ready/discardable for a missing stable ID. `finalize_and_remove_common` is the only production location allowed to call `EntityStore::remove` or `AnimStore::remove`. It is not described as a complete native destructor.

Remove the old `flush_pending_delete()` production API after updating tests and the app path. The ordinary sim tail is the only production caller of `process_pending_delete()`.

### Test-only order ledger

Under `#[cfg(test)]`, add a serde-skipped vector on `Simulation` and a compact `pub(super)` event enum in `world/lifecycle.rs` so the parent `world/mod.rs` can type its field. Every production-stage recording statement must itself be wrapped in `#[cfg(test)]`; do not leave an unguarded call to a test-only method in release code. At minimum record:

```rust
enum LifecycleTestEvent {
    RevealLimboCleared,
    RevealCoordinatesCommitted,
    MarkPut,
    CellMarked,
    RevealDisplayBoundary,
    LogicAppended,
    LogicMembershipSet,
    BreakSlot { slot: usize, target: u64 },
    BreakSenderCleared { target: u64 },
    BreakReceiverClassEffect { target: u64 },
    BreakReceiverCleared { target: u64 },
    ConcealDeselected,
    ConcealUnmarked,
    ConcealDisplayBoundary,
    ConcealAnimBoundary,
    ConcealVocBoundary,
    ConcealLogicRemoved,
    ConcealDirtyTacticalRectBoundary,
    ConcealClearDrawnStateBoundary,
    ConcealLimboSet,
    ConcealClearRedrawBoundary,
    UninitClassPre,
    UninitRemovalNotifyBoundary,
    UninitAliveCleared,
    PendingDeleteQueued,
    BinaryFrameCommitted,
    PendingDeleteDrainStarted,
    FinalizedCommon { stable_id: u64 },
}
```

The test ledger proves internal writes and release-output emission order. Upper-layer `Boundary` events plus matching `LifecycleOutput` values still do not prove that a renderer/audio consumer performed the effect.

## Simulation Checklist

- [ ] No floating-point arithmetic is introduced into simulation state or lifecycle decisions.
- [ ] Stable-ID and contact-slot iteration order is preserved; no BTreeMap scan substitutes for radio Broadcast.
- [ ] Reveal and drain allocate only at non-hot structural boundaries; movement reuses its request buffer.
- [ ] No RNG stream is read or advanced by lifecycle work.
- [ ] No `sim/` dependency on render, UI, sidebar, audio, or net is added.
- [ ] LogicVector append/remove and membership-byte writes occur in verified order.
- [ ] Conditional tactical dirtying and unconditional drawn-state clearing remain separate, ordered outputs.
- [ ] Every owner-count decrement goes through the serialized/hashed exactly-once guard; `dying` is never that guard.
- [ ] Coordinate writes, Mark/cell writes, and rollback retain native signed/fixed-point inputs supplied by the caller.
- [ ] Snapshot version and every authoritative hash input change are deliberate and reviewed.
- [ ] Existing user edits in `techno_ai.rs` remain untouched.
- [ ] Rust regression hashes are not described as gamemd parity evidence.

## Risk Areas

### High risk

- Snapshot/hash churn from removing `Presence`, adding lifecycle/bookkeeping facts, and serializing pending delete. Do not rebaseline unrelated goldens or run two rebaseline owners concurrently.
- Borrow ordering in BREAK broadcast and Reveal/Conceal. Never hold an entity borrow across synchronous `transmit` or another `&mut Simulation` call.
- Production caller migration: passenger, paradrop, bunker, and garrison paths currently mutate occupancy in different orders. Preserve caller-local prechecks and non-common side effects; move only the common lifecycle writes.
- Ordinary Infantry is not ready. Do not use the new native-alive byte to activate its production object-AI/death path in this plan.
- Owner-count release currently straddles immediate and deferred death timing. Centralize the mutation before deleting either handoff; otherwise a later UnInit can double-decrement.

### Medium risk

- Logic append failure is difficult to trigger naturally. The test seam must not exist or branch in release builds beyond the real `try_reserve` path.
- Multi-cell Mark/unmark must update all foundation cells exactly once and keep `occupancy_enter_order` stable.
- An alive queued fixture means pending delete is not always empty; comments, serialization, hashing, and tests must agree.
- Movement crush removes occupancy earlier than the world request. Keep `cell_marked` coherent at that existing removal point and label exact timing UNCHECKED.

### Low risk

- Module extraction and test-only wrapper renames, provided the direct-primitive audits pass.
- Moving binary-frame commit immediately before the ordinary drain, because all earlier tick consumers still observe the pre-increment frame.

## Parity-Critical Items

The implementation review must reject the patch if any of these occur:

- `in_logic_vector` is set before LogicVector append succeeds.
- Reveal registers logic before Mark/cell membership.
- Mark failure restores old coordinates, deletes the object, or leaves logic/cell membership.
- Conceal sets limbo or native-alive before radio/Mark/logic steps.
- Techno BREAK uses entity-store order, sorted contact IDs, or a snapshot that hides synchronous slot mutation.
- Receiver contact clearing happens before represented class-specific BREAK work.
- Conceal merges dirtying with drawn-state clearing, emits dirtying without positive eligibility, omits unconditional drawn-state clearing, emits either before logic unregister, or emits redraw-clear before limbo.
- UnInit clears native-alive before virtual Limbo/Conceal or appends pending delete before alive clear.
- A carrier reaches removal notification/Conceal before its captured passengers have each completed their own UnInit enqueue.
- Immediate, deferred, recursive, or duplicate UnInit can decrement the represented owner count more than once for one entity.
- Combat or app code pre-deselects, pre-unmarks, pre-unregisters an immediate death, or clears other objects' references before the UnInit boundary.
- A production subsystem physically removes an entity/Anim outside the drain.
- The ordinary drain runs before `binary_frame` commit, drops a non-dead queued object, or finalizes a duplicate more than once.
- Any app-owned second pending-delete drain remains.
- Snapshot rebuild adds every stored object to occupancy regardless of `cell_marked`.
- Any blocked listener, destructor, upper-layer consumer, Infantry Mission owner, save/load, or scheduler behavior is marked MATCH/VERIFIED.

## Task 0: Protect the Baseline and Revalidate Before Editing

**Files:** read-only whole tree; protect every execution-start dirty path, especially `src/sim/world/techno_ai.rs` and the currently unrelated `src/app_init.rs`.

At implementation start:

1. Run `git status --short` and record the current HEAD.
2. Record `git hash-object src/sim/world/techno_ai.rs` in the implementation notes. At plan-writing time it is `3dde2a4262f712a4659e557945997a26e188e790`; use the execution-start value if the user has changed it since.
3. Record every pre-existing dirty path and preserve each one. The shared worktree advanced repeatedly through unrelated `src/app_init.rs` commits/edits during plan review; do not assume any plan-writing HEAD or dirty-file snapshot remains current. Never clean, reset, format, stage, or edit an unrelated execution-start dirty path.
4. Re-run the research-index validation/handoff if any cited research document or contract has changed since 2026-07-21.
5. Before any Cargo run, check:

   ```powershell
   Get-Process cargo,rustc -ErrorAction SilentlyContinue |
       Select-Object ProcessName,Id,CPU
   ```

**Stop:** if a conflicting lifecycle implementation or snapshot rebaseline is already in progress, reconcile ownership before editing.

## Task 1: Split and Persist the Independent Lifecycle Axes

**Files:** `src/sim/game_entity.rs`, `src/sim/world/substrate.rs`, `src/sim/occupancy.rs`, `src/sim/world/world_hash.rs`, `src/sim/snapshot.rs`, `src/sim/world/mod.rs`.

1. Add `ObjectLifecycle` and replace the serde-skipped `Presence` field.
2. Initialize every newly constructed GameEntity as alive + limbo + unmarked + non-logic-member, with `dirty_rect_eligible=false` unless an explicit represented type fact says otherwise and `owned_count_released=false` always.
3. Delete `derived_presence()` and replace its tests with an independent state-product test.
4. Add `is_object_alive()` and keep `is_active()` transitional as specified above.
5. Remove `#[serde(skip)]` from `ObjectSubstrate::pending_delete`; update the comments so the queue may survive a Rust snapshot boundary.
6. Change `OccupancyGrid::rebuild` to skip every entity with `!entity.lifecycle.cell_marked` before checking passenger/layer state.
7. Change load repair so `rebuild_logic_membership()` rebuilds only `in_logic_vector` from the serialized LogicVector. It must not derive alive, limbo, or cell state.
8. Replace `debug_assert_presence_consistent` with lifecycle invariants that permit active-off-cell, marked-but-non-logic, health-zero/native-alive, and dead-limbo-pending states. Still reject duplicate/mismatched LogicVector membership and a `cell_marked` entity missing its expected cached occupancy cells.
9. Hash `object_alive`, `in_limbo`, `cell_marked`, `dying`, `dirty_rect_eligible`, and `owned_count_released` at a fixed position in each entity fold. At one fixed substrate fold position, hash `pending_delete.len()` followed by every queued ID in insertion order; never hash IDs without the length delimiter.
10. Bump `SNAPSHOT_VERSION` from 27 to 28 and update only version-owned fixtures.

Add/replace tests named:

- `lifecycle_authority_state_axes_are_independent`
- `lifecycle_authority_alive_limbo_does_not_rebuild_occupancy`
- `lifecycle_authority_pending_boundary_roundtrips_queue_and_state`
- `lifecycle_authority_logic_rebuild_does_not_rederive_limbo_or_mark`
- `lifecycle_authority_each_axis_changes_state_hash`
- `lifecycle_authority_bookkeeping_facts_roundtrip_and_change_state_hash`

Focused verification:

```powershell
cargo test -p vera20k lifecycle_authority_state_axes -- --nocapture
cargo test -p vera20k lifecycle_authority_alive_limbo -- --nocapture
cargo test -p vera20k lifecycle_authority_pending_boundary -- --nocapture
```

Do not update unrelated hash goldens until the full lifecycle patch is assembled and reviewed.

## Task 2: Add the Private Lifecycle Module and Transactional LogicVector

**Files:** add `src/sim/world/lifecycle.rs`, add `src/sim/world/lifecycle_tests.rs`; modify `src/sim/world/mod.rs`, `src/sim/world/logic_vector.rs`.

1. Declare `mod lifecycle;` and `#[cfg(test)] mod lifecycle_tests;` near the existing world module list.
2. Move inline registration/removal, Reveal/Conceal, occupancy Mark helpers, UnInit, and drain logic out of `world/mod.rs` into the new module as staged work; keep the old bodies only until each task compiles, then delete them.
3. Implement `LogicVector::try_push` using `try_reserve`, plus the `#[cfg(test)] fail-next-insert seam.
4. Replace `retain` removal with first-match `position` + `Vec::remove` and a boolean result.
5. Implement private entity/Anim logic helpers with verified flag ordering.
6. Add test-only helpers for existing scheduler/fixture tests. Production modules may not call raw registration or raw cell Mark helpers.
7. Add the serde-skipped test ledger to `Simulation` under `#[cfg(test)]`; initialize it only in test builds.
8. Replace only the deferred animated-death world call to `unregister_live_object` with the single named `legacy_unregister_logic_only_for_app_death` transaction. Immediate deaths must route directly through ordered UnInit as specified in Task 6.
9. Update comments on `for_each_live_object` to describe exactly what the implementation does. Do not strengthen its parity claim or edit `techno_ai.rs` uniqueness assertions.

Tests:

- `lifecycle_authority_logic_flag_sets_after_append`
- `lifecycle_authority_logic_append_failure_leaves_flag_clear`
- `lifecycle_authority_logic_remove_compacts_then_clears_flag`
- `lifecycle_authority_flagged_missing_remove_still_clears_flag`
- `lifecycle_authority_legacy_app_death_handoff_changes_only_logic_membership`
- preserve existing same-pass append and self-removal scheduler regression tests as Rust mechanism tests only.

## Task 3: Implement Mark-Before-Register Reveal and Split Spawn Storage

**Files:** `src/sim/world/lifecycle.rs`, `src/sim/world/world_spawn.rs`, `src/sim/occupancy.rs`, `src/sim/world/lifecycle_tests.rs`.

1. Add the Reveal request/result types and required algorithm from Planned Interfaces.
2. Make Mark success atomically add all current footprint cells, assign enter order, and set `cell_marked` only after the cell additions complete.
3. Make Mark failure restore only `in_limbo`; keep the committed position and keep the object alive/stored.
4. Do not undo Reveal when logic append fails. Return `logic_registered=false` for test/diagnostic visibility.
5. Replace `place_spawned(ge, active: bool)` with an explicit store phase and explicit placement phase.
6. Make active placement result-bearing. A failure result must carry or leave accessible the stable ID so the caller can apply its own verified policy.
7. Preserve the current owner-count increment at construction, initialize `owned_count_released=false`, and do not mix release/decrement behavior into the Mark/logic transaction. Task 6 owns the only decrement helper.
8. Do not wire a synthetic `Can_Enter_Cell` implementation. Existing callers pass `MarkSucceeded` only after their present admission checks.

Use one concrete coordinate fixture in both success and failure tests: adjusted position `(rx=10 cells, ry=20 cells, z=2 height levels, sub_x=128 leptons, sub_y=64 leptons)`. Success must Mark the `(10,20)` cell/footprint and refresh cached screen coordinates before logic append. Mark failure must retain those exact authoritative cell/lepton/height values while restoring limbo and leaving cell/logic membership absent.

Tests:

- `lifecycle_authority_reveal_commits_coords_then_marks_then_registers`
- `lifecycle_authority_reveal_mark_failure_keeps_adjusted_coords_alive_limbo`
- `lifecycle_authority_reveal_early_reject_commits_nothing`
- `lifecycle_authority_reveal_logic_failure_keeps_successful_mark`
- `lifecycle_authority_second_reveal_is_idempotent`
- `lifecycle_authority_failed_redeploy_shape_retains_stored_amcv`

The AMCV test is a transaction fixture using stock identity/keys; it does not claim the full live GACNST caller has been rewired.

## Task 4: Migrate Represented Reveal/Conceal Callers and Keep Anim Separate

**Files:** `src/sim/passenger.rs`, `src/sim/aircraft/drop_payload.rs`, `src/sim/docking/bunker_link.rs`, `src/sim/production/production_sell.rs`, `src/sim/anim_class.rs`, plus test fixtures in `src/sim/docking/bunker_install.rs`, `src/sim/world/world_commands.rs`, `src/sim/snapshot.rs`, and `src/sim/world/world_tests.rs`.

For each production caller, preserve its existing admission and class-specific work, but remove manual writes now owned by lifecycle:

1. **Passenger boarding:** replace bulk contact cleanup + skeletal `conceal` with `techno_limbo`. Ensure both boarding paths invoke it. Passenger-role/nav/order writes remain caller-specific.
2. **Passenger unload/eject:** build a Reveal position from the selected exit, remove manual `occupancy.add`, and call result-bearing Reveal. Existing scatter/RNG order remains after successful placement.
3. **Garrison sell eject:** replace manual occupancy + Reveal with the same transaction; retain owner transfer and scatter order.
4. **Bunker install:** replace pre-remove occupancy + skeletal Conceal with Techno Limbo. Keep reciprocal bunker state and mission writes in their existing class-specific positions.
5. **Bunker release:** replace Reveal-before-occupancy pairs with one Mark-before-register Reveal request.
6. **Paradrop success:** remove manual occupancy and skeletal Reveal; call the transaction at the local success boundary. Preserve the current parachute-attach retry policy and classify its bulk failure cleanup separately rather than pretending it is Techno Limbo.
7. **Anim constructor/destruction:** call `reveal_anim` and `conceal_anim`; do not route Anim through cell Mark or radio BREAK. Preserve inactive and stop-sound ordering.
8. Convert direct registration/occupancy calls in tests to the transaction or explicit test-only helpers. Production raw helper call sites should become zero.

Required caller tests retain their existing assertions and add state-axis/order checks. At minimum run filters for passenger membership, bunker reveal, paradrop full-subcell rejection, garrison eject, and Anim pending deletion.

Audit:

```powershell
rg -n "register_live_object\(|unregister_live_object\(|\.reveal\(|\.conceal\(" src/sim
rg -n "add_entity_occupancy\(|remove_entity_occupancy\(" src/sim
```

Every production hit must be a lifecycle implementation site, movement/cell-motion operation with a documented reason, or an explicitly named blocked legacy bridge.

## Task 5: Implement Ordered Techno BREAK and Common Object Conceal

**Files:** `src/sim/world/lifecycle.rs`, `src/sim/world/mod.rs`, `src/sim/radio/mod.rs`, `src/sim/radio/receive.rs`, `src/sim/entity_store.rs`, `src/sim/world/lifecycle_tests.rs`.

1. Add `broadcast_break_before_conceal(sim, sender)` in radio code.
2. Snapshot only the sender's contact capacity. For each ascending index, re-read that slot immediately before calling `transmit(Break)`.
3. Do not hold a mutable/immutable sender borrow across `transmit`.
4. Preserve `transmit_break` sender-clear-before-receiver behavior and record a test event after sender clear, before dispatch.
5. Refactor receiver BREAK so refinery/bunker category-specific fields change first; then one common tail removes the sender from the receiver's contacts.
6. Run that common tail for Unit, Infantry, and Aircraft even though their class-specific BREAK behavior is not yet represented.
7. Leave Building GrandOpening and conditional `0x19` explicit TODO/BLOCKED comments tied to the radio research; do not fabricate them.
8. Add/initialize the serde-skipped `lifecycle_outputs` buffer. Reveal and Conceal emit the release-visible outputs at the exact Planned Interfaces slots; no output consumer is implemented in `sim/`.
9. Implement `object_conceal` and `techno_limbo` in the exact represented order: deselect, unmark, display/Anim/Voc outputs, logic unregister, optional `DirtyTacticalRect` only when `dirty_rect_eligible`, unconditional `ClearDrawnState`, limbo set, redraw-clear output. Keep native-alive true throughout; never merge the optional dirty and unconditional drawn-clear stages.
10. Remove lifecycle calls to `clear_radio_contacts_for`. Keep its current non-lifecycle drop-failure users clearly labeled as legacy/unverified instead of calling it a parity mechanism.

Tests:

- `lifecycle_authority_limbo_break_uses_sparse_slot_order`
- `lifecycle_authority_break_sender_is_clear_before_receiver_effect`
- `lifecycle_authority_break_receiver_effect_precedes_common_clear`
- `lifecycle_authority_break_clears_non_structure_receiver_contact`
- `lifecycle_authority_stale_break_contact_is_idempotent`
- `lifecycle_authority_conceal_deselects_unmarks_unregisters_then_sets_limbo`
- `lifecycle_authority_conceal_outputs_match_release_order`
- `lifecycle_authority_conceal_without_dirty_eligibility_still_clears_drawn_state`
- `lifecycle_authority_conceal_dirty_eligibility_emits_dirty_before_drawn_clear`
- `lifecycle_authority_conceal_keeps_object_alive`

## Task 6: Stage UnInit and Make Pending Delete Dead-Gated/Deduplicating

**Files:** `src/sim/world/lifecycle.rs`, `src/sim/world/mod.rs`, `src/sim/world/substrate.rs`, `src/sim/anim_class.rs`, `src/sim/passenger.rs`, `src/sim/combat/mod.rs`, `src/sim/combat/combat_tests.rs`, `src/sim/production/production_sell.rs`, `src/app_sim_tick.rs`, `src/sim/world/lifecycle_tests.rs`, `src/sim/world/world_tests.rs`.

1. Add `release_owned_count_once` backed by serialized/hashed `owned_count_released`, and make it the sole owner-count decrement authority. Call it from `run_represented_uninit_pre_hook`, then put the existing building-fire and bunker-link cleanup behind that named hook. Preserve current output while labeling the exact native sub-order BOUNDED/UNCHECKED; do not use `dying` as the count guard.
2. Add `PassengerCargo::take_for_uninit`: `mem::take` boarding-order IDs, reset `total_size` and `garrison_fire_index`, retain capacity/size limits. In the carrier UnInit pre-base stage, clear each represented `Inside { transport_id }` relation, set the passenger's existing Rust death health to zero, and recursively call its own UnInit in captured order.
3. Ensure destroyed garrisons detach/eject their cargo before building UnInit. Successful exits use Reveal. A no-exit occupant keeps the existing health-zero outcome but does not pre-set `dying` or common lifecycle fields; call its UnInit immediately. The carrier cargo must be empty before the building reaches generic cargo recursion.
4. Add a distinct removal-notification boundary after carried-passenger recursion and before virtual Limbo. It is test-visible but has no speculative global listener scan.
5. Call Techno Limbo while `object_alive=true`.
6. After Conceal completes, set `object_alive=false`; set/retain `dying=true` only as the transitional Rust death gate.
7. Append the stable ID last and do not suppress duplicates.
8. Implement dead/readiness checking and duplicate collapse in `process_pending_delete`.
9. Keep an alive queued ID in its original relative queue position. Serialize/hash that state.
10. Finalize/remove a ready entity or inactive Anim exactly once per stable ID. Missing IDs are scrubbed defensively.
11. Keep a named common-finalization boundary and explicit blocked comments for alive restore and concrete destructors.
12. Make this module the only production caller of both store `remove` methods.
13. In combat, delete `clear_targets_on_dead_entity` and stop pre-clearing selected/attack/movement state for immediate deaths. Stop killing/mutating transport cargo in combat; carrier UnInit owns it.
14. For no-animation deaths, emit `immediate_uninit_ids` without setting `dying`, releasing owner count, or pre-unregistering. In the world consumer, exclude those IDs from the deferred-animation handoff and route them directly through UnInit (after any verified garrison/bunker pre-work); UnInit invokes the exactly-once count helper.
15. For the still-blocked animated-death path, retain health-zero, `dying=true`, and death-sequence selection. At the existing world handoff timing, call `release_owned_count_once` and then the single named legacy logic-only unregister. Do not pre-deselect or clear attack/movement/other entities' targets. Eventual app-requested UnInit calls the same count helper and must observe `owned_count_released=true`, so it cannot decrement again. This remains DRIFT until Mission/Foot owns completion.
16. In `app_sim_tick.rs`, remove the caller-side occupancy removal and `flush_pending_delete`. After current animation completion, call central UnInit only; the serialized queue survives until the next ordinary sim drain. Drain `lifecycle_outputs` in emitted order through an exhaustive no-op match so the release seam cannot accumulate, and label both the no-op consumers and app-owned UnInit request transitional.

Tests:

- `lifecycle_authority_uninit_limbos_while_alive_then_clears_alive_then_queues`
- `lifecycle_authority_uninit_removal_boundary_sees_alive_marked_target`
- `lifecycle_authority_transport_uninits_passengers_in_cargo_order_before_carrier_notify`
- `lifecycle_authority_destroyed_garrison_detaches_or_uninits_occupants_before_building_uninit`
- `lifecycle_authority_immediate_combat_death_reaches_uninit_without_precleanup`
- `lifecycle_authority_animated_death_legacy_handoff_changes_only_dying_sequence_count_and_logic`
- `lifecycle_authority_immediate_uninit_releases_owned_count_once`
- `lifecycle_authority_deferred_animation_then_uninit_releases_owned_count_once`
- `lifecycle_authority_duplicate_uninit_does_not_double_release_owned_count`
- `lifecycle_authority_app_completion_does_not_preunmark_or_run_second_drain`
- `lifecycle_authority_uninit_dead_limbo_remains_resolvable_until_drain`
- `lifecycle_authority_duplicate_queue_finalizes_once`
- `lifecycle_authority_alive_queued_object_remains_queued`
- `lifecycle_authority_entity_and_anim_share_ordered_drain`
- preserve mutual-death queue-order and immediate-structure-death tests.

Do not implement CaptureManager/chrono/deploy wrapper objects, a speculative listener scan, or production Mission/Foot sequence ownership in this task.

## Task 7: Replace Movement Crush Raw Removal with a Lifecycle Request

**Files:** add `src/sim/lifecycle_request.rs`; modify `src/sim/mod.rs`, `src/sim/world/mod.rs`, `src/sim/movement/mod.rs`, `src/sim/movement/movement_tick.rs`, `src/sim/movement/movement_tests.rs`, `src/sim/movement/prone_speed_tests.rs`, `src/sim/miner/miner_tests.rs`, `src/sim/world/lifecycle_tests.rs`.

1. Add the data-only request types.
2. Add the reusable serde-skipped request vector to `Simulation` and initialize it in `construct`.
3. Extend `tick_movement_with_grids`, `tick_movement_with_grid`, and `tick_movement` with `&mut Vec<LifecycleRequest>`. Update every direct caller in movement, prone-speed, and miner tests. Do not add a heap-owning vector to the currently `Copy` `MovementTickStats`, and do not create a temporary sink that is discarded by a wrapper.
4. Preserve current crush victim sort/dedup and sound emission.
5. Keep the current occupancy removal needed by the movement phase, and clear `cell_marked` at that same point so state stays coherent. Label this exact timing UNCHECKED pending the unified per-object scheduler.
6. Set victim health to zero, append one `Uninit { reason: Crush }`, and remove the bulk contact scrub and raw `entities.remove` call.
7. Because the victim now remains stored until world consumption, exclude every crushed stable ID from all remaining work in the same movement call: `finalize_finished_entities`, `update_locomotor_phases`, and the hover vertical pass. Reuse the already sorted/deduplicated `crush_kills` IDs for membership tests; do not allocate or mutate the victim after its request is emitted.
8. Immediately after movement returns, `mem::take` the reusable request vector, drain/apply requests in order through `Simulation::uninit`, then restore the now-empty vector so its capacity is retained. Do not borrow `self.pending_lifecycle_requests` while calling another `&mut self` method.
9. Do not apply requests after gate, vision, combat, or the late tail; that would create additional same-tick drift.

Tests:

- `lifecycle_authority_crush_emits_one_request_without_store_removal`
- `lifecycle_authority_crushed_victim_skips_all_remaining_movement_postpasses`
- `lifecycle_authority_world_consumes_crush_request_through_uninit`
- `lifecycle_authority_duplicate_crush_request_tears_down_once_at_drain`
- update `test_crush_removal_clears_live_radio_contacts` so it proves ordered BREAK/UnInit rather than raw BTreeMap cleanup.

Audit after this task:

```powershell
rg -n "(substrate\.)?entities\.remove\(" src/sim
rg -n "anims\.remove\(" src/sim
```

The allowlist is: the `EntityStore::remove` method implementation itself, the common lifecycle finalizer that calls it, the equivalent `AnimStore` implementation/finalizer, and named tests explicitly constructing corrupted state. There must be zero other production callers; do not report that the raw `rg` output literally contains only the drain.

## Task 8: Move the Ordinary Drain After Binary-Frame Commit

**Files:** `src/sim/world/mod.rs`, `src/sim/world/lifecycle.rs`, `src/sim/world/lifecycle_tests.rs`, `src/sim/world/world_tests.rs`.

At the end of `run_late_region`, preserve all prior phase work and change only the verified tail relation:

```rust
self.session.total_sim_ms = self
    .session
    .total_sim_ms
    .saturating_add(tick_ms as u64);
self.session.binary_frame = ((self.session.total_sim_ms * 15) / 1000) as u32;
#[cfg(test)]
self.trace_lifecycle_for_test(LifecycleTestEvent::BinaryFrameCommitted);

self.process_pending_delete();

// Keep OCCUPANCY_DEBUG validation here, after the drain.

// Keep the separate Rust tick assignment in its current post-drain and
// post-debug-validation relation.
self.session.tick = execute_tick;
```

1. Keep occupancy debug validation after the drain so dead structures are not rebuilt into the comparison.
2. Do not add guessed handling for the four native late-skip flags.
3. Do not change earlier timer consumers; they must still observe the pre-increment binary frame during the tick.
4. Add an order-ledger test because final state alone cannot distinguish frame-before-drain.
5. Correct stale tests/comments that claim a command-boundary drain or pre-frame native drain.

Tests:

- `lifecycle_authority_late_tail_commits_frame_before_drain`
- existing `binary_frame_committed_late_gate_captures_pre_increment_frame`
- existing command-death, vision/power gate, and state-hash tail tests with corrected descriptions.

## Task 9: Close the Bounded Acceptance Matrix and Run Serial Validation

**Files:** all edited files; no new behavior beyond prior tasks.

1. Run focused tests serially, reading the literal `test result:` line:

   ```powershell
   cargo test -p vera20k lifecycle_authority_ -- --nocapture
   cargo test -p vera20k logic_vector -- --nocapture
   cargo test -p vera20k radio -- --nocapture
   cargo test -p vera20k passenger -- --nocapture
   cargo test -p vera20k bunker -- --nocapture
   cargo test -p vera20k paradrop -- --nocapture
   cargo test -p vera20k combat -- --nocapture
   cargo test -p vera20k crush -- --nocapture
   cargo test -p vera20k snapshot -- --nocapture
   ```

2. Run the direct-authority audits:

   ```powershell
   rg -n "Presence::|derived_presence\(" src/sim
   rg -n "register_live_object\(|unregister_live_object\(" src/sim
   rg -n "clear_radio_contacts_for\(" src/sim
   rg -n "(substrate\.)?entities\.remove\(|anims\.remove\(" src/sim
   rg -n "flush_pending_delete\(|process_pending_delete\(" src/sim src/app_sim_tick.rs
   rg -n "clear_targets_on_dead_entity\(" src/sim
   rg -n "lifecycle_outputs" src/sim src/app_sim_tick.rs
   rg -n "owned_count_released|release_owned_count_once|owned_count" src/sim
   ```

3. Classify every remaining hit in the plan handoff. `flush_pending_delete` and `clear_targets_on_dead_entity` must have zero production hits, and every represented owner-count decrement must be inside `release_owned_count_once`. Do not hide the remaining app-owned UnInit request, drop-failure scrub, test-fixture, or movement-unmark exceptions.
4. Format only edited Rust files with edition 2024. Do not run crate-wide `cargo fmt` and do not include `techno_ai.rs` in the rustfmt command.
5. Inspect `git diff --stat`, then inspect every edited-file diff for unrelated churn.
6. Run one final serial check:

   ```powershell
   cargo check -q -p vera20k
   ```

7. Recompute `git hash-object src/sim/world/techno_ai.rs` and require it to equal the implementation-start hash. Verify every other execution-start dirty path is still outside the implementation diff.
8. Report the bounded verdict as:
   - independent state / Reveal / represented Conceal-BREAK / UnInit spine / crush request / ordinary drain: implemented with named tests;
   - complete lifecycle parity: **not certified**;
   - every deferred item below: still BLOCKED/UNCHECKED.

No commit, branch, push, or golden rebaseline is part of this plan unless the user separately requests it.

## Bounded Acceptance Matrix

| Contract acceptance | This plan's result |
|---|---|
| State product | Covered fully for represented Rust state. |
| Reveal success/failure | Covered at the lifecycle transaction; live placement oracle and full failed-redeploy caller remain blocked. |
| Conceal order | Covered for sim-owned selection/cell/logic/limbo plus separate optional dirty and unconditional drawn-clear outputs; exact dirty eligibility and upper-layer consumers/effects remain blocked. |
| Techno BREAK | Covered for slot order, sender clear, represented class effect, common receiver clear; GrandOpening/`0x19` remain blocked. |
| UnInit order | Covered for exactly-once represented owner-count release, represented pre-hook, carried-passenger recursion, removal boundary, virtual Limbo, alive clear, and queue append; exact owner-count sub-order and the complete listener/manager roster remain blocked. |
| Ordinary Infantry sequence | Not covered; protected blocker. |
| All removal entries | Immediate combat, carried cargo, destroyed-garrison no-exit, crush, ordinary UnInit, and Anim expiry route through authority. App completion requests authority without a second drain; remaining non-lifecycle failure scrubs and the Infantry production host stay explicit. |
| Dead-limbo window | Covered. |
| Drain order | Covered once per ordinary sim tick after frame commit; the app flush is removed; exceptional skip flags/destructors remain blocked. |
| Snapshot integrity | Covered for Rust v28 lifecycle, dirty-eligibility, owner-count-release, and pending-queue authority; native save/load parity not claimed. |
| Determinism | Covered by ordered Rust event/hash regression tests; not retail parity certification. |

## Self-Review Checklist

- [x] Current simulation source was re-read while the shared worktree advanced through unrelated `app_init.rs` changes; implementation must still revalidate its execution-start HEAD.
- [x] The July contract supersedes the stale May behavioral decisions.
- [x] Every unblocked next-workflow item maps to a task: state axes, private boundary, Mark-before-register, BREAK routing, immediate/cargo UnInit routing, raw-removal request, and ordinary drain placement.
- [x] The plan preserves `techno_ai.rs` and does not treat its test-only work as production authority.
- [x] Every binary-derived ordering claim is tied to a research doc/anchor.
- [x] Blocked listener/manager bodies, upper-layer consumers, Infantry Mission ownership, destructors, skip flags, scheduler, and native save/load are not smuggled into code snippets.
- [x] Snapshot/hash changes and golden ownership are called out explicitly.
- [x] Conditional dirtying is distinct from unconditional drawn-state clearing, and owner-count release has an explicit serialized/hashed exactly-once authority.
- [x] Physical removal and direct lifecycle primitive audits have concrete commands.
- [x] Validation is serial and follows repository Cargo coordination rules.

## Sources

### Approved authority and architecture

- `docs/contracts/2026-07-21-ordered-reveal-conceal-uninit-lifecycle-authority-implementation-contract.md`
- `docs/plans/2026-05-28-logicclass-object-lifecycle-spine-design.md` — historical architecture context only where not superseded

### Verified research

- [docs/research/LIMBO_AND_CELL_OCCUPATION_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LIMBO_AND_CELL_OCCUPATION_LIFECYCLE_GHIDRA_REPORT.md)
- [docs/research/LOGIC_OBJECT_REGISTRATION_HELPER_FUN_0055BAA0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGIC_OBJECT_REGISTRATION_HELPER_FUN_0055BAA0_GHIDRA_REPORT.md)
- [docs/research/CELLCLASS_SUBSTRATE_LIVE_OBJECT_LIST_WRITERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_SUBSTRATE_LIVE_OBJECT_LIST_WRITERS_GHIDRA_REPORT.md)
- [docs/research/BROADCAST_RADIO_TO_ALL_LIMBO_BREAK_CLEANUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BROADCAST_RADIO_TO_ALL_LIMBO_BREAK_CLEANUP_GHIDRA_REPORT.md)
- [docs/research/OBJECTCLASS_UNINIT_DEATH_CLEANUP_ORDERING_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_UNINIT_DEATH_CLEANUP_ORDERING_RESWARM_20260528.md)
- [docs/research/PENDING_DELETE_DRAIN_DESTRUCTOR_TIMING_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PENDING_DELETE_DRAIN_DESTRUCTOR_TIMING_RESWARM_20260528.md)
- [docs/research/FAILED_REDEPLOY_LIMBO_UNIT_CLEANUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FAILED_REDEPLOY_LIMBO_UNIT_CLEANUP_GHIDRA_REPORT.md)

### Binary anchors spot-checked for this plan

- `ObjectClass::Reveal @ 0x005F4EC0`
- `ObjectClass::Conceal @ 0x005F4D30`
- Logic registration helper `0x0055BAA0`
- Logic removal helper `0x0055BAE0`
- `ObjectClass::UnInit @ 0x005F65F0`
- Broadcast BREAK `0x0065ACE0`
- Pending-delete drain `0x00725C70`

### Stock INI evidence used only by the failed-redeploy fixture

- `[MultiplayerDialogSettings] MCVRedeploys=yes`
- `[GACNST] ConstructionYard=yes`
- `[GACNST] UndeploysInto=AMCV`
- `[AMCV]` stock object definition

There is no direct INI key that defines the common Reveal, Conceal, UnInit, LogicVector, radio-BREAK, or pending-delete ordering in this plan.
