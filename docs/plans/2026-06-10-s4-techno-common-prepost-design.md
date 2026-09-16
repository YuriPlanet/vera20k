# Slice S4 — TechnoClass common pre/post-mission body (units) — DESIGN / SCOPING

**Status:** DRAFTED — not approved. **Blocked on a research prerequisite** (see §1).
**Date:** 2026-06-10
**Rule:** Rust-native structure, gamemd-native semantics.
**Ladder position:** S0–S3 merged to `dev` (S3 = `073c5ac4`; `SNAPSHOT_VERSION` 23).
S4 is the next rung. Source of the slice contract:
[docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md) §9-S4 (slice table row
`S4 | UnitClass | pre/post common + RNG pos | yes | S3`).

S4 operates **one layer deeper than S3**. S3 owns the post-Foot `UnitClass::AI` tail
(Fire → Facing → … → Spawn, in `world/unit_post.rs`). S4 owns the **`TechnoClass::AI_Update`
common body** that runs *inside* `FootClass::AI`, beneath that tail:

```
UnitClass::AI (0x007360C0)
  └─ FootClass::AI (0x004DA530)              [10 subsystems; AI_Update is #1 — FOOTCLASS_COMPLETE §3.1]
       └─ TechnoClass::AI_Update (0x006F9E50)   ← S4 OWNS THIS BODY
            ├─ pre-mission block      (steps 1–20)        [S4a]
            ├─ +0xC4 AI-tick counter  (step 21)           [LANDED S2]
            ├─ Mission_Dispatch       (step 22, 0x005B3060)[LANDED S2]
            └─ post-mission block     (steps 23–42)       [S4a/b/c]
                 ├─ step 23: passive/opportunity acquire (missions 2/10/5)  [S4c — SHADOW]
                 ├─ step 40: damage-particle RNG spawn                       [S4b — AUTHORITATIVE, lockstep]
                 └─ step 42: EMP-recovery (building branch only)             [deferred to S8]
       └─ ILocomotion::Process (vtable+0x40, ~0x004DA877)  [runs AFTER dispatch — S1/S2 ordering]
  └─ (post-Foot tail: TurretAI / Fire_At_Target / Facing_Update)  [S3]
```

Spine verified from the live binary in the S-design D4/D6 lanes
(`decompile_function 0x004DA530`, `0x006F9E50`, `0x0073647B`/`0x004DA539` cited inline in
the S-design §"central per-object spine" and `UNITCLASS_PERCELLPROCESS_CALLER_TICK_ORDER` §3.2).

---

## 1. The gating research gap (READ FIRST)

**S4 cannot be designed to step-accuracy, nor implemented, until a verified full-body decode
of `TechnoClass::AI_Update (0x006F9E50)` exists.** It does not exist today.

- The S-design references D4 by **step number** only (steps 12, 20, 21, 22, 23, 27, 40, 42) —
  there is no enumerated, RNG-annotated body list. Provenance: the S-design workflow notes
  "6 of 15 lanes returned structured findings"; D4 did its `decompile_function 0x006F9E50`
  work but its output was reconciled **inline by step-number**, never saved as a standalone
  report (S-design provenance line; confirmed by corpus scan — only
  `BUILDINGCLASS_UPDATE_AI_TICK` and `TECHNOCLASS_AI_MIGRATION_BOUNDARY` name AI_Update, and
  the former's a–z paraphrase is explicitly **"NOT the true order"**, S-design line 588).
- Pieces exist scattered across verified docs (§3 anchors below) but no single ordered body.
- S2 and S3 each had their layer's verified report backing them (UnitClass turret/fire-timing,
  L2 fire-damage-timing, FootClass mission-move). **S4's TechnoClass-common layer has no
  equivalent.**

**Partial unblock (docs-grounded, 2026-06-10):** [docs/research/TECHNOCLASS_AI_UPDATE_BODY_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_SYNTHESIS.md)
reconstructs the body order from verified docs by in-function byte address (split point =
`Mission_Dispatch 0x006FA655`; landmarks for rocking/IsAlive/behind-marker/passive-acquire/
damage-particle/timer-accumulator/EMP). It makes four corrections to this design (EMP recovery
has a **foot branch**, not building-only; **EMP decrements per-tick** unlike iron-curtain/temporal;
the "three early-returns" are really **two IsAlive returns inside AI_Update** plus a separate
BuildingClass post-parent check; damage-particle is post-dispatch on `g_MainRng`). It pins the
six strictly-binary gaps (U1–U6). The Ghidra task below is now **verify-and-fill the synthesis**,
not a from-scratch decode.

**Prerequisite task (Ghidra-gated): produce [TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md)** by
verifying the synthesis §2 map and resolving its U1–U6 —
a `/re-investigate`-grade decode of `0x006F9E50` enumerating, per step in body order:
step index · address · field(s) touched · gate condition · RNG draw (count + stream + helper)
· early-return? · active-in-YR. Special focus (lockstep-critical):

1. **Damage-particle spawn block** (`~0x006FA6xx`): exact `g_MainRng` draw count and order
   (spawn-probability roll, `FUN_007178c0` visual offset, `ParticleSystemClass` ctor internal
   draw, list-pick), the gate (`+0x308==NULL` + `HealthRatio<ConditionYellow` + has-systems),
   and the **byte position** within the body. This is the only S4 piece that moves the hash.
2. **The two unit early-returns**: step 12 (self-heal kills the unit mid-pre-block) and step 27
   (`IsAlive` after `Mission_Dispatch`). Confirm they are unit-reachable (not building-only).
3. **`+0x70` smoothed health** (step 7): confirm it is purely visual (render path), so S4 keeps
   it unhashed. S-design line 381 lists `+0x70` as "smoothed health" — verify no sim consumer.
4. **Iron-curtain / force-shield / temporal timers**: confirm they are passive
   `CurrentFrame - Start < Duration` checks, **not** per-tick decrements (S4 asserts the absence).

Until that report lands, this doc is a **scoping** doc: it fixes the slice shape and the
sub-slice boundaries, not the per-step Rust.

---

## 2. What is already true in the current tree (verified this session)

| Item | State | Evidence |
|---|---|---|
| `+0xC4` AI-tick counter (`mission.tick_counter`) | **LANDED (S2)** — exactly-once/tick | `techno_ai.rs` test `s2_tick_counter_increments_exactly_once`; in-loop + tail increment |
| `Mission_Dispatch` (in-loop, scoped Units) | **LANDED (S2)** | `movement_tick.rs` dispatch; `techno_ai.rs` host shadow |
| `DamageParticleSystems=` CSV | **PARSED** | `rules/object_type.rs:768,1186` |
| `ConditionYellow` / `ConditionRed` | **PARSED** (`condition_yellow_x1000`) | `rules/ruleset.rs:281-283` |
| Damage-particle RNG draw (sim, lockstep) | **ABSENT** — zero draws; our particles are render-side | corpus scan: no `damage_particle` RNG anywhere in `src/sim/` |
| `OpportunityFire` parse | **LANDED (S4c prep, 2026-06-10)** — `object_type.opportunity_fire`, default no | parse + 2 tests; consumption still deferred |
| `CanRetaliate` parse | **LANDED (S4c prep, 2026-06-10)** — `object_type.can_retaliate`, default yes | parse + 2 tests; consumption still deferred |
| `CanPassiveAcquire` INI key | **DOES NOT EXIST in stock INI** | grep `ini/*.ini` = 0 hits — gate is `OpportunityFire` + weapon, not a separate key |
| Passive/opportunity scanner | **ABSENT** | no `vtable+0x39C` analogue in `src/sim/combat/` |
| `+0x70` smoothed-health *sim* field | **ABSENT** (health bars are render-side) | `app_ui_overlays.rs` / `app_building_anim.rs` are app-layer |

**Consequence:** S4's "+0xC4 increment / Mission_Dispatch" wording is already satisfied by S2.
S4's *new* work is (a) wrapping that with the pre/post common bracket + the two unit
early-returns, (b) the damage-particle RNG, (c) the passive-acquire shadow.

---

## 3. Verified anchors for the S4 components (existing docs)

- **Damage-particle spawn (step 40):** [PARTICLESYSTEMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLESYSTEMCLASS_GHIDRA_REPORT.md) §8.6.1
  (`TechnoClass::AI_Update 0x006f9e50` — Damage Smoke). Gate `+0x308==NULL` +
  `health_ratio < ConditionYellow` + has-DamageParticleSystems; spawn-probability roll
  (`RulesClass +0x558/+0x560` red/yellow bands), `FUN_007178c0` random visual offset, system
  list-pick. **Stream = `g_MainRng`** for all particle spawns
  ([PER_FRAME_RNG_CONSUMPTION_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PER_FRAME_RNG_CONSUMPTION_ORDER_GHIDRA_REPORT.md) §3.1: "particles (all types)" →
  `g_MainRng`; ParticleClass ctor draws 1). Exact draw **count/position UNVERIFIED** → §1 gate.
- **Passive/opportunity acquire (step 23):** [GRIZZLY_OPPORTUNITYFIRE_CONSUMER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GRIZZLY_OPPORTUNITYFIRE_CONSUMER_GHIDRA_REPORT.md)
  + [GRIZZLY_OPPORTUNITYFIRE_FIRST_SHOT_TIMING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GRIZZLY_OPPORTUNITYFIRE_FIRST_SHOT_TIMING_GHIDRA_REPORT.md). `Mission_Dispatch` runs
  **first**; then the mission **{2 Move, 10 Harvest, 5 Guard}** block reads `OpportunityFire`
  (`TechnoType +0x6AF`) and runs the scanner `vtable+0x39C` (`0x006FA699..0x006FA6C1`), writing
  `g_CurrentFrameCounter` to `+0x4FC`; passive-acquire timer at `+0x180/+0x188` (45-frame
  cadence per S-design). The scan **sets the combat target only** — it does not fire; the shot
  is the later `UnitClass::AI → Fire_At_Target` path (S3's seam). Side-target waits for turret
  alignment. → **S4c lands the scanner as a shadow; it flips authoritative in S5** (keyed on the
  then-authoritative mission selector).
- **EMP recovery (step 42):** [TECHNOCLASS_SYSTEMS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_SYSTEMS_GHIDRA_REPORT.md) §6.3 +
  [RADIATION_EMP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIATION_EMP_GHIDRA_REPORT.md) §2.6 (`+0x504` EMPLockRemaining → 0 ⇒ recover; building
  branch `RestoreOnlineEffects`, foot branch restarts locomotor). **Building branch deferred
  to S8** per §9-S4; the foot branch (locomotor restart) is the unit-relevant part.
- **FootClass::AI subsystem order:** [FOOTCLASS_COMPLETE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_COMPLETE_GHIDRA_REPORT.md) §3.1 (10 subsystems:
  AI_Update #1, tib self-heal #2, veteran promote #3, locomotor Process #4, …). Frame-modulo
  cadences (tib self-heal `frame % Rules+0x1808`, the `&0x8000000f` 16-frame gate) in
  [FRAME_MODULO_CADENCE_INVENTORY_YR_TICK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FRAME_MODULO_CADENCE_INVENTORY_YR_TICK_GHIDRA_REPORT.md).
- **Mission-dispatch-before-locomotor:** `UNITCLASS_PERCELLPROCESS_CALLER_TICK_ORDER` §3.2,
  `TECHNOCLASS_AI_UPDATE_UNLOAD_ACCUMULATOR_ORDERING` (Mission_Dispatch before the unload
  accumulator; buildings RTTI-6 `goto`-skip it — assert units-only).

---

## 4. Proposed sub-slice breakdown

S4 as written bundles a structural reorder, a lockstep-critical RNG change, and a net-new
shadow subsystem. Per the ladder's small-first ethos and to isolate the lockstep risk, split:

### S4a — Common-body host bracket + the two unit early-returns (shadow → flip; structural)
Establish `techno_common_pre(sim,id)` / `techno_common_post(sim,id)` host functions in
`world/techno_ai.rs`, wrapping the existing S2 in-loop `+0xC4` + dispatch:
```
techno_common_pre(sim,id);     // pre block (steps 1–20), unit-relevant subset
// +0xC4 + Mission_Dispatch already landed (S2)
if !sim.is_alive(id) { return; }   // early-return: died in dispatch (step 27)
techno_common_post(sim,id);    // post block (steps 23–42), unit-relevant subset
```
Most pre/post steps are already modeled elsewhere (cloak, veterancy, etc. are separate
systems) or are no-ops for units. S4a's job is **ordering + the early-returns**, not relocating
every subsystem. Step 12 (self-heal death) and step 27 (`IsAlive`) become explicit early-exits.
**Shadow-first**: the bracket records its order in debug and proves `state_hash` bit-identical
before the authority flip. Asserts: no per-tick iron-curtain/temporal decrement.
*Hash-affecting only if it relocates a hashed write — keep relocations behavior-neutral or defer.*

### S4b — Damage-particle RNG faithful consumption (authoritative, lockstep-critical)
**DECODE COMPLETE** (`TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md §4`). Verified U2:
- **Stream = `Scen->Random`** (Scen+0x218, Scen=`*0x00a8b230`) — **NOT g_MainRng** (corrects the
  prior assumption). S4b must draw from VERA's **Scen->Random-equivalent** hashed stream.
- **Count: 0 / 0 / 1 / 2** — 0 if outer gate fails (no `DamageParticleSystems` Spark type, or
  HealthRatio ≥ ConditionYellow, or `vtable+0x1c8 ≤ -10`); 0 if inner gate fails (`+0x308 ≠ 0`
  live system, or zero Spark-`BehavesLike==3` systems); 1 if the prob-roll
  `RandomRanged(0,0x7ffffffe)` fails; 2 if it succeeds (+ list-pick `RandomRanged(0,count-1)`).
  `FUN_007178c0` (offset) and `ParticleSystemClass__Constructor` draw **nothing**.
- **Position:** post-mission block, after the anim-stage step, `0x006fae24` (roll) + `0x006faeb3`
  (list-pick).
Reproduce the **consumption only** (gate + roll + on-success list-pick) so the Scen->Random stream
stays bit-aligned; the particle **visual stays render-side** (unhashed). Model the gate exactly
(`Type+0xC8F` emits-damage-particles + `<ConditionYellow` + `+0x308`-empty + Spark-system-count) or
the count drifts. `SNAPSHOT_VERSION` bump + golden re-baseline (new Scen->Random draws shift it).
**Risk:** wrong count/stream/position = full-match desync. Prereq scoping: confirm VERA exposes a
Scen->Random-equivalent hashed stream + the spawn-chance Rules values are parsed.

### S4c — Passive/opportunity acquire (shadow) + INI parse
Parse `OpportunityFire` (+0x6AF) and `CanRetaliate` into `object_type.rs` (additive, zero
behavior). Implement the missions-{2,10,5} 45-frame (`+0x180/+0x188`) scanner as a **shadow**
(records would-be target acquisitions; zero divergence asserted; never writes the hashed target).
Confirm `+0x70` smoothed health stays render-only. **Authority flip is S5**, keyed on the
authoritative mission selector. No `SNAPSHOT_VERSION` bump (shadow).

Dependency: S4a → S4b → S4c is the safe order (bracket first, then the RNG inside it, then the
post-block scanner). S4c's INI parse has no dependency and can land first as prep.

---

## 5. Hash deltas & versioning

- **S4a:** ideally hash-neutral (structural bracket + early-returns that fire only on already-dead
  units). If any relocation is unavoidably hash-affecting, name it and bump; otherwise no bump.
- **S4b:** **hash-affecting** — introduces new `g_MainRng`-stream draws. `SNAPSHOT_VERSION`
  23→24, golden re-measured with the cited draw count/position.
- **S4c:** hash-neutral (shadow). No bump.

Per invariant #8, every flip carries a gamemd-evidence-cited golden; every shadow proves zero
`state_hash` movement before its later flip.

---

## 6. Acceptance tests (from §9-S4, mapped to sub-slices)

- S4a `techno_ai_pre_then_dispatch_then_post_order` — bracket runs pre → +0xC4 → dispatch → post.
- S4a `unit_died_in_dispatch_early_returns_no_post` — step-27 early-return skips the post block.
- S4a `iron_curtain_temporal_timers_not_decremented_per_tick` — on-demand `frame-start<duration`.
  **EMP is EXCLUDED** from this assertion: `EMPLockRemaining` genuinely decrements per-tick
  (synthesis §3.2) — do not lump it with iron-curtain/temporal. (Moot until EMP is modeled.)
- S4b `damage_particle_rng_consumed_at_native_position` — draw count/position per tick matches the
  decoded gate; a unit *not* below ConditionYellow consumes **zero** draws.
- S4b `health_smoothing_not_hashed_render_only` — `+0x70` never affects `state_hash`.
- S4c `passive_acquire_only_missions_2_10_5_shadow` — shadow scan fires only for missions 2/10/5
  at the 45-frame cadence; zero divergence asserted before any flip.

---

## 7. Work split: Ghidra-gated vs independent

**Ghidra-gated (needs a running gamemd instance):**
1. [TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md) decode (§1) — the prerequisite for everything.
2. S4b damage-particle exact draw count/stream/position.
3. Confirm the two unit early-returns are unit-reachable; confirm `+0x70` render-only;
   confirm iron-curtain/temporal are non-decrementing.

**Ghidra-independent (can proceed now):**
1. S4c INI parse: `OpportunityFire` (+0x6AF), `CanRetaliate` → `object_type.rs`
   (INI-grounded, additive, zero lockstep risk). `CanPassiveAcquire` is NOT a stock key — do
   not add it; the gate is `OpportunityFire` + weapon presence.

---

## 8. Decisions (user-confirmed 2026-06-10)

1. **Sub-slice split CONFIRMED:** land S4a → S4b → S4c as separate design→plan→implement cycles.
2. **RNG approach CONFIRMED: consumption-only / stream-align.** S4b reproduces the exact
   `g_MainRng`-equivalent draw count + position at the native per-object spot to keep the shared
   stream bit-aligned; the particle visual stays render-side (unhashed). No new sim-side
   particle-system subsystem. (Watch item: ensure the render path does not *also* draw from the
   synced stream — it must not, or the count doubles.)
3. **Next step CONFIRMED:** run the `TechnoClass::AI_Update` body decode (§1 prerequisite) once a
   gamemd Ghidra instance is up; that report grounds the S4a design and the S4b draw count.

---

## 9. Decode execution checklist (TECHNOCLASS_AI_UPDATE_BODY report)

Run when Ghidra is up. Authority order binary → Ghidra → docs; cite each MCP call inline; default
verdict DRIFT; mark anything not read this session UNCHECKED.

1. `decompile_function 0x006F9E50` — full body. Walk top→bottom; bucket every statement into
   **pre-mission (before the `+0xC4` inc + `Mission_Dispatch` call at `~0x006FA6xx`)** vs
   **post-mission**. Record the `Mission_Dispatch (0x005B3060)` call address as the split point.
2. Identify the **+0xC4** increment site and confirm it is immediately before the dispatch call
   (S2 modelled it there — verify position).
3. **Early-returns:** find every `ret`/early-exit. Confirm step-12 self-heal-death and step-27
   `IsAlive`-after-dispatch are present and **unit-reachable** (not gated to `WhatAmI==Building`).
   Record each guard's field + condition.
4. **Damage-particle block** (`~0x006FA6xx`, the §8.6.1 path): `decompile_function` the spawn
   helper(s); enumerate **every** RNG call in order — spawn-probability roll, `FUN_007178c0`
   offset, `ParticleSystemClass` ctor (recurse: `0x0062B842`-area lifetime draw), list-pick.
   For each: helper (`Random__Next` vs `RandomRanged`), bound, and **receiver/ECX** (is it
   `g_MainRng @ 0x00886B88` or `Scen->Random @ Scen+0x218`?). Record the exact draw COUNT for
   (a) gate-fails, (b) gate-passes-roll-fails, (c) gate-passes-roll-succeeds. This is the S4b
   golden input — get it byte-exact.
5. **Passive-acquire block** (`0x006FA699..0x006FA6C1`): confirm the mission gate is exactly
   **{2,10,5}**, the `OpportunityFire` read is `+0x6AF`, the scan timer is `+0x180/+0x188`
   (45-frame), and the scanner is `vtable+0x39C`. Confirm it writes the target field (`+0x2B4`)
   and `+0x4FC`, and does **not** fire.
6. **`+0x70` smoothed health** (step 7): confirm it is read only by render/draw, not by any sim
   gate — so S4 keeps it unhashed.
7. **Iron-curtain / force-shield / temporal**: confirm each is a passive `frame - start < dur`
   check, **not** a per-tick decrement.
8. Cross-check the body order against `FOOTCLASS_COMPLETE §3.1` (AI_Update is FootClass subsystem
   #1) and `TECHNOCLASS_AI_UPDATE_UNLOAD_ACCUMULATOR_ORDERING` (dispatch precedes the unload
   accumulator; buildings RTTI-6 skip it). Note any conflict as DRIFT.

Output: [docs/research/TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md) — ordered step table
(index · addr · field · gate · RNG[count,stream,helper] · early-return · active-YR) + the S4b
draw-count truth table + an UNCHECKED section.

---

## 10. S4a Rust landing-surface inventory (current tree, 2026-06-10)

Where S4a's bracket attaches and which post-block systems already exist:

- **S2 authoritative dispatch + `+0xC4`:** `src/sim/movement/movement_tick.rs:1041-1046`
  (in-loop `mission.tick_counter` increment + dispatch-time mission commit). This is the wrap
  point for `techno_common_pre` (before) / `techno_common_post` (after).
- **`object_ai_stage` host:** `src/sim/world/mod.rs:2017` (returns `dispatch_trace`) — currently
  the **shadow/record** host (S0/S1/S2-shadow), distinct from the authoritative in-loop dispatch.
  **S4a design must resolve** whether the bracket lives at the authoritative site (movement_tick)
  or migrates dispatch into the host. Combat at `:2283`, `run_late_region` at `:2677`,
  `refresh_mission_shadow_except` at `:2692`.
- **EMP paralysis/recovery (post-block step 42):** **NOT modeled in sim** (grep `emp_lock`/
  `EMPLock` → 0 hits; broad `EMP` matches are `temp`/`empty` false positives). Building EMP is
  S8 anyway; a foot-unit EMP-recovery branch would be net-new — **not an S4a blocker**, name it.
- **Self-heal (post-block step 12 self-heal *death* early-return):** **NOT modeled** (`SelfHealing`
  INI key unparsed; no unit self-regen — the `heal/regen/repair` hits are bridge/building/AI repair).
  ⇒ The step-12 self-heal-death early-return is **moot until a self-heal system exists**; S4a
  implements only the **step-27 `IsAlive`-after-dispatch** early-return (we have death/IsAlive),
  and carries step-12 as a named placeholder.
- **Implication:** S4a's real authoritative content is narrower than "fold steps 1–42" — most
  pre/post steps are either separate existing systems or absent (EMP/self-heal). S4a = the bracket
  ordering + the step-27 early-return; the decode (§9) confirms the exact present-vs-absent split.
