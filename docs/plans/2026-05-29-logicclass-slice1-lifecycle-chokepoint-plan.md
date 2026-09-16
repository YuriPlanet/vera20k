# LogicClass Substrate — Slice 1: Lifecycle Chokepoint Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained.

**Goal:** Introduce `reveal / conceal / unlimbo / uninit` as the single lifecycle vocabulary for
LogicClass-vector membership, route every production spawn/limbo/death site through them, and add a
debug invariant — with **zero behavior change and zero state-hash change**.

**Architecture:** `sim/` only. The `LogicVector` order primitive and the `register_live_object` /
`unregister_live_object` / `despawn_entity` operations already exist and are faithful to gamemd. This
slice adds named lifecycle methods that *delegate* to those primitives so membership coverage is
structural (a future spawn path can't silently forget to register) instead of hand-wired per site.
It is the safe foundation for Slice 2 (routing object phases through logic order).

**Design doc:** [docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) §6 (boundary), §8 (Slice 1);
`docs/plans/2026-05-28-logicclass-object-lifecycle-spine-design.md` (Chosen Approach → Lifecycle helpers).

---

## Grounding Summary

- **Docs:** The study doc §6 defines the three-owner boundary (Registry / Lifecycle chokepoint /
  Scheduler) and §8 Slice 1 scopes exactly this work. The 2026-05-28 spine design already *named* the
  helpers: `reveal→register`, `conceal→unregister` (object stays in store = limbo), `unlimbo→reveal`,
  `uninit→conceal then store-remove` with `despawn_entity` routed through `uninit`. This plan
  implements that naming as delegators.
- **Ghidra (verified this session):** native lifecycle is `ObjectClass::Reveal @ 0x005F4EC0` →
  register `0x0055BAA0` (idempotent `+0x98` guard, tail-append); `ObjectClass::Conceal @ 0x005F4D30`
  → remover `0x0055BAE0` (order-preserving compacting left-shift, clears `+0x98`); `ObjectClass::UnInit
  @ 0x005F65F0` → Conceal then deferred free. The Rust primitives already match these shapes
  (`LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md §4.1`). Slice 1 changes **naming/routing only**, not
  these mechanics — so no new binary verification is required for this slice.
- **Repo pattern:** mirror the existing thin-delegator + doc-comment style of `register_live_object`
  (`src/sim/world/mod.rs:667-674`) and `despawn_entity` (`:765-788`). Membership flag is
  `GameEntity::in_logic_vector` (`src/sim/game_entity.rs:172`, `#[serde(skip)]`).
- **INI:** none — this slice touches no game constants.
- **Git state:** target files current as of `58f78a6` (sim/world plumbing) and `795fdd4` (LogicVector
  + two-stream RNG + hash). The design premise (LogicVector + register/unregister/despawn exist) holds
  against the working tree read this session.
- **Still unknown after grounding:** death-to-limbo timing (when a `dying` entity should leave the
  vector vs Rust's `dying`→later-`despawn`). This does **not** affect Slice 1 (despawn timing is
  unchanged here) and is deferred to Slice 2. See Open Questions.

## Key Technical Decisions

- **Lifecycle helpers are thin delegators over the existing primitives, not rewrites.**
  `reveal`=`register_live_object`, `conceal`=`unregister_live_object`, `unlimbo`=`reveal`,
  `uninit`=the current `despawn_entity` body. — **Confidence:** high — **Source:** repo
  `src/sim/world/mod.rs:667-788`; design `2026-05-28-...-spine-design.md` (Lifecycle helpers).
- **`uninit` becomes the canonical impl; `despawn_entity` delegates to it.** The body moves to
  `uninit(id)`; `despawn_entity(id)` becomes `{ self.uninit(id) }`. Keeps all existing
  `despawn_entity` callers (incl. tests + app layer) working unchanged while making `uninit` the real
  chokepoint. — **Confidence:** high — **Source:** design intent "despawn routes through uninit."
- **Primitives stay `pub(crate)`.** `register_live_object` / `unregister_live_object` remain (used by
  the new helpers and by the existing scheduler tests in `snapshot.rs`). Only *production* call sites
  migrate to the named helpers; test call sites may stay on the primitives. — **Confidence:** high.
- **Debug invariant runs once per tick under `cfg(debug_assertions)`.** `order.len()` must equal the
  count of in-store entities with `in_logic_vector == true`, and the order must be duplicate-free.
  O(n) debug-only; release builds unaffected. — **Confidence:** high — **Source:** study §6, design
  "Error Handling" (`debug_assert!`).
- **Parity-neutral by construction.** Because the helpers are exact delegators and no register/
  unregister/despawn call is added or removed (only renamed at the call site), the deterministic state
  hash is unchanged. — **Confidence:** high — verified by the Task 9 replay-hash check.

## Open Questions

### Resolved During Planning

- *Do `reveal/conceal/unlimbo/uninit` names collide with existing symbols?* No. Only `reveal_radius` /
  `reveal_radius_into` exist (free fns in `src/sim/vision/mod.rs:556,737`), a different namespace from
  `Simulation` methods. (verified via `Grep "fn reveal|fn conceal|fn unlimbo|fn uninit"`.)
- *Are `passenger.rs:1163` / `:1188` register calls production or test?* To be confirmed in Task 3 by
  reading their enclosing scope; the documented production unload sites are `passenger.rs:881` and
  `:1034`. If 1163/1188 are under `#[cfg(test)]`, leave them on the primitive.
- *Does Slice 1 need the death-to-limbo answer?* No — `despawn_entity`/`uninit` timing is unchanged.

### Deferred to Implementation

- None for Slice 1 (no execution-time unknowns; the slice is structural).

### Deferred to later slices (not this plan)

- **Death-to-limbo timing** (Slice 2 prerequisite): when a `dying` entity should leave the vector.
- Routing object phases through logic order (Slice 2); ore-before-objects reorder (Slice 3); missing
  global rungs (Slice 4); true interleaved walk (Slice 5).

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `src/sim/world/mod.rs` | Add `reveal/conceal/unlimbo/uninit` helpers; move despawn body into `uninit`; add debug invariant + per-tick call |
| Modify | `src/sim/world/world_spawn.rs` | `register_live_object`→`reveal` (`:260`, `:438`); `despawn_entity`→`uninit` (`:738`) |
| Modify | `src/sim/passenger.rs` | unload `register_live_object`→`reveal` (`:881`, `:1034`); board `unregister_live_object`→`conceal` (`:487`) |
| Modify | `src/sim/production/production_sell.rs` | garrison-eject `register_live_object`→`reveal` (`:459`) |
| Modify | `src/sim/aircraft/drop_payload.rs` | paradrop `register_live_object`→`reveal` (`:240`) |
| Modify | `src/sim/slave_miner.rs` | slave-death `despawn_entity`→`uninit` (`:473`, `:555`) |
| Modify | `src/sim/world/world_orders.rs` | engineer-consume `despawn_entity`→`uninit` (`:244`, `:408`) |
| Modify | `src/app_sim_tick.rs` | combat-death `despawn_entity`→`uninit` (`:306`) — app layer, allowed (calls into sim) |
| Modify | `src/sim/snapshot.rs` | Add Slice-1 lifecycle acceptance tests |

## Interface Changes

New `pub(crate)` methods on `Simulation` (additive; nothing depends on their absence):
`reveal(&mut self, id: u64)`, `conceal(&mut self, id: u64)`, `unlimbo(&mut self, id: u64)`,
`uninit(&mut self, id: u64)`. `despawn_entity` keeps its signature (now delegates to `uninit`), so its
existing callers are unaffected even where not migrated. `register_live_object` /
`unregister_live_object` remain `pub(crate)` primitives.

## Sim Checklist

- [x] All math uses `fixed`-point — N/A (no arithmetic added).
- [x] New state included in deterministic state hash — N/A (no new persisted state; the debug
  invariant reads existing state and is `cfg(debug_assertions)`-only, never hashed).
- [x] No dependencies on render/ui/sidebar/audio/net — confirmed (helpers wrap existing sim ops;
  `app_sim_tick.rs` is app layer calling *into* sim, not sim depending on app).
- [x] Tick ordering impact — none; helpers are delegators, despawn timing unchanged.
- [x] BTreeMap iteration order — unaffected (the debug invariant counts a flag; order authority is
  unchanged in this slice).

## Risk Areas

- **Lowest-risk slice by design** (pure rename/delegation). The only ways to introduce a behavior
  change are: (a) accidentally changing the *order* of a register/unregister/despawn relative to other
  ops while moving the body, or (b) migrating a call site whose semantics differ from the helper.
  Mitigations: Task 2 moves the `despawn_entity` body verbatim into `uninit`; Tasks 3-4 are 1:1 name
  swaps with no argument or position change; Task 9 asserts the replay state hash is unchanged.
- **Debug invariant false-positive risk:** if any code sets `in_logic_vector` without touching `logic`
  (or vice-versa), the assert fires. That is the intended catch, but to avoid breaking existing debug
  test runs, Task 6 first *confirms* the invariant already holds across the suite before wiring the
  per-tick call. If it fires, that is a pre-existing latent bug — surface it, do not paper over it.
- **Coverage audit (Task 5)** may find an active-spawn path that does not register. If so, that is a
  **behavior change** to add a missing `reveal` — surface it as a finding and get sign-off; do **not**
  fold it silently into this parity-neutral slice.

## Parity-Critical Items

Slice 1 is **parity-neutral by construction**. The single parity-critical guarantee is that the
migration preserves the exact set, order, and timing of register/unregister/despawn calls.

| Task # | Item | Why it matters | Verification |
|--------|------|----------------|--------------|
| Task 2 | `uninit` body == old `despawn_entity` body, verbatim | Any reorder of unregister-vs-store-remove or occupancy/radio cleanup changes despawn-tick observable state | Diff the moved body line-for-line; Task 9 replay-hash unchanged |
| Tasks 3-4 | Call-site swaps are 1:1 (same id, same position) | A dropped/added register call changes the live vector → desync | `reveal`≡`register_live_object`, `conceal`≡`unregister_live_object`, `uninit`≡`despawn_entity`; Task 9 replay-hash unchanged |
| Task 5 | Spawn-coverage completeness | A missing `reveal` = object never gets AI (player-visible) | Read each active-spawn fn; confirm coverage table |

---

## Tasks

### Task 1: Add `reveal` / `conceal` / `unlimbo` delegating helpers

**Why:** Establish the lifecycle vocabulary first; later tasks migrate call sites onto it.

**Files:**
- Modify: `src/sim/world/mod.rs` (immediately after `unregister_live_object`, currently ending `:686`)

**Pattern:** thin delegator + doc comment, mirroring `register_live_object` (`:667-674`).

**Step 1: Add the three helpers**
```rust
// src/sim/world/mod.rs — after unregister_live_object (~:686)

    /// Native `ObjectClass::Reveal` append: an object becomes a live AI member.
    /// Active spawns / unlimbo / unload / paradrop call this. Delegates to the
    /// `+0x98`-guarded tail-append primitive; idempotent.
    pub(crate) fn reveal(&mut self, stable_id: u64) {
        self.register_live_object(stable_id);
    }

    /// Native `ObjectClass::Conceal`: the object leaves the live AI set but stays
    /// in the store (limbo). Delegates to the compacting-remove primitive.
    pub(crate) fn conceal(&mut self, stable_id: u64) {
        self.unregister_live_object(stable_id);
    }

    /// Native `TechnoClass::Unlimbo` → Reveal. A limbo-created object joins the
    /// live set at unlimbo/landing time, not at construction.
    pub(crate) fn unlimbo(&mut self, stable_id: u64) {
        self.reveal(stable_id);
    }
```

**Step 2: Verify**
Run: `cargo check -p <sim-crate>` (the crate owning `src/sim/`; use the workspace member name from
`Cargo.toml`).
Expected: compiles; three new methods present, no warnings about unused (they are used from Task 3-4,
so an `unused` warning here is acceptable until those land — do not `#[allow]` it permanently).

**Step 3: Commit** — `git add -A && git commit -m "sim/world: add reveal/conceal/unlimbo lifecycle helpers"`

---

### Task 2: Make `uninit` the canonical despawn impl; `despawn_entity` delegates

**Why:** `uninit` is the native conceal-then-free chokepoint. Moving the body here (and delegating)
keeps all existing `despawn_entity` callers working while making `uninit` the real entry point.

**Files:**
- Modify: `src/sim/world/mod.rs:765-788` (the current `despawn_entity`)

**Pattern:** verbatim body move + thin delegator.

**Step 1: Rename the method, keep the body identical**
Change the signature line `pub(crate) fn despawn_entity(&mut self, stable_id: u64) {` to
`pub(crate) fn uninit(&mut self, stable_id: u64) {`. **Move nothing else** — the body
(entity-info gather → owned-count decrement → occupancy remove → `clear_radio_contacts_for` →
`unregister_live_object` → `entities.remove`) stays byte-for-byte. Update the body's existing comment
`// conceal: leave the active order first` to call the helper:
replace `self.unregister_live_object(stable_id);` with `self.conceal(stable_id);` (equivalent;
vocabulary-consistent).

**Step 2: Add the delegator**
```rust
// src/sim/world/mod.rs — directly after uninit

    /// Remove an entity from the world. Retained name for existing callers and
    /// tests; routes through `uninit` so the conceal-before-free ordering is
    /// centralized.
    pub(crate) fn despawn_entity(&mut self, stable_id: u64) {
        self.uninit(stable_id);
    }
```

**Step 3: Update the doc comment on `for_each_live_object`** (`:708-710`) that references
`despawn_entity always unregisters before freeing` — change to `uninit always conceals before freeing`
(behavioral statement unchanged).

**Step 4: Verify**
Run: `cargo check` then `cargo test -p <sim-crate> despawn_entity_clears_live_radio_contacts -- --nocapture`
(the existing test at `world_tests.rs:86`).
Expected: PASS — body is unchanged, so this regression test still holds.

**Step 5: Commit** — `git commit -am "sim/world: uninit as canonical despawn; despawn_entity delegates"`

---

### Task 3: Migrate production register sites → `reveal`

**Why:** Make active-object insertion go through the lifecycle vocabulary.

**Files & exact swaps** (replace `register_live_object(` with `reveal(` — same receiver, same arg):
- `src/sim/world/world_spawn.rs:260` — `self.register_live_object(spawn_sid)` → `self.reveal(spawn_sid)`
- `src/sim/world/world_spawn.rs:438` — `self.register_live_object(stable_id)` → `self.reveal(stable_id)`
- `src/sim/passenger.rs:881` — `sim.register_live_object(pax_id)` → `sim.reveal(pax_id)`
- `src/sim/passenger.rs:1034` — `sim.register_live_object(pax_id)` → `sim.reveal(pax_id)`
- `src/sim/production/production_sell.rs:459` — `sim.register_live_object(passenger_id)` → `sim.reveal(passenger_id)`
- `src/sim/aircraft/drop_payload.rs:240` — `sim.register_live_object(passenger_id)` → `sim.reveal(passenger_id)`

**Step 1: Confirm `passenger.rs:1163` / `:1188` scope.** Read `src/sim/passenger.rs:1150-1195`. If
they are inside a `#[cfg(test)]` module, **leave them** on `register_live_object`. If production,
migrate them to `reveal` too and add them to this list.

**Step 2: Apply the swaps** above (1:1, no other change).

**Step 3: Verify**
Run: `cargo check`.
Expected: compiles; the Task-1 `reveal` unused warning is now gone.

**Step 4: Commit** — `git commit -am "sim: route active spawns through reveal()"`

---

### Task 4: Migrate production conceal/uninit sites

**Why:** Route limbo (board transport) and destruction through the lifecycle vocabulary.

**Files & exact swaps:**
- `src/sim/passenger.rs:487` — `sim.unregister_live_object(pax_id)` → `sim.conceal(pax_id)` (board transport)
- `src/app_sim_tick.rs:306` — `sim.despawn_entity(*dead_id)` → `sim.uninit(*dead_id)` (combat death; app→sim call)
- `src/sim/slave_miner.rs:473` — `sim.despawn_entity(stable_id)` → `sim.uninit(stable_id)`
- `src/sim/slave_miner.rs:555` — `sim.despawn_entity(stable_id)` → `sim.uninit(stable_id)`
- `src/sim/world/world_spawn.rs:738` — `self.despawn_entity(stable_id)` → `self.uninit(stable_id)` (MCV deploy: old MCV)
- `src/sim/world/world_orders.rs:244` — `self.despawn_entity(engineer_id)` → `self.uninit(engineer_id)`
- `src/sim/world/world_orders.rs:408` — `self.despawn_entity(engineer_id)` → `self.uninit(engineer_id)`
- `src/sim/world/mod.rs:1276` — `self.despawn_entity(sid)` → `self.uninit(sid)`

(Leave `despawn_entity` calls in `*_tests.rs` and `world_hash.rs` tests as-is — they exercise the
retained delegator on purpose.)

**Step 1: Apply the swaps** (1:1).

**Step 2: Verify**
Run: `cargo check`.
Expected: compiles.

**Step 3: Commit** — `git commit -am "sim: route conceal/destruction through conceal()/uninit()"`

---

### Task 5: Audit spawn-coverage completeness (read-only; surface gaps, do not fix)

**Why:** The chokepoint is only as complete as its coverage. Confirm every active-spawn path reveals
and every limbo path does not — without changing behavior in this slice.

**Files (read):** `src/sim/world/world_spawn.rs` (`spawn_from_map_with_resolved`,
`spawn_object_at_height`, `spawn_object_limbo_at_height`), `src/sim/production/` (unit completion →
which spawn fn it calls), `src/sim/passenger.rs` (unload paths), `src/sim/aircraft/drop_payload.rs`
(paradrop), `src/sim/production/production_sell.rs` (garrison eject), `src/sim/slave_miner.rs`,
`src/sim/superweapon/` (genetic converter / iron curtain create entities?).

**Step 1:** For each `entities.insert(` in **non-test** sim code (from the grep set), determine whether
the inserted object is *active* (must `reveal`) or *limbo* (must not). Build a coverage table:
`spawn fn | active? | reveals? | verdict`.

**Step 2:** Confirm `spawn_object_limbo_at_height` does **not** reveal (correct: limbo). Confirm
production-completed units route through `spawn_object_at_height` (reveals).

**Step 3:** If a gap is found (active spawn with no reveal, or limbo spawn that reveals), **STOP and
record it as a finding** in this plan's results — adding/removing a reveal is a behavior change that
needs the user's sign-off and its own task (not part of the parity-neutral slice).

**Step 4:** Write the coverage table into
[docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) under a new "Slice 1 coverage audit"
appendix (docs are local-only; no commit needed for `docs/`).

**Step 5: Verify** — no code changed; `cargo check` still green.

---

### Task 6: Add the debug membership invariant

**Why:** Make the split-state invariant (`order` Vec vs per-entity flag) self-checking so future
desyncs between them are caught in debug builds.

**Files:**
- Modify: `src/sim/world/mod.rs` (add method near the lifecycle helpers)
- Modify: `src/sim/world/mod.rs` (call it once at the end of `advance_tick`, before/around the
  existing `state_hash()` at `:2031`)

**Step 1: Add the check method**
```rust
// src/sim/world/mod.rs — near the lifecycle helpers

    /// Debug-only invariant: the logic order and the per-entity membership flag
    /// are two views of one set and must never disagree. The order must be
    /// duplicate-free, and `order.len()` must equal the number of in-store
    /// entities whose `in_logic_vector` is set. O(n); compiled out of release.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_logic_membership_consistent(&self) {
        let order = self.logic.as_slice();
        let mut seen = std::collections::BTreeSet::new();
        for &id in order {
            debug_assert!(seen.insert(id), "logic order has duplicate id {id}");
        }
        let flagged = self
            .entities
            .values()
            .filter(|e| e.in_logic_vector)
            .count();
        debug_assert_eq!(
            order.len(),
            flagged,
            "logic order length ({}) != entities flagged in_logic_vector ({})",
            order.len(),
            flagged
        );
    }
```

**Step 2: Call it once per tick (debug only)**
In `advance_tick`, immediately before `let state_hash = self.state_hash();` (`:2031`):
```rust
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
```

**Step 3: Verify**
Run: `cargo test -p <sim-crate>` (debug profile).
Expected: full suite PASSES with the invariant active. **If it fires**, the order/flag are already
desynced somewhere — capture the failing test and the id, and treat it as a pre-existing bug to
diagnose (do not delete the assert to make it pass).

**Step 4: Commit** — `git commit -am "sim/world: debug invariant for logic-order/membership consistency"`

---

### Task 7: Acceptance tests for the lifecycle chokepoint

**Why:** Lock the contract: reveal/conceal roundtrip membership, unlimbo == reveal, uninit conceals
before freeing, and the invariant holds.

**Files:**
- Modify: `src/sim/snapshot.rs` (extend the existing scheduler-test module that already builds small
  sims and calls `register_live_object`, ~`:240-427`)

**Step 1: Add the tests** (use the module's existing sim-builder helpers; mirror their style)
```rust
    #[test]
    fn reveal_then_conceal_roundtrips_membership() {
        let mut sim = Simulation::new();
        // insert one active entity with stable_id 1 (use the module's helper)
        sim.entities.insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        sim.reveal(1);
        assert!(sim.entities.get(1).unwrap().in_logic_vector);
        assert_eq!(sim.live_object_order_snapshot(), vec![1]);
        sim.conceal(1);
        assert!(!sim.entities.get(1).unwrap().in_logic_vector);
        assert!(sim.live_object_order_snapshot().is_empty());
        assert!(sim.entities.get(1).is_some()); // conceal keeps the store slot (limbo)
    }

    #[test]
    fn unlimbo_equals_reveal_appends_member() {
        let mut sim = Simulation::new();
        sim.entities.insert(GameEntity::test_default(7, "E1", "Americans", 3, 3));
        sim.unlimbo(7);
        assert!(sim.entities.get(7).unwrap().in_logic_vector);
        assert_eq!(sim.live_object_order_snapshot(), vec![7]);
    }

    #[test]
    fn uninit_conceals_then_frees_store_slot() {
        let mut sim = Simulation::new();
        sim.entities.insert(GameEntity::test_default(2, "MTNK", "Americans", 4, 4));
        sim.reveal(2);
        sim.uninit(2);
        assert!(sim.entities.get(2).is_none(), "uninit frees the store slot");
        assert!(sim.live_object_order_snapshot().is_empty(), "uninit leaves the order");
    }

    #[test]
    fn despawn_entity_delegates_to_uninit() {
        let mut sim = Simulation::new();
        sim.entities.insert(GameEntity::test_default(3, "MTNK", "Americans", 6, 6));
        sim.reveal(3);
        sim.despawn_entity(3);
        assert!(sim.entities.get(3).is_none());
        assert!(sim.live_object_order_snapshot().is_empty());
    }

    #[test]
    #[cfg(debug_assertions)]
    fn lifecycle_keeps_membership_invariant() {
        let mut sim = Simulation::new();
        for id in [1u64, 2, 3] {
            sim.entities.insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
            sim.reveal(id);
        }
        sim.conceal(2);
        sim.uninit(1);
        sim.debug_assert_logic_membership_consistent(); // must not panic
        assert_eq!(sim.live_object_order_snapshot(), vec![3]);
    }
```
(If `GameEntity::test_default(id, type, owner, rx, ry)` isn't the exact helper signature in scope,
use the constructor the surrounding `snapshot.rs` tests already use — match it verbatim.)

**Step 2: Verify**
Run: `cargo test -p <sim-crate> reveal_then_conceal_roundtrips_membership unlimbo_equals_reveal_appends_member uninit_conceals_then_frees_store_slot despawn_entity_delegates_to_uninit lifecycle_keeps_membership_invariant -- --nocapture`
Expected: 5 PASS.

**Step 3: Commit** — `git commit -am "sim/snapshot: Slice-1 lifecycle chokepoint acceptance tests"`

---

### Task 8: Full regression + parity-neutrality verify

**Why:** Prove the slice changed naming/structure only — not behavior or the deterministic hash.

**Step 1:** Run the full sim test suite: `cargo test -p <sim-crate>`.
Expected: same pass set as the pre-slice baseline. **Known baseline failures** (per the live-pass
contract doc) are movement×4, ai×1, ore_growth×1, production×4 — if exactly those still fail and
nothing new, that is the expected baseline; if any *new* failure appears, stop and diagnose (Slice 1
must not introduce failures).

**Step 2: State-hash neutrality.** Identify an existing deterministic replay/advance-tick fixture
(e.g. the `binary_frame_*` tests in `world_hash.rs:1160+`, or any replay-hash regression test). Run it
before and after is implicit since it lives in the same suite — confirm it still passes. If a dedicated
"replay produces hash H" golden fixture exists, run it and confirm H is unchanged. If none exists,
record that as a gap (a replay-hash golden would strengthen future slices) but do **not** add one in
this slice.

**Step 3:** `cargo clippy -p <sim-crate>` — no new warnings from the changed files.

**Step 4:** Record results (pass counts, any baseline failures) in the plan's execution notes.

---

### Task 9: Final commit + study-doc status update

**Why:** Close the slice and mark it done in the substrate study.

**Step 1:** Update [docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) §8 Slice 1 to
"DONE (2026-05-29)" with a one-line result (call sites migrated, invariant added, hash unchanged).
(`docs/` is local-only — no commit for the doc.)

**Step 2:** Ensure all code commits from Tasks 1-7 are present: `git log --oneline -8`.

**Step 3:** Leave the branch on `dev` (per project git workflow — commit feature work directly to
`dev`, no PR/push unless the user asks).

---

## Sources & References

- **Design / study:** [docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) (§4.1 primitive
  fidelity, §6 boundary, §7 retire list, §8 Slice 1); `docs/plans/2026-05-28-logicclass-object-lifecycle-spine-design.md`
  (Lifecycle helpers, Chosen Approach); `docs/plans/2026-05-28-logicclass-scheduler-live-pass-contract.md`
  (primitive already built).
- **gamemd.exe (verified this session, kept out of Rust comments):** `ObjectClass::Reveal 0x005F4EC0`
  → register `0x0055BAA0`; `ObjectClass::Conceal 0x005F4D30` → remover `0x0055BAE0`; `ObjectClass::UnInit
  0x005F65F0`; PerTickUpdate `0x0055AFB0`; singleton `0x0087F778`.
- **Repo code:** `src/sim/world/mod.rs` (lifecycle primitives `:667-788`, `for_each_live_object :711`,
  `advance_tick :1450`, `state_hash` call `:2031`); `src/sim/world/logic_vector.rs`;
  `src/sim/game_entity.rs:172` (`in_logic_vector`); `src/sim/world/world_hash.rs:47-53` (order hashed);
  `src/sim/snapshot.rs:240-427` (scheduler test module to extend).
- **Call sites migrated:** register→reveal (`world_spawn.rs:260,438`, `passenger.rs:881,1034`,
  `production_sell.rs:459`, `drop_payload.rs:240`); unregister→conceal (`passenger.rs:487`);
  despawn→uninit (`app_sim_tick.rs:306`, `slave_miner.rs:473,555`, `world_spawn.rs:738`,
  `world_orders.rs:244,408`, `mod.rs:1276`).
- **INI keys:** none.
