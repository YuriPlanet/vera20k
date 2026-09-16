# AI Shell Migration — Plan 3: Leaves + commence authority

**Status:** DRAFTED — not approved.
**Date:** 2026-06-02
**Rule:** Rust-native structure, gamemd-native semantics.
**Companion docs:** [docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md) (the design doc; §S6–S8, slice ledger, §10 negative facts) + the mission/radio substrate plan (Slices 0–6, of which 0–3 are landed: commits `d41352b7`, `792d6051`, `ff1d2a32`, `6943e8ed`).

---

## Overview

This plan migrates the remaining `TechnoClass`/`FootClass` leaves onto the per-object AI shell and flips the commence/`MissionCom` selector to authoritative, completing the substrate work that Plan 1 (Mission/radio Slices 0–3) and Plan 2 (the shell host S0–S5) begin. It is sequenced strictly by dependency: aircraft dispatch (L3) and infantry fear/sequence parity (L4) are leaf relocations that can proceed against the already-landed `for_each_live_object` driver; the commence-gate structure + `MissionCom` authority flip (L6) is the one hash-moving authority change and is gated on the Verify A1 busy-byte findings; the BuildingClass wrapper (L7) is outline-only and last because of its blast radius into HouseClass/Factory globals. Every behavior-moving step lands shadow-first (serde-skip, not hashed, `debug_assert` agreement) and flips only behind a named acceptance test that pins gamemd order, taking a `SNAPSHOT_VERSION` bump + golden rebaseline at each hash boundary. The plan does **not** invent slices beyond the four reviewed here, and it explicitly surfaces (rather than papers over) every UNCHECKED busy byte, deferred scatter path, and approach-(B) deviation from the design doc's literal S5-dependency.

---

## Dependency order & gating

**Execution order:** **L3 aircraft → L4 infantry fear → L6 commence gate + MissionCom authority → L7 building (outline, last).**

- **Depends on Plan 1** (Mission/radio Slices 0–3, landed): `MissionCom` shadow component, `MissionTimer`, `Contacts` slot model, verb/retask scaffolding.
- **L3 (aircraft):** approach (B) decouples from the absent S5 `techno_ai.rs` shell by driving an aircraft-scoped `dispatch_aircraft_mission` via the already-landed `for_each_live_object`. **Gate:** confirm approach (B) vs blocking on S5 with the user before coding — (B) deliberately rescopes two design-named tests (see L3 Review notes #1, #2). L3's crash/OOB numerics are additionally gated on a fresh Ghidra trace.
- **L4 (infantry fear):** value-correction Tasks 1–5 can proceed against the current global sweep independently; **Task 6 (relocate into the shell) is a HARD BLOCKER on S0/S1** (`techno_ai.rs`/`object_ai_stage`/`infantry_ai` do not exist yet) and its mission-exclusion guard depends on **S5** (MissionCom authority) for the mission source.
- **L6 (commence gate + MissionCom authority):** **additionally gated on the Verify A1 findings.** The reviewed L6 plan establishes that the fabricated "A1 lane report" must be stripped and every busy byte ships as an UNCHECKED stub until A1's setter traces land; the authority flip (sub-step B) is the single hash boundary in this plan. Depends on Plan 1 Slices 0–3 + the landed Slice-6 verb/retask scaffolding.
- **L7 (building):** **outline-only, last.** Hard-blocked on S0–S7 landing (no `sim/ai/` shell exists today) + S5 MissionCom building leg + a fresh `BuildingClass::Update` re-decode. Per design §10.2, the leaf migration must **not** start with BuildingClass; this slice only becomes actionable tasks after L3/L4/L6 and the shell host land.

**Verify A1 (gating L6):** the busy-byte/ready-flag setter traces. L6 is authored to ship the commence-gate *structure* with UNCHECKED busy stubs regardless of A1; A1's findings determine only when each leaf busy flag becomes field-accurate (a later slice), and L6's review explicitly removed all invented A1 addresses. Treat A1 as the precondition for *field-accuracy*, not for the structural slice.

---

### Slice L3 (= design Slice S7) — Aircraft missions under per-object dispatch

> **Review notes (what I corrected in the draft, verified against the live tree this session):**
> 1. **`aircraft_ai_body_is_thin_shell` test reinterpretation was too quiet.** The design's intent (`:873`) pins a *gamemd structural fact* — "the AI body only clears the **one-shot mission byte**." Approach (B) builds **no `AircraftClass::AI` shell and no one-shot byte** (those are S5's `techno_ai.rs` surface). The draft silently relabeled the test to "thin dispatcher." Corrected: the test is **renamed and rescoped** to what (B) actually proves, and the original byte-clear assertion is explicitly **deferred to S5** so we don't ship a test whose name implies a structure we didn't build.
> 2. **AreaGuard test was wrong about the mechanism.** Under (B) there is **no MissionClass `+0x220` 450-stub** and **no `AreaGuard` variant** in `AircraftMission` (verified `aircraft/mod.rs:36-124`). The draft said it "routes to the Sleep/idle equivalent" — no such routing exists. Corrected to pin the real (B) fact: AreaGuard is **unrepresentable** for aircraft; the dispatcher has no arm for it.
> 3. **Invariant-#6 diagnosis sharpened and confirmed.** Verified the leak is real: `tick_animations` (`animation.rs:402-407`) only reaps a no-anim `dying` entity on the **next app-layer sweep** (`app_sim_tick.rs:300-313`), so a self-destruct aircraft lingers in occupancy + logic order through vision/power/combat/AI/defeat/state_hash **for the rest of the current tick** — a cross-phase dying-window leak identical in shape to the command-death leak the engine already drains pre-Phase-1 (`mod.rs:1763-1769`). The draft's "eventually uninits it" was too vague.
> 4. **Crash `−400` threshold provenance.** The design (`:325`, `:875`) cites it as decompile-sourced ("crash-flag Z descent w/ −400 destroy"). The FACTS block did **not** re-verify it this cycle. The draft's re-gating on a fresh Ghidra trace is **correct discipline** — kept, with the design provenance noted so the trace is a confirmation, not a from-scratch dig.
> 5. **Stale design line cite.** Design `:392`/`:864`/`:866` cite the global sweep as `aircraft/mod.rs:144..183`; live tree is fn `tick_aircraft_missions` `:154`, snapshot filter `:165-179`. The draft already re-anchored — confirmed correct.
> 6. **Verified-correct in the draft (no change):** AirfieldDocks has **no FIFO** (`aircraft_dock.rs:107` "There is no wait queue"; the FACTS-block "non-native FIFO RETIRE" is **stale** — the draft caught this). `air_movement.rs` has **no** crash/−400/OOB path (`tick_altitude:529`; `jumpjet_crash_speed` is a descent *speed*, not a self-destruct floor) — crash/OOB are genuinely new behavior. All world/mod.rs anchors (`:1902`, `:947`, `:929`, `:1010`, `:1060`, `:895`, `:1742`, `:2078`, `:2254`) confirmed. `derived_mission` aircraft arm `:487-498`. Test registration goes at `mod.rs:2430` next to `mod slice6_retask_tests;`.

**Status:** AUTHORED PLAN (read-only research complete; no Rust written this session). Source of truth: design §S7 (`:862-876`) + dependency table (`:902-914`) + §10 negative facts, reconciled against the **live Rust tree** (every file:line below re-Read this session; the FACTS block and design line numbers are stale — re-Read before editing).

**Naming:** task "Slice L3" = design **Slice S7**. Same slice.

---

#### 1. Approach choice (brainstorm)

The honest dependency reading (`:911`: *S7 depends on S5*) is that S7-as-designed cannot land: its `match category` shell home — `src/sim/world/techno_ai.rs` — **does not exist** (`Glob` → no file; S0–S5 of this substrate are unlanded; only Mission/radio Slices 0–3 are committed). Two candidate approaches:

- **(A) Block S7 on S5.** Wait for the `techno_ai.rs` shell + `Mission_Dispatch` scaffold, then S7 is a thin "relocate the aircraft sweep under dispatch" change. Clean, but stalls L3 behind unscheduled work, and the per-object-order parity win (the *point* of S7) is deliverable independently of the full MissionCom-authority stack.
- **(B) Land a minimal aircraft-scoped dispatch entry point now** via the *already-landed, proven* `for_each_live_object` (`mod.rs:947`, native same-pass re-read semantics), without the absent `Mission_Dispatch`/`MissionCom`-authority machinery. The aircraft state already lives in a self-contained `Option<AircraftMission>` machine with read-only handler bodies (`tick_attack_state`, `tick_approach`, `tick_overfly`, `enter_idle_mode`, `try_drop`); the only thing the global sweep adds over per-object dispatch is **BTreeMap-id iteration order instead of live logic order** — the exact DRIFT S7 exists to fix (`:392`), fixable with a `match category == Aircraft` guard inside a per-object body. No class tree, no `dyn`, invariant #2 honored.

**Chosen: (B), scoped as 3 steps (S7a shadow → S7b crash/OOB + deferred-death fix → S7c order flip).** It delivers the named acceptance test (per-object live-order dispatch with RTB/dock ordering preserved) without inventing the absent S5 shell, and isolates the genuinely *new* behavior (crash-descent + OOB kill) into its own step so the order-flip step stays a pure iteration-order change with a clean shadow proof. The plan does **not** create `techno_ai.rs`; it adds an aircraft-scoped dispatch fn + a `for_each_live_object` caller, so when S5 lands its shell, this fn becomes the aircraft arm S5 absorbs (no rework, just a call-site move).

**MUST resolve with the user before coding (gate):** approach (B) is a deliberate deviation from the design's S5-dependency. Confirm (B) vs blocking on S5. *This deviation also rescopes two of the design's named acceptance tests* (see Review notes #1, #2) — surface that explicitly in the same gate.

---

#### 2. Goal

Replace the global BTreeMap-id-ordered aircraft mission sweep (`aircraft/mod.rs:154`, iterating `sim.substrate.entities.values()`) with **per-object dispatch in live logic order** via `for_each_live_object` (`mod.rs:947`), so a mid-pass spawned/retasked aircraft observes native same-pass scheduler semantics and aircraft interleave with each other and with ground systems as gamemd orders them. Additionally: introduce crash-descent and map-bounds-strafe OOB kill (both absent in Rust today), and route those **plus the existing self-destruct paths** through deferred death (`uninit` → `pending_delete`), fixing a standing invariant-#6 violation. Do **not** implement Aircraft AreaGuard or the `+0x294` airstrike-deaf latch.

---

#### 3. Files / surfaces (exact file:line, live tree — re-verified)

| Surface | file:line | Role in this slice |
|---|---|---|
| Global sweep `tick_aircraft_missions` | `src/sim/aircraft/mod.rs:154` | RETIRE — body split into a per-object dispatch fn |
| Phase-1 snapshot (`entities.values()` BTreeMap-order filter) | `aircraft/mod.rs:165-179` | RETIRE — replaced by `match category==Aircraft` guard inside per-object body |
| Phase-2 match (per-`AircraftMission` arm) | `aircraft/mod.rs:225-618` | MOVE verbatim into `dispatch_aircraft_mission` per-object body |
| Phase-3/4/5 apply (mutate/air-move/fire/paradrop/despawn) | `aircraft/mod.rs:623-779` | MOVE into per-object apply, inlined after the per-object compute |
| Existing self-destruct (Idle/Guard) | `aircraft/mod.rs:625-632` | FIX — direct `dying=true` write → route through deferred death |
| Existing self-destruct (silent despawn / Overfly exit) | `aircraft/mod.rs:771-779` | FIX — same |
| Call site in `advance_tick` | `src/sim/world/mod.rs:1902` (comment `:1899` "between movement and combat") | REWIRE — drive per-object dispatch via `for_each_live_object`; phase position unchanged |
| `for_each_live_object` (native same-pass iterator, re-reads `logic.len()` each step) | `mod.rs:947-954` | USE — the live-logic-order driver replacing `.values()` |
| `live_object_order_snapshot` (NO sort) | `mod.rs:929` | reference (order source) |
| `uninit` (conceal→unmark→Dying→enqueue) | `mod.rs:1010-1046` | USE — deferred-death entry for crash/OOB/self-destruct |
| `flush_pending_delete` | `mod.rs:1060` (drains @ `:1719`, `:2254`; app-layer @ `app_sim_tick.rs:313`) | reference (slot-free drain; do **not** add a new drain) |
| Combat immediate-death channel (`immediate_uninit_ids` → `self.uninit`) | `mod.rs:2078` | reference — the existing in-tick `uninit` pattern S7b mirrors |
| `refresh_mission_shadow` def / call / asserts / hash | def `:895`, `debug_assert_mission_shadow_consistent` `:908`; (call + hash are in `run_late_region`, re-grep before editing) | reference — S7c is hash-affecting; shadow agreement already wired pre-hash |
| `derived_mission` aircraft arm | `game_entity.rs:487-498` | reference — aircraft MissionCom mapping (unchanged by S7) |
| `tick_attack_state` | `aircraft/attack_mission.rs` (`&EntityStore`, returns `AttackTickResult`) | KEEP — called per-object |
| `tick_approach` / `tick_overfly` | `aircraft/paradrop_mission.rs` | KEEP — called per-object |
| `enter_idle_mode` | `aircraft/idle_mode.rs:41` | KEEP — called per-object |
| `try_drop` | `aircraft/drop_payload.rs` | KEEP — called per-object |
| `AirfieldDocks` (slot model, **no FIFO**) | `src/sim/docking/aircraft_dock.rs:100-120` (esp. `:107` "no wait queue") | reference — FACTS-block "non-native FIFO RETIRE" is **STALE**; nothing to retire. The "first-empty-slot, emergent iteration-order" comment (`:104-112`) is the mechanism S7c's order flip perturbs |
| `tick_altitude` (altitude SM; **no crash/OOB**) | `src/sim/movement/air_movement.rs:529` | reference — confirms crash/OOB don't exist; S7b adds them |
| `tick_animations` no-anim dying reap | `src/sim/animation.rs:402-407` | reference — proves the invariant-#6 leak: a no-anim `dying` self-destruct is reaped only on the *next* app-layer sweep |
| App-layer death-anim reaping + drain | `src/app_sim_tick.rs:300-313` | reference — current self-destruct frees only via this sweep, one+ ticks late, never synchronous unmark |
| Test module registration | `mod.rs:2430` (`mod slice6_retask_tests;`) | ADD `#[cfg(test)] mod slice7_aircraft_tests;` beside it |
| Existing test model | `src/sim/world/slice6_retask_tests.rs` | model the new test file on this |

**Stale-citation reconciliation (verified):** `tick_aircraft_missions` `:154` (FACTS said `:152`); call site `mod.rs:1902` (FACTS `:1866`); `advance_tick` def `:1742`; `for_each_live_object` `:947`; `flush_pending_delete` `:1060`; `live_object_order_snapshot` `:929`; `uninit` `:1010`; `refresh_mission_shadow` def `:895`. Design `:864/:866/:392` cite the sweep as `aircraft/mod.rs:144..183` — stale; use the live `:154` / `:165-179`. Re-grep every one before writing edits.

---

#### 4. Step-by-step tasks

S7 is an **absorb** (relocate-a-sweep) + one genuinely-new-behavior step. The absorbs give the **function-move mapping + new step signatures + shadow→authority transition**, not speculative full bodies. S7b's crash/OOB structure is concrete, but its numeric thresholds + exact trigger are gated on a Ghidra trace (§8).

##### S7a — Shadow: per-object dispatch fn, computed but NOT yet driving the tick

**New surface:** `pub(crate) fn dispatch_aircraft_mission(sim: &mut Simulation, rules: &RuleSet, id: u64, path_grid: Option<&PathGrid>)` in `aircraft/mod.rs`.

**Function-move mapping** (exact, no behavior change):
- Phase-1 filter (`:168-173`: `aircraft_mission.is_some()` + `loco.kind == Fly`) becomes a **guard at the top**: `let Some(e)=sim.substrate.entities.get(id) else {return}; if e.category != EntityCategory::Aircraft {return}` then the same `Fly` + `aircraft_mission.is_some()` guard. (Category gate is the invariant-#2 compare, not a trait.)
- Phase-2 `match &snap.mission { … }` arms (`:225-618`) move **verbatim** into the body, operating on the single `id` instead of `snap.id`. The `MissionMutation` accumulator (`:186-202`) shrinks to a single per-call `m`.
- Phase-3/4/5 apply blocks (`:623-779`) move inline **after** the match, applied to the single `id` (compute-then-apply per object — this is the interleave change S7c flips; in S7a it is shadow only).

**Shadow harness (ships, does NOT flip the tick):** keep the global `tick_aircraft_missions` as the **authoritative** driver at `mod.rs:1902`. Add a `#[cfg(debug_assertions)]` block that, *before* the global sweep, walks `for_each_live_object` and records what `dispatch_aircraft_mission` *would* produce (new_mission, ammo_delta, move_to, fire_at) into a scratch map, then after the global sweep `debug_assert!`s the per-object result equals the global-sweep result **for every aircraft whose outcome does not depend on cross-aircraft ordering** (single-aircraft scenarios: paradrop carrier, lone interceptor RTB). Where outcomes legitimately differ due to shared-state interleave (two aircraft racing for the same pad via `AirfieldDocks::try_reserve`), **log** the divergence — that divergence is the intended S7c behavior. Hash-neutral: nothing here writes the authoritative store except the unchanged global sweep.

**Becomes:** nothing authoritative. `dispatch_aircraft_mission` exists and is shadow-validated.

**Verify:** `cargo check -p vera20k`; run an existing aircraft test (paradrop/carryall) in debug — no single-aircraft `debug_assert` fires.

##### S7b — FIX (new behavior + deferred-death routing): crash-descent, OOB kill, self-destruct teardown

Adds the two missing kill paths and fixes the invariant-#6 leak. Gated on a Ghidra trace of the gamemd crash/OOB thresholds — **do not ship the numeric −400 / bounds rule until traced** (design `:325/:875` cites it as decompile-sourced, but the FACTS block did not re-verify it this cycle; treat the trace as confirmation). Until then this step lands only the *deferred-death routing fix* (verified-needed) and a `// UNCHECKED` crash stub.

- **Deferred-death routing (ship now, verified):** the two self-destruct sites — Idle/Guard (`aircraft/mod.rs:625-632`) and silent-despawn (`:771-779`) — currently do `entity.health.current = 0; entity.dying = true; entity.aircraft_mission = None` directly. **Verified leak:** that direct write leaves the entity in its cell + logic order; `tick_animations` (`animation.rs:402-407`) reaps a no-anim `dying` entity only on the **next app-layer sweep** (`app_sim_tick.rs:300-313`), so it lingers through vision/power/combat/AI/defeat/state_hash for the **rest of the current tick** — a cross-phase dying-window leak identical in shape to the command-death leak the engine already drains pre-Phase-1 (`mod.rs:1763-1769`). **Fix:** these sites call `sim.uninit(id)` (synchronous conceal + unmark, deferred slot-free) — the same in-tick pattern as combat's `immediate_uninit_ids` (`mod.rs:2078`). **Decision to make explicit for the user:** the silent-despawn (Overfly exit, no death anim) must NOT spawn a death animation → it routes straight through `uninit`. The AirportBound self-destruct may play a death anim in gamemd — **trace whether it should keep the anim** (spawn anim, let it route via the existing anim→`uninit` path but with synchronous unmark) **or be silent** (direct `uninit`) before choosing; do not guess.

- **Crash-descent + OOB kill (gated, stub now):** add `aircraft_self_termination_check(loco, position, map_bounds) -> Option<DeathKind>` returning crash (Z below the descent floor) or OOB-strafe-exit; both `Some` arms route through the same `uninit` call. Until the trace lands, this fn returns `None` behind a `// UNCHECKED: crash-descent threshold (design cites -400, re-verify in <AircraftClass::AI body>)` marker, and `aircraft_crash_and_bounds_kill_pending_delete` is `#[ignore]`d with a comment naming the blocking trace.

**Becomes authoritative:** the deferred-death routing of self-destruct (immediately). Crash/OOB becomes authoritative only after the trace fills the stub.

**Verify:** `cargo check -p vera20k`; test `aircraft_self_destruct_routes_through_deferred_death` (below) asserts synchronous conceal/unmark the same tick.

##### S7c — Authority flip: per-object live-order dispatch replaces the global sweep

- At `mod.rs:1902`, replace `crate::sim::aircraft::tick_aircraft_missions(self, rules, path_grid)` with a `for_each_live_object` walk calling `dispatch_aircraft_mission(self, rules, id, path_grid)` per id (the fn's internal `category==Aircraft` guard skips non-aircraft cheaply). **Phase position unchanged** (still between Phase 2.5 rocking and Phase 3 vision — invariant #3).
- Delete the global `tick_aircraft_missions` body (Phases 1–5) and the S7a shadow harness; keep `dispatch_aircraft_mission` + shared helpers (`find_nearest_airfield_for:784`, etc.).
- **Same-pass semantics now apply:** a body that retasks an aircraft (Guard→Attack) commits before the next aircraft runs; a mid-pass-spawned aircraft (rare for air, possible via carryall drop) is visited the same pass (`for_each_live_object` re-reads `logic.len()` each iteration, `mod.rs:949`).

**Becomes authoritative:** per-object live-logic-order aircraft dispatch. Iteration order changes BTreeMap-id → logic-vector order. Hash-affecting (`:911`) → `SNAPSHOT_VERSION` bump + rebaselined gamemd-cited golden.

**Verify:** named acceptance tests below; full-skirmish replay determinism.

---

#### 5. What becomes authoritative vs shadow

| Item | S7a | S7b | S7c |
|---|---|---|---|
| `dispatch_aircraft_mission` per-object fn | shadow (computed, asserted) | shadow | **authoritative** (drives the tick) |
| Aircraft iteration order | BTreeMap-id (global sweep) | BTreeMap-id | **logic order** (`for_each_live_object`) |
| Self-destruct deferred-death routing | unchanged (direct `dying=true`) | **authoritative** (via `uninit`) | authoritative |
| Crash-descent / OOB kill | n/a | stub (`// UNCHECKED`, test `#[ignore]`) | authoritative *only after Ghidra trace* |
| MissionCom aircraft shadow (`derived_mission:487`) | unchanged (still shadow) | unchanged | unchanged (S7 does not flip MissionCom; that's S5) |

S7 does **not** make `MissionCom` authoritative and does **not** touch `refresh_mission_shadow` — the aircraft mission stays the authoritative `Option<AircraftMission>` machine; S7 only changes *who iterates it and in what order*.

---

#### 6. Named acceptance tests (exact fn names)

New file `src/sim/world/slice7_aircraft_tests.rs` (modeled on `slice6_retask_tests.rs`); register `#[cfg(test)] mod slice7_aircraft_tests;` at `mod.rs:2430`.

- **`aircraft_missions_dispatched_not_global_sweep`** — (the task's + design `:874` named test) build two aircraft with stable_ids ordered so BTreeMap-id order ≠ logic order; assign a pad-contended RTB/Docking scenario; assert each aircraft's mission advances in **logic-vector order** (the one earlier in `live_object_order_snapshot` wins the pad via `try_reserve` first) and the RTB→Docking sub-state progression is identical to the recorded pre-flip single-aircraft sequence. Pins both "per-object live order" and "RTB/dock ordering preserved."
- **`aircraft_dispatch_is_thin_router_not_inline_sm`** — (replaces the design's `aircraft_ai_body_is_thin_shell`; see Review note #1 — the design's "one-shot byte clear" structure does **not** exist under approach (B), so this test pins what (B) actually builds:) `dispatch_aircraft_mission` for a non-aircraft entity (Unit) is a no-op early-return (category guard); for an aircraft it only routes to a handler body — no state-machine logic inlined outside the per-mission handlers. **Comment in the test must state:** the gamemd "AI body clears one-shot mission byte" assertion is **deferred to S5** (`techno_ai.rs` shell), not modeled here.
- **`aircraft_crash_and_bounds_kill_pending_delete`** — (design `:875`) crash-descent (Z below floor) and OOB-strafe exit each enqueue to `pending_delete` via `uninit` (entity concealed/unmarked synchronously, slot freed at `flush_pending_delete`). **`#[ignore]` until the Ghidra crash/OOB threshold trace lands** (comment names the blocking trace).
- **`aircraft_self_destruct_routes_through_deferred_death`** — (S7b, ships now) an AirportBound interceptor with no airfield self-destructs; assert it is **concealed and unmarked from its cell the same tick** (presence Dying, gone from occupancy + logic order) and freed at `flush_pending_delete` — NOT lingering in-cell awaiting the app-layer anim sweep. Pins the invariant-#6 fix.
- **`aircraft_areaguard_unrepresentable_no_dispatch_arm`** — (replaces the design's `aircraft_never_areaguard_inherited_stub`; see Review note #2 — under (B) there is no MissionClass 450-stub and no `AreaGuard` variant:) assert `AircraftMission` has **no AreaGuard variant** and the dispatcher has **no arm** that could enter one — AreaGuard is unrepresentable for aircraft. Pins the do-not-implement (design `:870`, `:938-939`) at the type level. **Comment:** the gamemd "inherited 450-stub" framing is an S5-shell concept, not modeled here.
- **`s7a_per_object_shadow_zero_divergence_single_aircraft`** — (S7a) full single-aircraft paradrop+carryall replay: the shadow per-object result equals the global-sweep result every tick (the `debug_assert` never fires); proves the relocation is behavior-identical *before* the order flip.

---

#### 7. Determinism / hash notes

- **Hash-affecting: S7c only.** The order flip changes aircraft processing order from BTreeMap-id-ascending to logic-vector order, which can change `state_hash` (e.g. which of two pad-racing aircraft wins the slot via the first-empty-slot/emergent-order `AirfieldDocks` model `aircraft_dock.rs:104-112`, changing occupancy + downstream positions). Per invariants #4/#8: S7c bumps `SNAPSHOT_VERSION` and rebaselines the golden, justified by the design evidence that aircraft AI runs per-object under `FootClass::AI`, not as an id-ordered sweep.
- **S7a hash-neutral.** Shadow compute writes nothing authoritative.
- **S7b may be hash-affecting (surface to user).** The deferred-death fix moves a self-destruct entity out of occupancy + logic order **synchronously** instead of one+ ticks later via the app-layer sweep. This *does* change which raw-store consumers (vision `refresh_fog`, power) and the `state_hash` count the entity within the self-destruct tick — i.e. it is a **real behavior fix**, not neutral, in the self-destruct case. Replay a self-destruct scenario and diff the hash: if it moves, S7b also takes the `SNAPSHOT_VERSION` bump. Do **not** silently fold it; surface to the user. (The draft's "confirm this does not alter the hash" was optimistic — given the verified leak, expect it to move.)
- **No new RNG draw.** S7 consumes no RNG; per-object dispatch does not reorder RNG within a single aircraft. Invariant #7 holds. (Note: the order flip *does* change the global RNG-consumption order *across* aircraft if any handler draws — verify no aircraft mission handler draws RNG; if one does, that reorder is part of the S7c hash change and must be in the golden rebaseline rationale.)
- **Frame-anchored timers untouched.** `Docking.reload_timer` stays a `MissionTimer` (`aircraft/mod.rs:78`), re-armed via `MissionTimer::armed(now,…)` (`:491/:513`), never decremented. Invariant #5 holds.
- **Deferred death preserved.** Crash/OOB/self-destruct route through `uninit` → conceal + unmark synchronous, `pending_delete` slot-free deferred. The one-tick Dying window is preserved. Invariant #6 holds.
- **Shadow-first.** Authority (the order flip) lands only after S7a's zero-divergence single-aircraft shadow proof. Invariant #4 holds.

---

#### 8. Dependencies + risk + do-not-do

**Dependencies:**
- **Design says S7 depends on S5** (`:911`). Approach (B) **decouples** S7 from S5 by standing up an aircraft-scoped `dispatch_aircraft_mission` driven by the *already-landed* `for_each_live_object`, not the absent `techno_ai.rs` shell. **Resolve with the user before starting** — and flag in the same gate that (B) rescopes two design-named tests (Review notes #1, #2). If S5 is required first, this fn becomes the aircraft arm of S5's `match category` shell (no rework — move the call site).
- **S7b's crash/OOB depends on a Ghidra trace** (not done — read-only grounding this session). Decompile `AircraftClass::AI 0x00414BB0`, confirm the crash-descent floor (design says `−400`, re-verify), and the map-bounds-strafe OOB rule before the numerics ship. The deferred-death *routing* half does not need the trace.

**Risk:**
- **Interleave change (S7c, MEDIUM, fires every match with ≥2 aircraft sharing an airfield).** Per-object compute-then-apply means aircraft N's mutation is visible to N+1 in the same pass (shared `AirfieldDocks`, shared occupancy). The design's stated parity risk (`:870`) and the *point* of the slice — but the highest-blast item. Mitigation: S7a's shadow harness logs every cross-aircraft divergence before the flip, so the golden rebaseline is reviewed against an explicit divergence list.
- **Self-destruct hash shift (S7b, LOW frequency — fires only when an AirportBound aircraft loses its airfield, but verified-real when it does).** The deferred-death fix moves the entity out of occupancy/logic the same tick; expect the hash to move. Take the version bump if it does.

**Do NOT do:**
- Do **not** implement Aircraft **AreaGuard** — inherited 450-stub, never live for aircraft in YR (design `:870`, `:938-939`). The test pins it unrepresentable.
- Do **not** implement the `+0x294` airstrike-owner radio-deaf latch — deferred back-pointer detail (design `:870`), mark UNCHECKED.
- Do **not** retire an `AirfieldDocks` FIFO — **there is none** (`aircraft_dock.rs:107`; the FACTS-block claim is stale). Do not re-introduce a wait queue.
- Do **not** create `src/sim/world/techno_ai.rs` (S5's surface).
- Do **not** move the `advance_tick` phase position of aircraft dispatch (stays between Phase 2.5 and Phase 3 vision — invariant #3).
- Do **not** ship the −400/OOB numerics without the Ghidra trace; leave S7b's crash stub `// UNCHECKED` and the crash test `#[ignore]`d.
- Do **not** flip `MissionCom` to authoritative here — S7 is iteration-order + deferred-death only.
- Do **not** silently keep the original design test names `aircraft_ai_body_is_thin_shell` / `aircraft_never_areaguard_inherited_stub` — under approach (B) they assert structures that don't exist; use the rescoped names in §6 and document the S5 deferral inline.

**Relevant files (absolute):** `src/sim/aircraft/mod.rs`, `...\src\sim\aircraft\idle_mode.rs`, `...\src\sim\world\mod.rs`, `...\src\sim\game_entity.rs`, `...\src\sim\docking\aircraft_dock.rs`, `...\src\sim\movement\air_movement.rs`, `...\src\sim\animation.rs`, `...\src\app_sim_tick.rs`, new `...\src\sim\world\slice7_aircraft_tests.rs`, model `...\src\sim\world\slice6_retask_tests.rs`. Design doc: [docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md) (§S7 `:862-876`, table `:902-914`, §10 negative facts `:938-939`).

---

### Slice L4 — Infantry fear / sequence parity under the InfantryClass shell

**Review notes (what I corrected in the draft):**
- **Stale line citations in the design doc, NOT in the draft:** the corrections block (`design:38`) and the task's FACTS block both say `tick_fear_for_entities` is "called `world/mod.rs:1960`." Verified live: the actual call site is **`world/mod.rs:1996`** (deploy `tick_deploy_state` at `:1992`, combat `tick_combat_with_fog` at `:2027`). The draft plan used `:1996` throughout and was **correct** — the FACTS block is the stale one. Kept `:1996`; flagged the FACTS drift inline so the implementer trusts the live tree.
- **`techno_ai.rs` / the shell scheduler does NOT exist.** Glob for `src/sim/world/techno_ai.rs` returned nothing; Grep for `object_ai_stage|infantry_ai|techno_ai` across `src/sim` returned **zero matches**. The shell is purely a design-doc concept (§7.2/§7.3). The draft hedged this in §8 but still listed `techno_ai.rs` in its surfaces table as if it were a live file. **Corrected:** every reference to the shell file is now explicitly "net-new, created by S0/S1; if S0/S1 has not landed, Task 6 is BLOCKED." This is the single most important sequencing correction.
- **Deferred-death API line:** `uninit` begins at **`world/mod.rs:1010`** (draft said `:1018-1046`; `:1018` is mid-body). `despawn_entity :1050`, `flush_pending_delete :1060` confirmed. Corrected the range.
- **Animation cascade lines:** the prone/recovery selection + in-progress Down/Up guards are at **`animation.rs:455-495`** (guards at `:461,:477,:483,:490`), `switch_to` at **`:204`**, `sequence_is_prone` at `:217`. Draft's `:442-500` was approximately right; tightened.
- **`fear_level`/`is_prone` are hashed** (`InfantryRuntime` at `game_entity.rs:48` derives `Serialize/Deserialize`, no `#[serde(skip)]`) — confirmed; the draft's "value corrections rebaseline the golden, no shadow needed for the values" reasoning holds.
- **Fear iterates `keys_sorted()` (BTreeMap id-ascending)** at `infantry.rs:135` — confirmed; the relocation-to-LOGIC-order being the genuine hash-affecting flip is correct.
- **"199" threshold** — confirmed UNVERIFIED in the decay handler; it traces to the corrections block's loose "49/50/199" phrasing, not to a decompile. Kept the draft's hard block on pinning a 199 test.
- Everything else in the draft (approach B, deferred-death routing, Fraidycat/panic deferral, no-MissionTimer-refactor, no Panic emission) checks out against the 8 invariants. No invariant violations found.

---

#### 1. Status & mapping

Maps to design **Slice S6** (`TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md:844`). The S6 body says fear is "currently absent / entirely unimplemented" (`:848`, `:850`) — **that wording is superseded by the authoritative round-2 corrections block (`design:38`)** and confirmed live this session: `InfantryRuntime { fear_level, is_prone }` (`game_entity.rs:48`, hashed) + `tick_fear_decay_and_prone` (`infantry.rs:101`) + `tick_fear_for_entities` (`infantry.rs:130`) exist and are authoritative today. **L4 is a parity-correction + relocation of a LIVE hashed system, not a from-scratch build.** Every task is written on that basis.

#### 2. Goal

Bring infantry fear-decay / prone-stance / crawl-fire sequence selection to exact gamemd parity (verify against `Fear_Decay_Handler 0x005200B0` and `InfantryClass::AI 0x0051BAB0`), then relocate the corrected `tick_fear_for_entities` from the global between-deploy-and-combat sweep into the per-object `infantry_ai` shell step at the verified within-AI position (**after `FootClass::AI` + `Mission_Capture`, before `Fire_At_Target`** — design `:846`). Close the verified parity gaps (mission-type Down/Up exclusion, Fraidycat scatter-flee, panic countdown) and add NAMED acceptance tests pinning the threshold transitions, the mission exclusions, and the within-AI order.

**Out of scope (do NOT implement):** tube/subterranean infantry sub-AI (`+0x684`/`g_TubeArray`, TS-legacy DORMANT — `design:846`, `:933`); fog darkening; `SequenceKind::Panic` as a fear-driven sequence (the gamemd Down/Up path never reaches `animation.rs:92` Panic).

#### 3. Approach choice (brainstorm step)

**Chosen: (B) correct-then-relocate.** Close the parity value-gaps first while `tick_fear_for_entities` still runs as today's global sweep over `keys_sorted()` (`infantry.rs:135`), each as an isolated shadow-or-rebaselined change; THEN flip iteration to per-object live LOGIC order under the shell as a separate hash-affecting step.

Rejected **(A) relocate-then-correct** because the relocation is the single largest blast-radius change — it switches fear-decay iteration from BTreeMap id-ascending to live LOGIC order AND interleaves fear between `Mission_Capture` and `Fire` per-object. Doing corrections first lets each value fix land as an attributable golden rebaseline ("did the threshold fix move the hash" stays separable from "did the iteration-order change move the hash"). The cost — two golden rebaselines instead of one — is the honest price of attributable determinism. (B) also satisfies shadow-first cleanly: the relocation lands shadowed (Task 6a) before authority flips (Task 6b).

#### 4. Files / surfaces (re-verified live this session)

| Surface | File:line | Role |
|---|---|---|
| `tick_fear_for_entities` | `src/sim/infantry.rs:130` | Global sweep being corrected + relocated. Today `(entities: &mut EntityStore, rules, interner)`, iterates `keys_sorted()` (`:135`). |
| `tick_fear_decay_and_prone` | `src/sim/infantry.rs:101` | Core decay+prone body; receives the mission-exclusion + Fraidycat corrections. Today guards only `entity.dying \|\| entity.deploy_state.is_some()` (`:114`). |
| `can_decay_fear` | `src/sim/infantry.rs:35` | `!obj.fearless` — verify vs gamemd `type+0xebc` gate (Task 1). |
| Fear constants | `src/sim/infantry.rs:12-19` | `PRONE_THRESHOLD=50`, `MAX_FEAR=300`, etc. Add `FRAIDYCAT_SCATTER_THRESHOLD=51` only if 3a chosen. |
| Existing fear tests | `src/sim/infantry.rs:185-408` | Already pin 50/51 + Fraidycat-blocks-Down + fearless decay-gate. New tests append here. |
| `InfantryRuntime { fear_level, is_prone }` | `src/sim/game_entity.rs:48` | Authoritative, `Serialize/Deserialize`, **hashed** (no serde-skip). Add panic-countdown here ONLY if gap 4 modeled, as `#[serde(skip)]` shadow first. |
| Fear call site | `src/sim/world/mod.rs:1996` | Current global call (between deploy `:1992` and combat `:2027`). Relocated in Task 6. **FACTS/corrections-block say `:1960` — STALE; live tree is `:1996`.** |
| `tick_deploy_state` | `src/sim/world/mod.rs:1992` | Ordering anchor: fear runs after deploy state. |
| `tick_combat_with_fog` | `src/sim/world/mod.rs:2027` | Consumes the prone bit; fear MUST commit `is_prone` before this. |
| Animation prone cascade | `src/sim/animation.rs:455-495` | Reads `entity.infantry.is_prone` to pick Crawl/Prone/fire; guards in-progress Down/Up at `:461,:477,:483,:490`. `switch_to` `:204`, `sequence_is_prone` `:217`. Unchanged by this slice. |
| Deferred-death API | `World::uninit` `src/sim/world/mod.rs:1010`; `despawn_entity :1050`; `flush_pending_delete :1060` | Sequencer self-Destroy routes here — NOT `entities.remove`. |
| **Shell scheduler (NET-NEW)** | `src/sim/world/techno_ai.rs` **— DOES NOT EXIST YET** | Verified: Glob found no such file; Grep for `object_ai_stage\|infantry_ai\|techno_ai` across `src/sim` = **zero matches**. Created by S0/S1. Design concept at `§7.2:549`, `§7.3:575`. Task 6 is BLOCKED until this exists. |
| gamemd ground truth | `Fear_Decay_Handler 0x005200B0`, `InfantryClass::AI 0x0051BAB0` | Re-confirm via `decompile_function` before pinning new tests. |

**Before writing code, the implementer MUST Read:** `src/sim/infantry.rs` (full), `src/sim/game_entity.rs:46-60`, `src/sim/animation.rs:450-500`, `src/sim/world/mod.rs:1010-1068` + `:1985-2030`, and re-confirm whether S0/S1's shell (`object_ai_stage`/`infantry_ai`) has landed (Glob `src/sim/world/techno_ai.rs`; Grep `infantry_ai`). Do not trust any `file:line` here without re-reading.

#### 5. Step-by-step tasks

**Task 1 — Re-confirm gamemd thresholds + locate the "199" source (RESEARCH; blocks the value tests).**
The FACTS/corrections block lists "49/50/199." The ground-stage decompile of `Fear_Decay_Handler 0x005200B0` found only **49/50** (`0x31`/`0x32` boundaries); **"199" does NOT appear in the decay handler.** Before any test pins 199:
- `decompile_function 0x005200B0` — re-confirm: Down requires `0x31 < fear` (≥50, i.e. fear>49); Up requires `fear < 0x32` (<50); Fraidycat scatter `LAB_005201dc` requires `fear > 0x32` (≥51).
- Locate "199" elsewhere: Grep Rust for `199`; check `apply_fear_from_damage` (`infantry.rs:48`) / a condition-color threshold path. Do NOT pin a 199 test until its source function is decompiled and cited.
- Confirm decay-gate flag: gamemd `type+0xebc` vs Rust `!obj.fearless` (`can_decay_fear :35`). If `+0xebc` is a distinct flag from the `Fearless` INI bit, `can_decay_fear` is DRIFT.

No code. Output: a verdict block (49/50 CONFIRMED / 199-source-found-or-UNVERIFIED / decay-gate-flag mapping), each cited inline with its `decompile_function` call.

**Task 2 — Current-SEQUENCE Down/Up exclusion (parity gap, DRIFT today). ⚠️ CORRECTED 2026-06-02 — prior framing was a misread.**
**CORRECTION (binary-verified):** gamemd does NOT gate this on the mission. `InfantryClass__Fear_Decay_Handler 0x005200B0` suppresses both Down (`Do_Action(5)`) and Up (`Do_Action(7)`) while the infantry's **current animation sequence (`Doing`, InfantryClass +0x6C4) ∈ {0x1B,0x1C,0x1D,0x1E}** (27–30 — deploy/special sequences; exact names TBD). It is read from +0x6C4 (the sequencer's `Doing` index into the per-type `SequenceData` at `TypeData+0xe3c`), **NOT** CurrentMission (+0xAC) and **NOT** the type index. BOTH the original "Capture/Sabotage family" AND the prior "interrupt-mission set {ParadropOverfly(27)…}" labels are WRONG. Verified `decompile_function 0x005200B0 / 0x00520AE0 / 0x00521B60 / 0x00517A50 / 0x00517CC0` — see [docs/research/READYTOCOMMENCE_S5_BLOCKER_CLOSURE_AND_FEAR_SEQUENCE_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/READYTOCOMMENCE_S5_BLOCKER_CLOSURE_AND_FEAR_SEQUENCE_GATE_GHIDRA_REPORT.md). The same {27-30} sequence set is also enforced inside `InfantryClass__DoType_Sequencer 0x00520AE0`.

Rust `tick_fear_decay_and_prone` has no current-sequence guard. **HARD BLOCKER:** the Rust engine does not model an infantry `Doing`/DoType sequence enum at gamemd parity, and sequences 27-30 are not yet named — so this gap CANNOT be implemented faithfully until the infantry `Doing` enum (and which entries are 27-30) is decoded (deferred follow-up: dump `TypeData+0xe3c` entries 27-30 or locate the DoType enum). Decay still runs (gamemd decrements regardless); only the Down/Up *transition* is gated. **Do NOT add a CurrentMission guard — that is the misread this correction fixes.** Signature (this gap) takes the current sequence, NOT the mission:
```
// current_seq = the infantry's Doing/DoType index (gamemd InfantryClass +0x6C4); gate Down/Up while it is in 27..=30
pub fn tick_fear_decay_and_prone(obj: &ObjectType, entity: &mut GameEntity, current_seq: /* Doing/DoType enum — TBD */ u32) -> Option<SequenceKind>
```

**Task 3 — Fraidycat scatter-flee (parity gap, DRIFT today).**
gamemd `LAB_005201dc`: Fraidycat AND `fear > 0x32` (≥51) AND mission-exclusions clear AND a busy-byte zero → `Do_Mission` with a scatter target (the unit FLEES, does not go prone). Rust today just blocks Down for Fraidycat (`infantry.rs:118`), never models the flee.
- **(3a) Model now** — emit a scatter intent. Requires a scatter verb; the scatter system (`scatter.rs:71`) is **DISABLED** (commented at `world/mod.rs:2235-2243`). Re-introduces a dormant dependency — high coupling. Also risks a phantom RNG draw (Task 7).
- **(3b) Defer with explicit DRIFT marker** — keep Fraidycat-blocks-Down, add a `// DRIFT: Fraidycat scatter-flee (Do_Mission) unmodeled; gamemd LAB_005201dc fear>0x32` comment + a test pinning the *current* behavior.

**Recommend (3b)** — couples to the disabled scatter system; closing it belongs with the scatter re-enable, not here. Surface as DRIFT (CLAUDE.md burden-of-proof), do not silently treat as done. Per `feedback_design_approval_ask`, ASK the user 3a vs 3b before picking.

**Task 4 — Panic countdown `+0xbf` on fear-hits-zero (parity gap, DRIFT today).**
gamemd: when fear decays to 0 AND `param_1[0xbf]==0`, a vtable `+0x2ac` predicate is consulted; if true, `param_1[0xbf] = type+0x684` (a panic/scatter duration seed). Same coupling as Task 3 (dormant scatter; `+0x2ac`/`type+0x684` untraced this session). **Recommend defer with UNCHECKED marker** unless the user wants the scatter system re-enabled here. If modeled: add `panic_countdown: u16` to `InfantryRuntime` (`game_entity.rs:48`) as **`#[serde(skip)]` shadow first** (do not hash until authority flip), and decode `+0x2ac`/`type+0x684` before pinning a test. Do NOT pin the design's "panic scatter above threshold" assertion (`design:858`) until the seed and predicate are decompiled.

**Task 5 — Sequencer self-Destroy via deferred delete (signature change; SCOPE-CONFIRM).**
The `DoType_Sequencer` death-completion self-Destroy is the **tail of `InfantryClass::AI`, after Fire** — separate from `Fear_Decay` (Fear_Decay itself never self-destroys; design `:846`, `:602`). **Fear-decay does NOT need `uninit`.** If L4 folds the death-sequence completion into the same infantry shell step (S6 scope names "DoType_Sequencer self-Destroy" — `design:846`, and S6 acceptance test `infantry_self_removal_enqueues_pending_delete` `:856`), that path MUST route through deferred death.
- `tick_fear_for_entities` today takes `&mut EntityStore` and **cannot reach `World::uninit`**. To fold the sequencer path in, the shell step must be World-level. Proposed: `fn infantry_fear_and_sequence_step(&mut self, id: u64, rules: &RuleSet)`. Fear/prone mutates `entity.infantry`+`entity.animation` (as today); the death-sequence completion calls `self.uninit(id)` (`world/mod.rs:1010`) → enqueue `pending_delete` (`:1045`) → deferred free by `flush_pending_delete` (`:1060`). **Never** `entities.remove` directly; the one-tick Dying window is preserved.
- If the sequencer death path is OUT of L4 scope (fear/prone only, death-completion a sibling task), keep `&mut EntityStore`. **CONFIRM with the user** which the slice covers — S6 bundles both; default IN unless told otherwise.

**Task 6 — Relocate into the InfantryClass shell (hash-affecting flip; SHADOW FIRST). LANDS LAST.**
**6a (SHADOW):** introduce the `infantry_ai` shell step (or extend the S0/S1 emerging shell) calling the corrected fear/sequence step at the verified position — **after `FootClass::AI` + `Mission_Capture`, before `Fire_At_Target`** (design `:846`, `:602`). Keep the global `world/mod.rs:1996` call live and authoritative. In the shell, compute the would-be fear result and `debug_assert!` it equals what the still-live global sweep produced for the same entity that tick. Serde-skip any shell scratch; not hashed. Full-skirmish replay → zero `debug_assert` divergence (value parity; only iteration ORDER differs, which 6a does not commit).
**6b (FLIP):** remove the global `world/mod.rs:1996` call; the shell step becomes authoritative. **This changes fear-decay iteration from BTreeMap id-ascending (`keys_sorted()`, `infantry.rs:135`) to live LOGIC order** (active-vector order, design `§7.2:560`) — hash-affecting. Bump `SNAPSHOT_VERSION`, rebaseline the golden, cite per-object fear at `InfantryClass::AI 0x0051BAB0`.
**Ordering invariants preserved (invariant 3):** fear still commits `is_prone` BEFORE combat reads it (combat `:2027`; the infantry shell step runs in the AI stage, which per the design phase map precedes Phase-5 combat — confirm the shell stage sits before combat at implementation). Fear still runs after deploy-state effects. advance_tick is NOT otherwise reordered — only the fear call's home moves from a standalone global sweep into the per-object AI shell at its verified within-AI slot.

**Task 7 — Named acceptance tests (§7).** Append value-correction tests to `infantry.rs:185`. The within-AI order + relocation-shadow tests live in the shell's test module (Grep for the S0/S1 shell test location first — it does not exist yet).

#### 6. What becomes authoritative / what is shadow

| State | Today | After L4 |
|---|---|---|
| `fear_level`, `is_prone` | Authoritative + hashed | Authoritative + hashed (value corrections rebaseline the golden; NO shadow phase for the *values* — a shadow over already-live values is meaningless) |
| Fear-decay **iteration order** | Global sweep, BTreeMap id-ascending (`world/mod.rs:1996`) | **Shadow first** (6a, `debug_assert`-agreed) → **authoritative** (6b, per-object live LOGIC order). The ONE genuine shadow→flip in this slice |
| Mission-type Down/Up exclusion | Absent (DRIFT) | Authoritative (value correction; golden rebaseline) |
| Fraidycat scatter-flee | Blocks-Down only (DRIFT) | DRIFT marker + current-behavior test (rec. 3b); or shadow→auth if 3a |
| Panic countdown `+0xbf` | Unmodeled (DRIFT) | DRIFT marker (rec. defer); or `#[serde(skip)]` shadow if modeled |
| Sequencer self-Destroy | Not in fear path | Routes through `World::uninit` → `pending_delete` (invariant 6) — IF in scope (Task 5) |

**Shadow-first compliance (invariant 4):** the value corrections (Tasks 2–4) modify an already-authoritative, already-hashed system — a direct authoritative change with a clean attributable golden rebaseline, NOT a shadow→flip (a shadow over live values is meaningless). L4's shadow-first obligation attaches to the **iteration-order relocation** (Task 6), which IS landed shadowed (6a) before authority flips (6b).

#### 7. NAMED acceptance tests (exact fn names)

Doc-mandated (`design:855`, `:858`):
1. **`infantry_fear_decay_thresholds`** — fear=50→Down (≥50, i.e. fear>49); fear=49 prone→Up (fear<50); fear=49 standing→None; decay decrements every tick regardless of mission. **NO "199" assertion and NO "panic scatter above threshold" assertion** until Task 1/Task 4 locate and cite their sources.
2. **`infantry_ai_order_capture_fear_fire_sequencer`** — within-AI order reproduced: FootClass::AI → Mission_Capture → fear decay → Fire. Pins Task 6; lives in the shell test module (net-new).

Value-correction tests (one per gap):
3. **`fear_prone_suppressed_during_locked_sequence`** (⚠️ renamed — was `_during_interrupt_missions`; the gate is the current `Doing` sequence, NOT a mission) — fear≥50 while the current sequence ∈ {27,28,29,30} does NOT go prone; on a non-locked sequence it DOES. **Blocked** until the infantry `Doing` enum is modeled (Task 2 correction).
4. **`fear_up_suppressed_during_locked_sequence`** (⚠️ renamed) — prone + fear<50 while the current sequence ∈ {27,28,29,30} does NOT stand; on a non-locked sequence it does. **Blocked** as above.
5. **`fraidycat_no_prone_keeps_current_behavior`** (3b) — Fraidycat at fear≥51 does not go prone (current behavior pinned), marked with the DRIFT comment. (Rename to `fraidycat_scatter_flee_above_threshold` if 3a chosen.)
6. **`fear_decay_decrements_regardless_of_mission`** — decay runs even on excluded missions; only the transition is gated.

Sequencer death (if Task 5 in scope) — reuses the S6 doc-mandated name:
7. **`infantry_sequencer_self_destroy_enqueues_pending_delete`** (aligns with S6 `infantry_self_removal_enqueues_pending_delete` `:856`) — death-sequence completion calls `uninit` → `pending_delete`; synchronous conceal/unmark; slot freed only by `flush_pending_delete`; one-tick Dying window holds; no synchronous `entities.remove`.

Relocation shadow (Task 6a):
8. **`fear_shell_relocation_shadow_zero_divergence`** — full-skirmish replay: per-object shell computation `debug_assert`-agrees with the live global sweep before the flip.

Existing tests that MUST stay green: `decay_thresholds_and_fearless_decay_gate`, `fraidycat_rejects_fear_driven_down`, `crawls_gate_only_blocks_down_not_recovery`, `first_hit_and_fraidycat_set_fear`, `repeated_hit_adds_by_health_and_clamps`, `fearless_type_and_abilities_block_application`, `prone_speed_rounding_is_exact`, `object_category_import_keeps_rules_fixture_infantry`.

#### 8. Determinism / hash notes

- **Value corrections (Tasks 2–4)** change hashed `fear_level`/`is_prone` on affected entities → **golden rebaseline + `SNAPSHOT_VERSION` bump**, justified by `Fear_Decay_Handler 0x005200B0`. One cause per rebaseline (separate commits — approach B's point).
- **Relocation flip (6b)** changes iteration from BTreeMap id-ascending (`infantry.rs:135`) to live LOGIC order (`design §7.2:560`). Same per-entity values, different commit order → **hash-affecting → separate `SNAPSHOT_VERSION` bump + golden rebaseline**. 6a must prove value-parity (zero `debug_assert` divergence) first, so the only hash delta at 6b is attributable to iteration order alone.
- **RNG (invariant 7):** fear decay/prone consumes NO RNG (the gamemd handler's only RNG-adjacent draws are in the scatter/`Do_Mission` paths). The recommended deferrals (3b, defer-4) avoid RNG entirely. **If 3a is chosen, the scatter `Do_Mission` draw must land at the verified per-object gate or it is a phantom-draw desync — defer rather than insert a draw with no verified position.**
- **Frame-anchored timers (invariant 5):** fear decay is a per-tick `-= 1` on a counter (`infantry.rs:112`), NOT a `MissionTimer`. This is the one gamemd mechanic that genuinely decrements per-tick; faithful, not a violation. The never-decrement rule governs `MissionTimer` (start_frame+duration), which fear does not use. **Do NOT refactor fear onto MissionTimer.**
- **Deferred death (invariant 6):** sequencer self-Destroy routes through `World::uninit` (`:1010`) → `pending_delete` (`:1045`) → `flush_pending_delete` (`:1060`); synchronous conceal/unmark, deferred free, one-tick Dying window. Never `entities.remove` mid-AI.
- **No class tree (invariant 2):** the mission-exclusion guard and shell dispatch use `match category` / a `MissionType` enum read — no `dyn`/vtable/trait tree.

#### 9. Dependencies + risk + do-not-do

**Dependencies (sequencing):**
- **Shell scheduler (S0/S1) — HARD BLOCKER for Task 6.** `src/sim/world/techno_ai.rs` and `object_ai_stage`/`infantry_ai` **do not exist in the tree** (verified: Glob + Grep both empty). If S0/S1 has not landed, Task 6 cannot relocate into a shell that does not exist. **Tasks 1–5 (value corrections, no relocation) CAN proceed independently against the current global sweep at `world/mod.rs:1996`; Task 6 blocks on S0/S1.**
- **MissionCom authority (S5) — for Task 2.** The mission-exclusion guard needs the current mission id. If `MissionCom` is authoritative (S5 flipped), read `entity.mission.current`; if still shadow, read the live mission machine's selector. S6/L4 depends on S5 (`design:910`, slice table). The guard works against either source — bind to the authoritative one at implementation time.
- **Infantry `Doing`/DoType sequence enum (NOT `MissionType`)** — Task 2's 27-30 gate is on the current animation sequence (`Doing`, InfantryClass +0x6C4), so it needs the infantry `Doing` enum, which the Rust engine does not yet model. The earlier "`MissionType` 27-30" dependency was the misread; mission codes are irrelevant to this gate. Decode the `Doing` enum (and entries 27-30) before Task 2.

**Risk:**
- **Relocation flip (6b)** is the highest-risk step — iteration-order change interleaves fear between Mission_Capture and Fire per-object. Mitigated by 6a proving value-parity first.
- **Fraidycat scatter (3a) / panic countdown (4)** couple to the disabled scatter system (`scatter.rs:71`, commented at `world/mod.rs:2235-2243`) and untraced fields (`+0x2ac`, `type+0x684`, `+0xbf`). Recommended deferral keeps L4 decoupled; surface as DRIFT.
- **"199" threshold** is UNVERIFIED in the decay handler — pinning a test before locating its source would bake in an invented fact (CLAUDE.md verification discipline).

**Do NOT do:**
- Do NOT implement the tube/subterranean infantry sub-AI (`+0x684`/`g_TubeArray`, TS-legacy DORMANT — `design:933`). Assert-and-skip only (S6 test `infantry_tube_branch_is_noop_ts_legacy` `:857` covers this — note it belongs to the broader shell slice, not strictly L4's fear scope).
- Do NOT emit `SequenceKind::Panic` from the fear path — the gamemd Down/Up path never reaches `animation.rs:92`.
- Do NOT refactor fear-decay onto `MissionTimer` — fear genuinely decrements per-tick (§8).
- Do NOT pin a 199 test or a "panic scatter above threshold" test until Task 1/Task 4 decompile and cite the source.
- Do NOT bundle value corrections and the relocation into one commit/golden (approach B's whole point — attributable hash deltas).
- Do NOT call `entities.remove` for the sequencer death; route through `World::uninit`.
- Do NOT trust the FACTS/corrections-block `world/mod.rs:1960` call-site line — the live tree is **`:1996`** (verified this session).

**Relevant files (absolute):** `src/sim/infantry.rs`, `...\src\sim\game_entity.rs`, `...\src\sim\animation.rs`, `...\src\sim\world\mod.rs`, `...\src\sim\mission\mod.rs`. Shell file `...\src\sim\world\techno_ai.rs` **does not exist yet** (net-new from S0/S1). Design doc: `...\docs\research\TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md` (S6 `:844`, corrections block `:9`/`:38`, shell `§7.2-7.3:549-602`, slice table `:910`).

---

### Slice L6 — Commence Gate Structure + MissionCom Authority Flip (mission/radio Slice 6)

> **Review notes — what I corrected in the draft (defaulting to skeptical, every claim re-checked against the live tree + design doc §7.4/§8/Slice-S5):**
>
> 1. **Removed the fabricated "A1 lane report."** The draft's entire premise — "A1 traced every busy/ready byte, upgraded INFERRED→VERIFIED" with a verdict table and specific Ghidra symbols (`FUN_004a51d0`, `DAT_007eaf7c`/`0x007eaf7c` DoType table, traced setters for `+0x6DD`/`+0x6D2`/`+0x6D4`/`+0x6E1`/`+0x6E2`/`+0x6D1`/`+0x68D`/`+0x8D`) — is **not in the VERIFIED FACTS block and is directly contradicted by the design doc**. Design doc lines 689–691, 811, 826, 650 all state these bytes are **INFERRED from constructor init, DRIFT until each setter is traced**, and the locomotor `+0x80` idle body is **UNCHECKED**. I struck all invented addresses and the field-accurate predicate; the slice lands the **hook structure** only, with every busy byte an `UNCHECKED` stub. Per CLAUDE.md verification discipline, no offset/address/DoType-table goes into the plan that wasn't read from the binary this cycle.
> 2. **Deferred the retaliation-gate retarget (draft Step 6) out of L6.** Two independent blockers: (a) design §8 **row 6 verdict is KEEP-AS-GLOBAL-SERVICE — "only state representation + teardown call sites change,"** not the gate predicate; (b) `derived_mission()` (`game_entity.rs:482`) reads only `miner`/`aircraft_mission`/`dock_state`/`attack_target`/`movement_target` — it does **NOT** read `order_intent`, so a guarding unit (`order_intent = Guard`, no attack/move target) has `mission.current = None`. There is **no MissionCom field today that reproduces the Guard suppression**, so the draft's "MissionCom-derived gate that still suppresses the guard" is impossible without first adding a Guard-capturing derivation — itself out of scope and UNCHECKED. The tripwire test `slice6_retaliation_still_suppressed_for_guarding_unit` (`slice6_retask_tests.rs:204`) would go RED. **The literal `order_intent` gate stays byte-identical**; retargeting is a later slice gated on a Guard-mission derivation.
> 3. **Corrected the "live commence consumer" claim (draft Step 4).** Grep confirms `queue_mission`/`commence_queued` have **zero live callers** — they are test-only. There is no "dock-reserve `Queue_Mission(Harvest/Enter)` path" wired to the verbs today; all 9 retask sites use `assign_mission_with_teardown`/`assign_mission_keep_fields` (`world_commands.rs:148,290,331,360,386,422,794,893,1047,1150`). L6 adds the `commence` parameter + gate to the verb and pins it with the mandated unit test; it does **not** invent a new live wiring (and the design doesn't require one this slice).
> 4. **Golden excludes busy-flag-dependent assertions** (design line 840 `s5_mission_authority_flip_golden_rebaselined`). The only hash-moving change is the MissionCom fold, whose hashed fields (`current`/`substate`) derive from the *legacy* machines (already-hashed executors) — so the rebaseline cause is purely "MissionCom selector + timer entered the hash," never an unverified busy byte.
> 5. **Line-number / shape fixes:** `ReadySnapshot` today is `{ category, is_driving }` only (`verb.rs:153-159`); `queue_mission` today is `(com, mission)` with no `commence`/`snap` (`:72`); the override-with-queued test is at `:257` (draft said `:258`); `assign_mission_keep_fields` writes `e.mission.current` directly with **no timer reset** (`retask.rs:97`) — noted so the flip doesn't accidentally start re-arming it. The doc-comment references a `mission_shadow_does_not_change_state_hash` test that does not exist by that name; shadow-neutrality is actually carried by `replay_hash_stable_through_slice6` — do not cite a phantom test.
> 6. **Approach (B) retained** (isolate the one hash boundary) — that part of the draft was sound.

---

#### 1. Approach (kept from draft, narrowed)

Execute as **one slice, two sequenced sub-steps with the single hash-moving change isolated**:
- **Sub-step A (hash-neutral):** land the `commence` parameter + `ready_to_commence` **structure** gate in the verbs; keep MissionCom shadow. No hash movement (verbs already write MissionCom in parallel; MissionCom still unhashed). Prove with `replay_hash_stable_through_slice6` staying at the **current** `SLICE6_BASELINE_HASH = 17281687802996982350` (unchanged).
- **Sub-step B (one hash boundary):** flip MissionCom to authoritative — fold the selector + timer into `world_hash`, invert `refresh_mission_shadow` into a standing cross-check, bump `SNAPSHOT_VERSION 16 → 17`, rebaseline the golden with the single cited cause.

Rationale unchanged from draft: exactly one golden rebaseline, one citable cause, the commence-gate behavior change lands behind its own named test rather than folded into the rebaseline. **Retaliation is untouched** (correction 2).

#### 2. Goal (corrected scope)

1. Add the `commence: bool` path to `queue_mission`; gate the promotion on `ready_to_commence(&ReadySnapshot)`, implemented as a real `match category` predicate (base `true`, 4 leaf arms) — **structure only**. Unit arm uses the existing `is_driving` (the one input the substrate already expresses); **every other busy byte is a `// UNCHECKED` stub returning the base verdict**, not a hardcoded flag.
2. Keep `assign_mission` as the ungated force-promote; keep `override`/`restore` as-is (already the correct single-depth queued-priority stack — `verb.rs:100/117`, tests `:246/:257`).
3. Flip MissionCom from shadow to authoritative for the **selector** (`current`/`substate`/`queued`/`suspended`/`timer`): fold into `world_hash`, invert `refresh_mission_shadow` to a cross-check, bump `SNAPSHOT_VERSION`. The legacy `Option<T>` machines stay the **executors** and become the cross-check source.
4. **Do NOT** retarget the retaliation gate. **Do NOT** make any busy byte field-accurate. **Do NOT** add a live `queue_mission(commence=true)` call site.

All 8 invariants honored — in particular: no `dyn`/vtable (`match category`); `advance_tick` phase order untouched (retaliation stays the Phase-6 global at its current site `:2261/:2262`, gate predicate unchanged); timers frame-anchored (`timer.reset(now)` = `defer(now,0)`); RNG-neutral (verbs pure).

#### 3. Files / surfaces (verified `file:line`, this session)

| File:line | Anchor | Role |
|---|---|---|
| `mission/verb.rs:153-159` | `struct ReadySnapshot { category, is_driving }` | MODIFY — add UNCHECKED busy-byte stubs + `current`/`queued`/`mission_state` only as needed by structure |
| `mission/verb.rs:166` | `ready_to_commence` | MODIFY — `match category`, base true, Unit uses `is_driving`, rest UNCHECKED stub |
| `mission/verb.rs:72` | `queue_mission(com, mission)` | MODIFY — add `snap: &ReadySnapshot, commence: bool, now: u32`; consult gate |
| `mission/verb.rs:83` | `commence_queued(com, now)` | KEEP body (pure "promote if queued"); the gate is applied by the caller |
| `mission/verb.rs:61` | `assign_mission` | KEEP — ungated force-promote |
| `mission/verb.rs:100/117` | `override_mission`/`restore_mission` | KEEP — already correct |
| `mission/retask.rs:72/89` | `assign_mission_with_teardown` / `assign_mission_keep_fields` | KEEP force/ungated (player-command contract) |
| `mission/mod.rs:188` | `struct MissionCom` | READ-ONLY anchor |
| `game_entity.rs:455-456` | `#[serde(default)] pub mission: MissionCom` | the field being flipped to authoritative |
| `game_entity.rs:482` | `derived_mission()` | KEEP — becomes the cross-check producer |
| `world/mod.rs:895` | `refresh_mission_shadow()` | MODIFY — invert: stop writing, derive→cross-check |
| `world/mod.rs:908` | `debug_assert_mission_shadow_consistent()` | KEEP — becomes the standing cross-check |
| `world/mod.rs:1220-1222` | inline `derived_mission()` on load | KEEP as pre-flip reconstruction; gate on `version < 17` |
| `world/mod.rs:2391/2393/2394` | refresh call → assert → `state_hash` | KEEP sites (phase order #3) |
| `world/world_hash.rs:486-492` | dock block in entity loop | ANCHOR — add MissionCom fold immediately after |
| `combat/combat_targeting.rs:352` | `attack_target.is_some() \|\| order_intent.is_some()` | **UNCHANGED this slice** (see correction 2) |
| `world/slice6_retask_tests.rs:70` | `SLICE6_BASELINE_HASH` | REBASELINE in sub-step B only (cited) |
| `world/slice6_retask_tests.rs:204` | retaliation tripwire | MUST stay GREEN (gate unchanged) |
| `snapshot.rs:22` | `SNAPSHOT_VERSION = 16` | BUMP → 17 (sub-step B) |
| `map/entities.rs` | `EntityCategory {Unit,Infantry,Structure,Aircraft}` | READ-ONLY — `match` discriminant (mapped via existing `From<EntityCategory> for ReadyCategory` `verb.rs:139`) |

#### 4. Step-by-step

##### Sub-step A — gate structure (hash-neutral)

**A1. Extend `ReadySnapshot` with UNCHECKED stubs (no invented offsets).**
`verb.rs:153`. Add only fields the *structure* needs plus honest UNCHECKED placeholders. Each busy byte is a `bool` whose doc-comment says `// UNCHECKED: leaf ReadyToCommence busy byte; INFERRED from ctor init, setter not traced — DRIFT, defaults to not-busy`. Do **not** name a binary offset in the Rust comment (`feedback_no_engine_refs_in_comments`); reference "the leaf commence busy flag" in prose.

```rust
pub struct ReadySnapshot {
    pub category: ReadyCategory,
    pub current: MissionType,        // for the structural excluded-mission check
    pub queued: Option<MissionType>, // Unit Move-with-queued nuance (structural)
    pub is_driving: bool,            // locomotor not-idle (the ONE verified-ish input;
                                     // the idle predicate body itself is UNCHECKED)
    // --- UNCHECKED busy-flag stubs: structure present, value not field-accurate.
    //     All default to `false` (not busy) so the gate is never falsely blocked
    //     before the setters are traced (DRIFT, surfaced not hidden). ---
    pub building_busy: bool,         // Building leaf — UNCHECKED, default false
    pub aircraft_busy: bool,         // Aircraft leaf — UNCHECKED, default false
    pub unit_deploy_busy: bool,      // Unit leaf — UNCHECKED, default false
    pub infantry_sequence_busy: bool,// Infantry leaf — UNCHECKED, default false
}
```
No DoType table, no `0x6xx` offsets, no collapse claims — none of that is verified.

**A2. `ready_to_commence` structure.**
`verb.rs:166`. Real `match category`, base `true`, the four leaves wired to their stub (which is base-true today) plus the *one* honest constraint we already had — Unit not-ready while driving. Excluded-mission constants pulled from `MissionType` (verified discriminants in `mod.rs:30-76`: `Sticky=6`, `Rescue=21`); the exact exclusion *set* per leaf is UNCHECKED, so apply only the universally-safe ones and comment the rest as pending-trace.

```rust
pub fn ready_to_commence(snap: &ReadySnapshot) -> bool {
    match snap.category {
        // Base predicate is `return 1`; each leaf override is structural until its
        // busy-flag setters are traced (see ReadySnapshot stubs).
        ReadyCategory::Building  => !snap.building_busy,
        ReadyCategory::Aircraft  => !snap.aircraft_busy,
        ReadyCategory::Infantry  => !snap.infantry_sequence_busy,
        ReadyCategory::Unit      => !snap.is_driving && !snap.unit_deploy_busy,
    }
}
```
Because every `*_busy` stub is `false` and `is_driving` is the only live input, this is **behavior-identical to today's** `ready_to_commence` (`Unit => !is_driving`, rest true) — so it is hash-neutral and the existing test `slice6_ready_to_commence_base_true_unit_not_while_driving` (`:307`) still passes verbatim. The new structure is what later slices fill in once setters are traced.

**A3. Add the `commence` path to `queue_mission`.**
`verb.rs:72`. Keep `commence_queued` (`:83`) a pure "promote if queued" — the gate is the caller's responsibility. Signature change is additive:

```rust
pub fn queue_mission(
    com: &mut MissionCom, snap: &ReadySnapshot,
    mission: MissionType, commence: bool, now: u32,
) -> bool {
    if is_transition_blocked(com.current, mission) { return false; }
    com.queued = Some(mission);
    if commence && ready_to_commence(snap) {
        return commence_queued(com, now);
    }
    true
}
```
Update the two existing call sites in `verb.rs` tests (`:236`, `:241`) to pass a snapshot + `commence=false`. `assign_mission` (`:61`) unchanged.

##### Sub-step B — authority flip (one hash boundary)

**B1. Invert `refresh_mission_shadow` → cross-check.**
`world/mod.rs:895`. Today it overwrites `current`/`substate` from `derived_mission()` (`:897-900`). After the flip the verbs own `current`/`substate`/`timer`; this function stops writing them. Two options, pick at implementation time:
- Drop the writer body and rely on `debug_assert_mission_shadow_consistent` (`:908`) at `:2393` as the standing cross-check; OR
- Keep it as an explicit `#[cfg(debug_assertions)]` cross-check that logs divergence.

Keep the `tick_counter` increment **only if** it stays out of the hash (see B3). Keep the `:1220` inline re-derive **only** as one-time reconstruction when loading a pre-flip save (`header.version < 17`); post-flip saves carry MissionCom via its existing `#[serde(default)]`.

**Cross-check caveat (load-bearing):** `derived_mission()` does not capture `order_intent`/deploy/guard, and `assign_mission_keep_fields` (`retask.rs:97`) writes `current` without resetting the timer. The cross-check must compare only the dimensions both sides actually agree on (`current`/`substate` for the executor-backed missions). For Guard-via-`order_intent`, the legacy machine and MissionCom diverge by construction (MissionCom would carry Guard if a verb wrote it; `derived_mission` yields None). **Validate the cross-check holds across the `slice6` scripted skirmish before flipping** — if it diverges on any fixture, the flip is not ready; do not suppress the assert.

**B2. Fold MissionCom into the hash.**
`world_hash.rs`, immediately after the dock block (`:492`), fixed field order:
```rust
(entity.mission.current as u16).hash(hasher);
entity.mission.substate.hash(hasher);
match entity.mission.queued    { Some(m) => { 1u8.hash(hasher); (m as u16).hash(hasher); } None => 0u8.hash(hasher) }
match entity.mission.suspended { Some(m) => { 1u8.hash(hasher); (m as u16).hash(hasher); } None => 0u8.hash(hasher) }
entity.mission.timer.start_frame.hash(hasher);
entity.mission.timer.duration.hash(hasher);
```
**Double-count audit (done this session):** `world_hash.rs` has no existing `.mission` fold; the hashed executor fields are `attack_target` (`:469`), `building_gate` (`:498`), dock (`:486`) — none is the mission *selector*, so the fold is net-new, no double-count.

**B3. `tick_counter` decision — OMIT from the hash.** It increments every tick for every entity (`:900`), so hashing it makes the hash advance unconditionally and turns the determinism oracle noisy without adding gameplay state. It is bookkeeping, not a selector. **Recommend OMIT** (documented in the fold comment). Surface explicitly, not a silent drop.

**B4. Bump `SNAPSHOT_VERSION 16 → 17`** (`snapshot.rs:22`) with a comment: "MissionCom selector + timer entered world_hash (Slice 6 authority flip)."

**B5. Rebaseline the golden.** Run `replay_hash_stable_through_slice6` once, read the failing `left:` value, paste into `SLICE6_BASELINE_HASH` (`:70`) with an inline doc-comment: `// Rebaselined at Slice-6 authority flip: MissionCom selector+timer entered world_hash at SNAPSHOT_VERSION 17. Busy-flag-dependent state is NOT in this golden (busy bytes UNCHECKED).` **Confirm the only diff vs version 16 is the fold** by capturing the hash with the fold commented out (must equal `17281687802996982350`), then enabling it.

#### 5. Authoritative vs shadow after L6

| State | Before | After |
|---|---|---|
| `MissionCom.current/substate` | shadow (re-derived each tick `:897`) | **authoritative** (verb-written; `derived_mission` is the cross-check) |
| `MissionCom.queued/suspended/timer` | verb-written, unhashed | **authoritative + hashed** |
| `MissionCom.tick_counter` | incremented, unhashed | unchanged — incremented, **still unhashed** (B3) |
| `ready_to_commence` | `Unit => !is_driving`, rest true | same observable verdict; now a 4-arm `match` with UNCHECKED busy stubs (structure for later slices) |
| Retaliation gate | literal `order_intent.is_some()` | **UNCHANGED** (correction 2; design §8 row 6 KEEP) |
| `attack_target`/`movement_target`/`dock_state`/`miner.state`/`aircraft_mission`/`order_intent` | authoritative executors | UNCHANGED — executors; feed the cross-check |
| busy-flag fields / loco `+0x80` body | n/a | **UNCHECKED stubs**, DRIFT, surfaced — fill when setters traced |

#### 6. Named acceptance tests

Sub-step A (must pass with golden **unchanged**):
1. `queue_commence_gated_by_ready_to_commence` (design mandate, `verb.rs` `#[cfg(test)]`) — `queue_mission(commence=true)` to a *driving* Unit (`is_driving=true`) sets `queued`, leaves `current`; `assign_mission` to the same unit promotes `current`. Pins the gate vs force-promote split.
2. `ready_to_commence_base_returns_true_four_leaf_overrides` (design mandate) — base/default true; routed through `match category` (no `dyn`); Building/Unit/Infantry/Aircraft each asserted with busy=false (ready) and the wired input toggled (Unit `is_driving`, others' stub) to prove the arm is reached. **Extend the existing `slice6_ready_to_commence_base_true_unit_not_while_driving` (`:307`)** rather than duplicate.
3. `queue_mission_commence_false_writes_queued_not_current` — `commence=false` sets `queued`, leaves `current` untouched.
4. `override_saves_queued_when_pending_else_current` (design mandate) — **already covered** by `slice6_override_with_queued_discards_current_saves_queued` (`:257`) + `slice6_override_without_queued_saves_current_then_restore` (`:246`); do not duplicate.
5. `commence_rearms_timer_due_next_tick` — after gated `commence_queued`, `timer.start_frame == now && duration == 0`; never a decrement (invariant #5).
6. `replay_hash_stable_through_slice6` (`:73`) — **stays at `17281687802996982350`** through sub-step A (gate is observably no-op today).

Sub-step B (the one rebaseline):
7. `mission_com_authority_cross_check_holds` (integration) — across the `slice6` scripted skirmish, the cross-check (`debug_assert_mission_shadow_consistent`) does not fire post-flip.
8. `mission_com_selector_hashed_after_flip` — `state_hash` responds to a `mission.current` change; stable across save/load round-trip at version 17. (Replaces the draft's `mission_com_hashed_after_flip`; assert the selector+timer specifically, not `tick_counter`.)
9. `slice6_retaliation_still_suppressed_for_guarding_unit` (`:204`) — **MUST stay GREEN** (the gate is unchanged; this is the regression guard that the flip did not accidentally touch retaliation).
10. `replay_hash_stable_through_slice6` (`:73`) — **REBASELINE** with the cited cause (B5). The captured value must be reproducible from "version 16 + fold only."
11. `s5_mission_authority_flip_golden_rebaselined` (design mandate) — assert the rebaselined golden holds AND that no busy-flag input participates (set every busy stub both ways in a fixture and confirm the *golden* hash is identical, proving busy bytes are excluded from hashed state).

#### 7. Determinism / hash notes

- **One hash boundary, one cited cause** (sub-step B only): MissionCom selector + timer enter `world_hash`; `SNAPSHOT_VERSION 16→17` and `SLICE6_BASELINE_HASH` move together with the inline justification.
- **Busy bytes are excluded from hashed state** (design line 840) — the only hashed MissionCom fields derive from already-hashed executors, so the rebaseline cause is unambiguous.
- **Iteration order preserved.** `refresh_mission_shadow`/cross-check and the hash entity loop both walk `entities.values()` = BTreeMap id-ascending.
- **Phase order untouched (#3).** Retaliation stays the Phase-6 global at `:2261/:2262`, gate predicate unchanged. `refresh_mission_shadow`/cross-check stays at `:2391`, pre-`state_hash` (`:2394`).
- **RNG-neutral (#7).** Verbs are pure. The gate change is observably a no-op today (`is_driving`-only), so it cannot change which units enter combat/acquire on any tick — sub-step A is provably hash-neutral, sub-step B's only delta is the fold. **Do not add the `RandomRanged(0,2)` re-arm jitter** — it lands with the dispatch consumer (a sibling slice), no consumer here, phantom desync.
- **Timer frame-anchored (#5).** `commence_queued`/`assign_mission` re-arm via `timer.reset(now)` = `defer(now,0)` (`timer.rs`), `now = self.binary_frame`. Never decrement. Note `assign_mission_keep_fields` (`retask.rs:97`) writes `current` **without** a timer reset — preserve that (do not start re-arming it on the flip).

#### 8. Dependencies, risk, do-not-do

**Depends on:** mission/radio Slices 0–3 (landed); the Slice-6 verb/retask scaffolding (landed `ff1d2a32`). **Blocked-on (open, do NOT resolve by inventing):** leaf `ready_to_commence` busy-flag setters (DRIFT, design open item line 689), locomotor `+0x80` idle body (UNCHECKED, line 690) — these are why the gate ships as structure-with-UNCHECKED-stubs, not field-accurate. A future slice traces them via Ghidra (Building `0x00454250`, Unit `0x00744270`, Infantry `0x00521B60`, Aircraft `0x0041B5E0` per design line 807 — **addresses are the design doc's, to be re-verified live before any field-accuracy slice**) and only then fills the stubs + adds busy-flag-dependent goldens. **This is the Verify A1 precondition: A1 supplies the traced setters; until A1 lands, every busy byte stays an UNCHECKED stub and L6 ships structure-only — A1's absence does NOT block the structural slice.**

**Sibling coordination:** if the bus-authority / MissionControl-consumption slice lands in the same window, coordinate **one** `SNAPSHOT_VERSION` bump.

**Risk:**
- *Low* — sub-step A is observably a no-op (gate already equals today's `!is_driving`), so it cannot drift; the value of A is the *structure* + the gated-promote plumbing.
- *Medium* — sub-step B rebaseline; mis-capturing masks a real drift. Mitigate per B5 (capture with fold commented = version-16 value, then enable).
- *Medium* — the cross-check (B1) must hold across the scripted skirmish before flipping; the `order_intent`-Guard divergence means the cross-check compares only executor-backed dimensions. If it diverges on a fixture, do not flip.
- *Low-but-DRIFT, surfaced* — busy bytes + loco idle body remain UNCHECKED; the gate is structurally correct but not field-accurate (a queue+commence-now to a busy-but-not-driving unit/aircraft/building may promote one tick early vs gamemd). This is the design's known parity risk (line 826) and is explicitly deferred, not hidden.

**Do NOT:**
- Do NOT invent or cite any busy-flag offset, DoType table, or setter address not read from the binary this cycle — keep every busy input an UNCHECKED stub (corrections 1).
- Do NOT retarget the retaliation gate — design §8 row 6 is KEEP, and `derived_mission()` cannot reproduce the Guard suppression; the tripwire test guards this (correction 2).
- Do NOT route player retask commands (`assign_mission_with_teardown`/`keep_fields`) through the commence gate — ungated force-promotes.
- Do NOT add a live `queue_mission(commence=true)` call site — there is no verified consumer this slice (correction 3); the verb is pinned by unit test only.
- Do NOT hash `tick_counter` (B3) — bookkeeping, makes the oracle noisy.
- Do NOT let a busy-flag input participate in the golden (design line 840; test #11 proves exclusion).
- Do NOT delete the legacy `Option<T>` executors — only the *selector authority* flips; they stay and feed the cross-check.

**Files touched (absolute):** `src/sim/mission/verb.rs`, `...\src\sim\mission\retask.rs` (call-site signature updates only if the new `queue_mission` arity ripples — it does not today, since no live caller exists), `...\src\sim\game_entity.rs` (cross-check role on `derived_mission`; load-path version gate), `...\src\sim\world\mod.rs` (`refresh_mission_shadow` inversion), `...\src\sim\world\world_hash.rs` (fold), `...\src\sim\snapshot.rs` (version bump), `...\src\sim\world\slice6_retask_tests.rs` (rebaseline + new integration tests). READ-ONLY anchors: `...\src\map\entities.rs`, `...\src\sim\combat\combat_targeting.rs` (confirm unchanged), `...\src\sim\mission\mod.rs`, `...\src\sim\mission\timer.rs`. Design doc: [docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md) (§7.4, §8 row 6, Slice-S5 lines 801–841).

---

### Slice L7 (= doc Slice S8) — BuildingClass Wrapper (LAST, OUTLINE-LEVEL)

> **Review notes (corrections applied this pass, all re-verified against the live tree):**
> 1. **Combat-driven `uninit` line corrected `:2078` → `:2079`** (`world/mod.rs:2079`, inside the `immediate_uninit_ids` loop). The plan body and its own FACTS-correction footer both said `:2078`; live is `:2079`. Ejection chain `:2136-2150` ✓, SW refresh `:2152-2156` ✓ unchanged.
> 2. **`ready_to_commence` is a free function, not an entity method.** Live signature is `pub fn ready_to_commence(snap: &ReadySnapshot) -> bool` at `mission/verb.rs:166`; the Building/Infantry/Aircraft base-`true` arm is `:171`. Task E reworded — the hook is finalized by extending `ReadySnapshot` + the `match` arm, not by adding a method.
> 3. **`derived_mission` fallthrough is `MissionType::None`, not "idle/guard".** A building with no miner/aircraft/dock/attack/movement state falls through to `(MissionType::None, 0)` at `game_entity.rs:509`. Corrected §3.3 / §5 / prereq #3 wording: buildings currently derive to **None** (not idle/guard); S5 must add the building leg.
> 4. **Dock FIFO field is `production.depot_dock_reservations`** (the `DockReservations` pattern; comment `building_dock.rs:5` confirms "FIFO queuing"), not a standalone `DockReservations` in `building_dock.rs`. §3.4 / §7 reference corrected.
> 5. Re-verified every other `file:line`: `advance_tick` `:1742`, `refresh_mission_shadow` def `:895`/call `:2391`/`state_hash` `:2394`, `uninit` `:1010`, `flush_pending_delete` `:1060` (drains `:1719`/`:1770`/`:2254`), `refresh_fog` def `:1395` called `:1967`, `tick_power_states` `:1973`, `tick_gate_runtimes` `:1803`, `tick_production_with_overlay_registry` `:2280`, `tick_repairs` `:2288`, `tick_building_docks` `:2289`, `tick_building_up/down` `:1507/:1531` called `:1688/:1690`, `live_object_order_snapshot` `:929` (NO sort), `remove_wall_entity_at` `:1365` (bypass at `:1380`), building fields `:206/:261/:263/:433/:456`, `refresh_building_damage_state_gate` `:670`, gate timers are `MissionTimer` (`:100/:107`), `tick_unload_accumulator` `:194` called `:802` after `phase_unloading` `:792`. **No `src/sim/ai/` dir or `techno_ai.rs` exists** — only `src/sim/ai.rs` (computer-player AI) and `src/sim/aircraft/` — confirming the S0–S7 host is genuinely absent. `power_system.rs` exposes only `is_low_power`/`was_low_power` (`:29/:36`); **no EMP surface today** — confirmed.

**Status:** AUTHORED OUTLINE — read-only research complete; no Rust written. This slice is **outline-only by design** (doc §9 S8 :880; §10.2 :930 *"Do NOT start the leaf migration with BuildingClass"* — it is last because of its blast radius into HouseClass/Factory globals). This document sketches how a `building_ai` step would wrap the Techno-common AI step, enumerates prerequisites and blast-radius risks, and names the acceptance tests. It deliberately does **not** produce full tasks or code — that comes only after S0–S7 land and the prerequisites below clear.

**Naming:** the workflow task calls this "Slice L7"; the design doc's slice ledger (doc §9 :912) names it **"Slice S8 — Building leaf shell (LAST)."** Same slice. Cross-reference the doc's acceptance-test names under S8.

**Source of truth:** [docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md) §9 S8 (:880–:897), §10.1–10.3 (:920–:949), plus the live Rust tree (every `file:line` below re-verified this session against the live file).

---

#### 1. Approach choice (brainstorm step)

**Chosen approach: a thin per-building `building_ai` bracket that wraps the existing Techno-common AI step and calls *out* to the already-global power/production/repair/gate phases as service functions — NOT a re-homing of those phases into the bracket.** The obvious-but-wrong alternative is to follow gamemd's literal structure: `BuildingClass::Update` runs all ~26 building phases (power-state transition, ProduceCash, gates, SAM gate, delayed fire, auto-sell, repair, auto-production, bridge destruction, zero-health destruction) *inline, per-building, around* the common AI step (phase 11). Porting that literally would mean pulling `tick_power_states`, `tick_production_*`, `tick_repairs`, `tick_gate_runtimes` out of their current global `advance_tick` phases and invoking them once-per-building inside the bracket. **Rejected** because (a) it violates invariant #3 (reorders/collapses the preserved global phases) and the doc's explicit constraint that house/factory/superweapon services "stay as their current global phases — no collapse" (doc §9 S8 :886, §10.2 :928/:930); (b) those global phases read/write shared HouseClass economy state (power surplus, build cash, factory queues) whose interleave with the existing per-house aggregation would change if driven per-building; (c) the only *observably* per-building-ordered behavior gamemd exposes through the bracket is the **ordering of pre/post phases around the common AI step for one building** plus the **building EMP-restore early-return** — both reproducible by a thin wrapper that reads/writes per-building state in native order while leaving the global service phases where they are. Matches the "translate the behavior contract, not the C++ class tree" rule (CLAUDE.md, doc §1) and keeps blast radius to per-building ordering only.

---

#### 2. Goal

Bring `BuildingClass` under the per-object AI shell established by S0–S7: a per-building `building_ai` shell-step that calls the Techno-common pre/post AI bracket (`techno_common_pre` → `+0xC4` increment → `Mission_Dispatch` → `techno_common_post`, established in S2–S4) and **adds the building-specific pre/post phases around it** — power-state read, occupant/docked-object update, ProduceCash read, gate state, delayed-fire, auto-sell, repair, **zero-health destruction**, and the **building EMP-restore early-return** — *without collapsing* the existing global power/production/repair/gate phases. Buildings (RTTI-6) **skip** the units-only unload accumulator. The slice flips authority on **only the per-building Update-bracket ordering around the common AI step and the EMP-restore early-return**; all HouseClass/Factory/superweapon services remain their current global phases.

---

#### 3. Files / surfaces (exact file:line, live tree)

##### 3.1 Host that must already exist (S0–S7 deliverables — NOT present today)
- `src/sim/ai/` shell tree (`object_ai_stage`, `techno_common.rs`, `leaf_building.rs`, `building_ai` dispatcher) and `src/sim/world/techno_ai.rs` — **do not exist yet.** Confirmed: only `src/sim/ai.rs` (computer-player decision AI, `ai::tick_ai`, called from `run_late_region` Phase 8 `world/mod.rs:1653`) and `src/sim/aircraft/` exist; there is no `src/sim/ai/` directory and no `techno_ai.rs` anywhere in `src`. S8 plugs a `Building` arm into whatever per-object update stage S0–S4 land.

##### 3.2 Building per-tick behavior lives TODAY as global phases (these STAY — the shell calls out to them)
| Concern | Owner (verified this session) | Call site in `advance_tick` |
|---|---|---|
| Power state | `power_system::tick_power_states` (`power_system.rs:140`) | `world/mod.rs:1973` (Phase 4) |
| Gates (`Gate=yes`, mission 0x18) | `gate_runtime::tick_gate_runtimes` (`gate_runtime.rs`) | `world/mod.rs:1803` (Phase 1, after ground movement) |
| Docked-object update (repair depot) | `building_dock::tick_building_docks` (`docking/building_dock.rs:135`) | `world/mod.rs:2289` (Phase 7) |
| Auto-production / ProduceCash | `production::tick_production_with_overlay_registry` (`production/…`) | `world/mod.rs:2280` (Phase 7) |
| Repair + auto-sell | `production::tick_repairs` (`production/…`) | `world/mod.rs:2288` (Phase 7) |
| Build-up / build-down anims | `tick_building_up` (`world/mod.rs:1507`), `tick_building_down` (`world/mod.rs:1531`) | `world/mod.rs:1688`, `:1690` (Phase 9, inside `run_late_region`) |
| Zero-health destruction | combat-driven: `combat::tick_combat_with_fog` sets `structure_destroyed` + `immediate_uninit_ids`; `self.uninit(dead_id)` @ `world/mod.rs:2079`; post-death ejection chain `:2136-2150`; SW refresh `:2152-2156` | Phase 5 (combat block, `:2050-2156`) |

##### 3.3 Per-building state the shell reads (exists today)
- `building_gate: Option<BuildingGateRuntime>` (`game_entity.rs:433`; runtime struct `:91`); transition/hold timers are `MissionTimer` (frame-anchored, `:100`/`:107`; Slice 1, commit 792d6051).
- `building_up: Option<BuildingUp>` (`:261`), `building_down: Option<BuildingDown>` (`:263`).
- `repairing: bool` (`:206`) — drives `tick_repairs`.
- `mission: MissionCom` (`game_entity.rs:456`) — SHADOW (not hashed; refreshed by `refresh_mission_shadow` @ `world/mod.rs:895`, called `:2391`, pre-`state_hash` `:2394`). **`derived_mission` (`game_entity.rs:482`) has NO building leg** — a building with no miner/aircraft/dock/attack/movement state falls through to `(MissionType::None, 0)` at `:509`. S5 must add the building leg before S8.
- `refresh_building_damage_state_gate` (`game_entity.rs:670`) — health→damaged-visual gate the post-phases read.
- `ready_to_commence` Building arm (`mission/verb.rs:171`, inside the free fn at `:166`): **returns base `true`** — the `+0x6DD` building busy-flag is the doc/FACTS UNCHECKED item (§10.3 :944), intentionally not wired.
- Power: `power_system.rs` exposes `is_low_power`/`was_low_power` (`:29`/`:36`); there is **no EMP-restore surface today** — the building EMP-restore early-return (doc §9 S8 :882) lands new in this slice.

##### 3.4 Drift-flagged surfaces to reconcile (not S8-introduced, but S8-adjacent)
- `production.depot_dock_reservations` (the `DockReservations` FIFO wait-queue pattern; `building_dock.rs:5` comment confirms "FIFO queuing", drained across `tick_building_docks` `:146/:234/:255/:325`) — doc-flagged DRIFT-to-remove (§10.2 :938: gamemd has no dock wait-queue). High-risk to repair-depot behavior; reconcile but do not let S8 depend on the FIFO.
- `remove_wall_entity_at` (`world/mod.rs:1365`) bypasses the substrate (`substrate.entities.remove` directly @ `:1380`). Not building-scoped but named by the design's "route every removal through the substrate" rule.

---

#### 4. Step-by-step tasks (outline level — function-move mapping + shell-step signatures only; NO speculative bodies)

> Per the task, S8 is sketch-only: precise function-move mapping + new shell-step signatures + what becomes shadow/authoritative. **No full bodies** — buildings are last and the binary phase order around the common AI step must be re-decoded fresh before any body is written (the 26-phase sequence in doc §9 S8 :882 is doc-sourced, not re-decompiled this cycle; §10.1 :924 marks the M5 corpus addresses pre-accepted-from-docs).

##### Task A — Add the `Building` arm to the per-object update stage (depends on S0–S4 host)
- **Signature (new):** `fn building_ai(sim: &mut Simulation, id: InternedId, rules: &RuleSet, frame: u32)` — the building leaf shell-step, called from the S0 `object_ai_stage` match arm `EntityCategory::Structure => building_ai(...)`. Dispatch is `match category` (invariant #2 — no trait/dyn).
- **Body shape (outline):** `building_emp_early_return_check` → `building_pre_phases` (per-building reads: power-online latch, occupant counter, docked-object via the existing `tick_building_docks` *result*, not a re-call) → `techno_common_pre` (S4) → `+0xC4` increment → `Mission_Dispatch` (S2) → `techno_common_post` (S4) → `building_post_phases` (gate-state read, delayed-fire counter, anim-slot, health→damage-state gate via `refresh_building_damage_state_gate`) → `building_zero_health_check`. **No body written this slice.**
- **Function-move mapping:** NONE of the global service fns move. `building_pre/post_phases` *read* the per-building fields (`building_gate`, `building_up/down`, `repairing`, `refresh_building_damage_state_gate`) in native order; the global `tick_power_states`/`tick_production_*`/`tick_repairs`/`tick_gate_runtimes` phases stay at their current `advance_tick` call sites (§3.2). The shell-step reconciles per-building *ordering*, not service ownership.

##### Task B — Building EMP-restore early-return (new surface)
- **Signature (new):** `fn building_emp_restore(entity: &mut GameEntity, frame: u32) -> bool` — returns `true` if the building took the EMP-lock-expiry restore path (sets the online-effects flag; the shell early-returns as the final block; doc §9 S8 :882, acceptance `building_emp_restore_early_return` :894).
- **Note:** no EMP surface exists in `power_system.rs` today (only `is_low_power`/`was_low_power`). This is the one genuinely-new building phase. The exact restore-flag semantics need a fresh decode of the building Update EMP block before a body is written — mark UNCHECKED. Any EMP-lock countdown must be a frame-anchored `MissionTimer`, never a per-tick decrement (invariant #5).

##### Task C — Zero-health destruction reconciliation (do NOT relocate combat-driven death)
- **Do not move** the combat-driven destruction (`world/mod.rs:2079` `uninit`, ejection chain `:2136-2150`, SW refresh `:2152-2156`). The S8 `building_zero_health_check` is a *post-common-AI-step IsAlive guard* (doc acceptance `building_zero_health_destruction_pending_delete` :893) that, when health ≤ 0, runs OnDestroyed/SpawnSurvivors/Limbo via the **existing** `uninit` deferred-death contract (`world/mod.rs:1010` — decrement-count → `remove_entity_occupancy` → `clear_radio_contacts_for` → `conceal` → `Presence::Dying` → `pending_delete.push`; slot freed only by `flush_pending_delete` `:1060`). The reconciliation task is to prove the per-building guard and the combat-driven path do not double-fire and preserve the exact post-death chain ordering — **not** to re-home destruction into the bracket.

##### Task D — Assert building skips the units-only unload accumulator
- No code; an **assertion/test** that buildings (RTTI-6) never run the `+0xf8 += +0x110` unload accumulator (`miner_dock_sequence.rs:194`, called `:802` — units-only). Acceptance `building_skips_unload_accumulator_units_only` (:892). A co-located miner must still run it.

##### Task E — Finalize the `ready_to_commence` Building hook
- `mission/verb.rs:166` `ready_to_commence(snap: &ReadySnapshot) -> bool` — the `_ => true` arm (`:171`) covers Building/Infantry/Aircraft. S8 finalizes the Building case by adding the `+0x6DD` building busy-flag to `ReadySnapshot` and a dedicated `ReadyCategory::Building` arm **only after the setter is traced** (prerequisite #2). Until then it stays base-true and the test documents the UNCHECKED status. (This is a free-function + snapshot-struct change, not an entity method.)

---

#### 5. What becomes authoritative / shadow

- **Becomes AUTHORITATIVE (the only flip in S8):** the **per-building Update-bracket ordering** around the Techno-common AI step (Task A pre/post phase sequence), and the **building EMP-restore early-return** (Task B).
- **Lands SHADOW first (shadow-first, invariant #4):** the entire per-building bracket runs shadowed (serde-skip on any new field, NOT hashed, `debug_assert` agreement against the current global-phase behavior) until a full-skirmish replay shows zero divergence (acceptance `s8_full_building_bracket_shadow_zero_divergence` :896). Only then flip, with a `SNAPSHOT_VERSION` bump.
- **STAYS as current global phases (no flip, no collapse — invariant #3):** `tick_power_states`, `tick_production_with_overlay_registry`, `tick_repairs`, `tick_gate_runtimes`, `tick_building_up/down`, combat-driven destruction, HouseClass/Factory/superweapon services. The shell reads their *results*; it does not own them.
- **STAYS shadow until S5 (prerequisite):** `MissionCom` for buildings (no `derived_mission` building leg yet — buildings currently derive to `MissionType::None`).

---

#### 6. Named acceptance tests (exact fn names — from doc §9 S8 :890–:896)

1. `building_update_27_phase_bracket_order` — the verified phase sequence around the common AI step (gamemd phase 11) is reproduced; house/factory global phases unchanged (pins invariant #3).
2. `building_skips_unload_accumulator_units_only` — buildings (RTTI-6) never run the `+0xf8`/`+0x110` accumulator; a co-located miner still does (Task D).
3. `building_zero_health_destruction_pending_delete` — zero-health destruction runs OnDestroyed/SpawnSurvivors/Limbo and enqueues to `pending_delete` with the destruction-delay timer; the IsAlive guard returns post-common-AI-step (Task C; pins deferred-death invariant #6).
4. `building_emp_restore_early_return` — the EMP-lock-expiry restore path sets the online-effects flag and early-returns as the final block (Task B).
5. `building_fog_darkening_not_implemented` — fog-of-war darkening branches are absent (TS_LEGACY, default off); only shroud is modeled (§7 below).
6. `s8_full_building_bracket_shadow_zero_divergence` — full-skirmish shadow replay shows zero `debug_assert` divergence before the authority flip; golden rebaselined with `SNAPSHOT_VERSION` bump (pins invariant #4 + #8).

> Note: test #1 name is `building_update_27_phase_bracket_order` (doc :891) — the "27" is intentional in the doc (26 building phases + the wrapped common AI step). The bracket-sequence count is re-decoded fresh per prerequisite #4 before the test pins a literal order.

---

#### 7. TS_LEGACY / do-not-implement (per task + doc §10.2)

- **Fog-of-war "previously seen" darkening branches** inside any building gap-generator / special-fx phase → **TS_LEGACY, default OFF** (`SpecialFlags & 0x1000`; FogOfWar=no default, rulesmd.ini:3040; doc §10.2 :934). Model **shroud only**. Acceptance `building_fog_darkening_not_implemented` (:895) asserts these branches are absent.
- **Building unload accumulator** (`+0xf8 += +0x110`, RTTI-6 skip) — assert absence for buildings, do not implement (doc §9 S8 :882).
- **Dock/radio wait-queue or FIFO** — gamemd has none (§10.2 :938); do not design one into the building docking shell. Reconcile the existing `production.depot_dock_reservations` FIFO as DRIFT-to-remove, separately from S8's flip.
- Aircraft AreaGuard / Ambush(14) / tunnel/subterranean are not building concerns (§10.2 :932–:939).

---

#### 8. Determinism / hash notes

- **Hash-affecting:** S8 is hash-affecting (doc ledger :912). The authority flip requires a `SNAPSHOT_VERSION` bump + rebaselined golden, gated on the shadow replay (`s8_full_building_bracket_shadow_zero_divergence`) showing zero divergence first.
- **MissionTimer (invariant #5):** building gate timers (`transition_timer`/`hold_timer`, `game_entity.rs:100`/`:107`) are already frame-anchored `MissionTimer` (start_frame + duration, never-decrement; Slice 1). The EMP-restore and destruction-delay timers (Tasks B/C) must also be `MissionTimer`, never per-tick `u8`/`u16` decrements.
- **Deferred death (invariant #6):** zero-health destruction routes through the existing `uninit` (`world/mod.rs:1010`) → synchronous conceal/unmark/detach → `pending_delete.push` → deferred slot-free at `flush_pending_delete` (`:1060`, drained `:1719`/`:1770`/`:2254`). No synchronous free.
- **RNG (invariant #7):** the building destruction path consumes RNG (SpawnSurvivors debris/garrison pick) at the same per-object position; do not add or move a draw. The damage-fire particle spawn (doc §10.3 :948 — `Random__RandomRanged` ×2 inside the AI step, gated ConditionYellow + DamageParticleSystems + `+0x308==0`) must consume at the matching position. No new RNG draws in the shadow phase.
- **Iteration order:** S8 does not move a phase (invariant #3). Movement/combat/retaliation continue to iterate `live_object_order_snapshot` (`world/mod.rs:929`, NO sort); vision (`refresh_fog` `:1395`, called `:1967`) and power (`:1973`) iterate `entities.values()` (BTreeMap id-ascending). The building shell-step plugs into whatever per-object stage S0–S4 establish — it does not introduce a new iteration model (§10.2 :929: per-phase iteration model, not hardcoded).

---

#### 9. Dependencies + risk / do-not-do

##### Prerequisites (hard blockers — S8 is not plannable beyond this outline until cleared)
1. **The shell host must exist.** S0–S4 (UnitClass shell, `object_ai_stage`, `techno_common_pre/post`, the `+0xC4`→dispatch→post ordering) + S5 (MissionCom authority flip + per-category `ready_to_commence`) + S6/S7 (Infantry/Aircraft leaves) per ledger (:912). **None of `sim/ai/` exists today** (confirmed: only `src/sim/ai.rs` computer-AI + `src/sim/aircraft/`).
2. **Building `ready_to_commence` busy-flag `+0x6DD` is UNCHECKED** (`mission/verb.rs:171` base `true`; §10.3 :944). Setter must be traced from the binary before the Building hook is field-accurate.
3. **MissionCom must be authoritative (S5)** and `derived_mission` (`game_entity.rs:482`) must gain a building leg — today a building with no miner/aircraft/dock/attack/movement state derives to `MissionType::None` (`:509`).
4. **Re-decode `BuildingClass::Update` fresh** — the 26-phase sequence (doc §9 S8 :882) is doc-sourced, not re-decompiled this cycle (§10.1 :924). The exact pre/post phase order around the common AI step, the EMP-restore block, and the zero-health/destruction-delay timer must be verified from the binary before any body is written.

##### Risk / do-not-do
- **Highest blast radius of any slice** (doc §9 S8 :888, §10.2 :930): touches power transitions, ProduceCash, auto-sell, repair, auto-production, bridge destruction, zero-health destruction — all reading/writing HouseClass/Factory globals via the Phase-7 functions. **Mitigation:** shadow the entire bracket first; flip only after zero-divergence replay; keep the global house/factory phases untouched.
- **Do NOT collapse** the global power/production/repair/gate phases into the per-building bracket (invariant #3, doc §10.2 :928/:930). The shell reads their results; it does not own them.
- **Do NOT relocate combat-driven destruction** into the bracket (Task C) — the post-death ejection chain (`world/mod.rs:2136-2150`) and SW refresh (`:2152-2156`) must keep their exact ordering and the deferred-Dying window.
- **Do NOT design or depend on a dock wait-queue** (§10.2 :938). Reconcile the existing `production.depot_dock_reservations` FIFO as separate DRIFT-to-remove.
- **Do NOT start the leaf migration with BuildingClass** (§10.2 :930) — S8 is last; this outline only becomes actionable tasks after S0–S7 land.
- **Do NOT port the C++ class tree** — dispatch stays `match category` + `Option<T>` + `CapabilityFlags`; no `dyn`/vtable/COM (invariant #2, §10.2 :940).

---

##### FACTS-block line drift corrected this session (use these, not the FACTS numbers)
`advance_tick` @ `mod.rs:1742` (FACTS :1706); `refresh_mission_shadow` def `:895`/call `:2391`/`state_hash` `:2394` (FACTS :859/:2355/:2358); `uninit` `:1010` (FACTS :974); `flush_pending_delete` `:1060`, drains `:1719`/`:1770`/`:2254` (FACTS :1024, :1683/:1734/:2218); `refresh_fog` def `:1395` called `:1967`, power `:1973`; **combat destruction `uninit` `:2079`** (the FACTS block AND the draft both said `:2078` — corrected), ejection `:2136-2150`, SW refresh `:2152-2156`; `building_up/down` `:1507/:1531` called `:1688/:1690`; `live_object_order_snapshot` `:929`; `remove_wall_entity_at` `:1365` (substrate bypass at `:1380`, FACTS said `:1344`). The `mod.rs` drift is ~+36 lines from the FACTS snapshot — every `mod.rs` line in this plan was re-verified against the live file.

---

## Acceptance test index (consolidated — every named test in this plan)

**Slice L3 — Aircraft** (new file `src/sim/world/slice7_aircraft_tests.rs`, registered at `mod.rs:2430`):
- `aircraft_missions_dispatched_not_global_sweep` — per-object live-order dispatch; pad-contended RTB/Docking ordering preserved.
- `aircraft_dispatch_is_thin_router_not_inline_sm` — dispatcher routes only; non-aircraft no-op; one-shot byte-clear deferred to S5 (comment).
- `aircraft_crash_and_bounds_kill_pending_delete` — crash/OOB → `uninit` → `pending_delete`. **`#[ignore]`** until the Ghidra crash/OOB threshold trace lands.
- `aircraft_self_destruct_routes_through_deferred_death` — self-destruct concealed/unmarked same tick (invariant #6 fix). **Ships now.**
- `aircraft_areaguard_unrepresentable_no_dispatch_arm` — no AreaGuard variant / no dispatcher arm (type-level do-not-implement).
- `s7a_per_object_shadow_zero_divergence_single_aircraft` — shadow per-object == global-sweep, single-aircraft, every tick.

**Slice L4 — Infantry fear** (value tests append to `infantry.rs:185`; order/shadow tests in the net-new shell test module):
- `infantry_fear_decay_thresholds` — 50→Down, 49 prone→Up, 49 standing→None, decay every tick. **No 199 / no panic-scatter assertion** until sourced.
- `infantry_ai_order_capture_fear_fire_sequencer` — within-AI order: FootClass::AI → Mission_Capture → fear → Fire. (shell test module)
- `fear_prone_suppressed_during_locked_sequence` (⚠️ gate is the current `Doing` sequence ∈ {27-30}, NOT a mission — blocked on the `Doing` enum) — fear≥50 + locked sequence → no prone.
- `fear_up_suppressed_during_locked_sequence` (⚠️ same correction) — prone + fear<50 + locked sequence → no stand.
- `fraidycat_no_prone_keeps_current_behavior` — current Fraidycat behavior pinned + DRIFT comment (rec. 3b). (rename `fraidycat_scatter_flee_above_threshold` if 3a chosen).
- `fear_decay_decrements_regardless_of_mission` — decay runs even on excluded missions; only transition gated.
- `infantry_sequencer_self_destroy_enqueues_pending_delete` — death-sequence completion → `uninit` → `pending_delete` (if Task 5 in scope; aligns with S6 `infantry_self_removal_enqueues_pending_delete`).
- `fear_shell_relocation_shadow_zero_divergence` — Task 6a: shell `debug_assert`-agrees with global sweep pre-flip.
- *Must stay green:* `decay_thresholds_and_fearless_decay_gate`, `fraidycat_rejects_fear_driven_down`, `crawls_gate_only_blocks_down_not_recovery`, `first_hit_and_fraidycat_set_fear`, `repeated_hit_adds_by_health_and_clamps`, `fearless_type_and_abilities_block_application`, `prone_speed_rounding_is_exact`, `object_category_import_keeps_rules_fixture_infantry`.

**Slice L6 — Commence gate + MissionCom authority:**
- *Sub-step A (golden unchanged):* `queue_commence_gated_by_ready_to_commence`, `ready_to_commence_base_returns_true_four_leaf_overrides` (extend `slice6_ready_to_commence_base_true_unit_not_while_driving:307`), `queue_mission_commence_false_writes_queued_not_current`, `override_saves_queued_when_pending_else_current` (already covered by `slice6_override_with_queued_discards_current_saves_queued:257` + `slice6_override_without_queued_saves_current_then_restore:246`), `commence_rearms_timer_due_next_tick`, `replay_hash_stable_through_slice6` (stays `17281687802996982350`).
- *Sub-step B (one rebaseline):* `mission_com_authority_cross_check_holds`, `mission_com_selector_hashed_after_flip`, `slice6_retaliation_still_suppressed_for_guarding_unit:204` (**MUST stay GREEN**), `replay_hash_stable_through_slice6:73` (**REBASELINE**, version-16+fold reproducible), `s5_mission_authority_flip_golden_rebaselined` (busy-byte exclusion proof).

**Slice L7 — Building (outline; doc §9 S8 :890–:896):**
- `building_update_27_phase_bracket_order`, `building_skips_unload_accumulator_units_only`, `building_zero_health_destruction_pending_delete`, `building_emp_restore_early_return`, `building_fog_darkening_not_implemented`, `s8_full_building_bracket_shadow_zero_divergence`.

---

## Cross-cutting invariants (recap — every slice above respects these)

1. **`sim/` never depends on `render`/`ui`/`sidebar`/`audio`/`net`.** No slice adds such a dependency; all work is inside `src/sim/`.
2. **No C++ class tree, no `dyn`/vtable/COM.** Dispatch is `match category` + `CapabilityFlags` + `Option<T>`. L3's aircraft guard, L4's mission-exclusion + shell dispatch, L6's `ready_to_commence` 4-arm match, and L7's `object_ai_stage` `Building` arm are all `match category`.
3. **`advance_tick` phase order is PRESERVED** until a slice explicitly changes it. L3 keeps aircraft dispatch between Phase 2.5 and Phase 3 vision; L4 keeps fear committing `is_prone` before combat reads it; L6 keeps retaliation at its Phase-6 site and the refresh/assert/state_hash sites; L7 keeps every global service phase in place (no collapse).
4. **Shadow-first.** New authority lands shadowed (serde-skip, not hashed, `debug_assert` agreement) before the flip. L3's `dispatch_aircraft_mission` (S7a), L4's iteration-order relocation (Task 6a), L6's MissionCom (sub-step A keeps it shadow), and L7's whole bracket all land shadowed first. (L4's *value* corrections are a direct authoritative change with golden rebaseline — a shadow over already-live hashed values is meaningless, explicitly noted.)
5. **Frame-anchored timers (`MissionTimer` start_frame+duration) never decrement.** L3's `Docking.reload_timer`, L6's `commence_queued`/`assign_mission` re-arm via `timer.reset(now)`, and L7's gate/EMP/destruction-delay timers are all frame-anchored. (L4's fear counter is the deliberate exception — a genuine per-tick `-=1`, NOT a `MissionTimer`; do not refactor it onto one.)
6. **Deferred death** — enqueue `pending_delete`, synchronous conceal/unmark/detach, deferred slot-free. L3 routes crash/OOB/self-destruct through `uninit` (fixing a standing leak); L4's sequencer self-Destroy routes through `uninit`; L7's zero-health destruction reuses the existing `uninit` contract. No synchronous `entities.remove` mid-AI.
7. **RNG consumed at the same per-object position/gate.** L3 adds no draw (verify no aircraft handler draws before the order flip); L4 avoids RNG via the recommended deferrals (3a's scatter draw would be a phantom-desync if mis-positioned — defer); L6 verbs are pure (do not add the `RandomRanged(0,2)` re-arm jitter — no consumer here); L7 keeps SpawnSurvivors / damage-particle draws at their matching positions.
8. **Every behavior-moving slice needs a NAMED acceptance test pinning gamemd order before it flips.** Each hash-affecting flip (L3 S7c, L4 Task 6b, L6 sub-step B, L7 the bracket flip) is gated on its named shadow/zero-divergence test and takes a `SNAPSHOT_VERSION` bump + golden rebaseline with a single cited cause.
