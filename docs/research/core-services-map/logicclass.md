# Core Service Profile — LogicClass (per-tick update scheduler / tick spine)

**Slug:** `logicclass`
**Primary doc:** [docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) (2026-05-29, study/design,
binary→Ghidra→docs, every load-bearing claim re-verified live that session).
**Role in the graph:** the *spine*. This service defines the ORDER in which every other per-tick
service runs. Almost every other core service is an INCOMING dependent — they are invoked *from*
the LogicClass driver, in a fixed ladder, once per frame.

---

## Purpose

"LogicClass" is two substrate roles plus one driver function, not a single class:

1. **Active-object registry** — a `DynamicVectorClass<ObjectClass*>` **singleton** at `0x0087F778`
   that owns the membership set + ORDER of objects that receive per-tick AI. Tail-append register,
   order-preserving compacting remove, per-object membership bit.
2. **Per-tick scheduler / driver** — `LogicClass::PerTickUpdate @ 0x0055AFB0` runs a **fixed ladder**
   of global subsystems (tiberium growth/spread, bombs, teams, lasers, lightning, radiation, EMP, …)
   and **one ordered live-vector object-AI pass** where each registered object's *entire* per-frame
   update is a single `vtable+0x5C` call, in insertion order, with the active count re-read each
   iteration.

The driver is the per-tick scheduler of the whole map: it is the single thing `Main_Tick` calls to
"advance the world one frame." Its rung order — and the RNG draw order that order produces — is the
lockstep-critical contract.

**Explicitly NOT LogicClass** (commonly conflated):
- `Process_Command @ 0x0055DEE0` — keyboard/command dispatcher (old `LogicClass::AI` label is wrong;
  `ECX = &keycode` stack local, hotkey handler table). Unrelated to the singleton.
- `LayerClass` / `g_DisplayLayers @ 0x008A0360` — the z-sorted **draw** list (render-only, distinct
  BSS instance, same `DynamicVectorClass` base). Walked by `Tactical_ObjectRenderingLoop @ 0x006D8DB0`.

---

## Owns

- **Singleton vector `0x0087F778`** (24 bytes, `DynamicVectorClass<ObjectClass*>`):
  - `+0x00` vtable ptr (`= 0x007E18FC`)
  - `+0x04` `ObjectClass** Items` (the live-object array)
  - `+0x08` `int Capacity`
  - `+0x0C` `IsAllocated` (owns-array; byte at `+0x0D`)
  - `+0x10` `int ActiveCount` (the per-tick loop bound, re-read each iteration)
  - `+0x14` `int GrowthStep`
- **Per-object membership bit `ObjectClass+0x98`** = "currently in the Logic vector." Distinct from
  InLimbo `+0x81`, IsAlive `+0x90`, UniqueID `+0x10`. NOT serialized — rebuilt from vector contents
  on load.
- **The object processing ORDER itself** = reveal-call chronology (no sort). This order is lockstep
  state (hashed). Initial order at map load = section order Terrain → Units → Aircraft → Infantry →
  Structures → Smudge (`ScenarioClass::Full_Init @ 0x00686B20`), then per-section INI index, then
  per-entry Unlimbo tail-append.
- **The fixed global-rung ladder** (§2.6 of the study) and its RNG draw order
  `B→C→E→J→N(per-object)→P→R→U`.

---

## Key functions & globals (addresses)

| Symbol | Addr | Role |
|---|---|---|
| `LogicClass::PerTickUpdate` | `0x0055AFB0` | the driver — fixed global-rung ladder + the one object-AI pass |
| `Main_Tick` | `0x0055D360` | **sole caller** of PerTickUpdate (call site `0x0055DC9E`); late frame-counter increment at `0x0055DE7E` |
| object loop | `0x0055B608..0x0055B619` | the forward `+0x5C` fan-out; count re-read at `0x0055B613` |
| Register (Add, vtable `+0x1C`) | `0x0055BAA0` | idempotent `+0x98` guard → `Insert(tail, sorted=0)` → set `+0x98` |
| Remove | `0x0055BAE0` | gate `+0x98`; index-of `vtable+0x10`; order-preserving left-shift compaction (`0x0055BB11`); decrement count; clear `+0x98`; no tail-zero, no index repair |
| Insert (DynVec) | `0x005519B0` | tail-append at old count; auto-grow via Resize slot |
| Save | `0x00551B20` | write count + each element ptr in array order (orchestrator `0x0067D300 @ 0x0067D435`) |
| Load | `0x00551B90` | read count, tail-append in saved order, swizzle each slot `FUN_006cf240` (orchestrator `0x0067E730 @ 0x0067E8D2`) |
| vtable | `0x007E18FC` | container vtable; **has no +0x5C slot** (the `+0x5C` AI dispatch is on the *element* objects' vtables) |
| singleton | `0x0087F778` | the active-object vector |

**Per-tick ladder rungs (callees that create cross-service edges):**

| Rung | Callee @ addr | Touches service |
|---|---|---|
| A | `TechnoClass__ProcessCellAction 0x6E53A0`, `RecalcBridgeShroudFlags 0x578100`, IC/chrono/psychic timers | techno-foot, bridge-helpers, cell-map |
| B | `TiberiumClass__GrowthDriver_AllTypes 0x00722C40` | cell-map (ore growth) |
| C | `TiberiumClass__SpreadDriver_AllTypes 0x007221B0` | cell-map (ore spread) |
| D | `BombClass__UpdateAll 0x00438BF0` | damage-helpers |
| E | `FUN_0054E4D0` (30-frame timer batch) | random-scenario (RNG) |
| F | teams `g_TeamClass_Array +0x5C` | frontier-ai |
| G | disk-lasers `g_DiskLaserClass_Array +0x5C` | drawing-helpers / damage-helpers |
| I | `LaserDrawClass__UpdateAllAI 0x00550150` | drawing-helpers |
| J | `LightningStorm__Process 0x0053A6C0` | random-scenario (RNG), damage-helpers |
| K | radiation sites `DAT_00B04BD4 +0x5C` | damage-helpers |
| L | `FUN_00554D50` (cell relight/terrain cache) | cell-map |
| M | `EMPulseClass__UpdateAll 0x004C54A0` | techno-foot |
| **N** | **inline object-AI vector pass** (`+0x5C` per object) | **techno-foot, abstract-object, mission-radio** |
| P | wave splash `Wave_splash_forces 0x0053CBE0` | damage-helpers |
| Q | `AlphaShapeClass__PurgeDisabled 0x00420E90` | drawing-helpers |
| R | `MapClass__UpdateCrateRegenTimers 0x0056BBE0` | cell-map, random-scenario |
| S | tactical AI `g_Tactical->+0x5C` | frontier-render (out-of-sim) |
| T | factories `g_FactoryClass_Array +0x5C` | factory-house |
| U | houses `g_HouseClass_Array +0x5C` | factory-house, frontier-ai |
| V | recenter `DisplayClass__GetLastRefObject → FUN_006D6070` | frontier-render (out-of-sim) |

---

## Tick / render position

LogicClass IS the tick position authority — it does not sit *in* an order, it *is* the order.
`Main_Tick @ 0x0055D360` calls `PerTickUpdate @ 0x0055AFB0` exactly once per frame (call site
`0x0055DC9E`), then increments the frame counter LATE (`0x0055DE7E`). So the whole tick reads the
**pre-increment** frame counter.

Within PerTickUpdate the object-AI pass (rung **N**) is sandwiched: tiberium growth/spread, bombs,
teams, lasers, lightning, radiation, EMP run **before** N; anims (SKIPPED in skirmish modes 0/5),
wave, alpha-shape, crate, tactical, factories, houses, last-ref run **after** N. Confirmed RNG draw
order within a tick: **B → C → E → J → N(per-object) → P → R → U**.

Maps to the Rust project's stated sim tick order (`World::advance_tick`): commands → ground movement
→ air/special movement → vision → power → turrets+combat → retaliation+passengers → scatter +
production + repairs + docks + ore growth → AI → defeat → building anims + cleanup → state hash. The
study's headline DRIFT is that Rust's ~22 phased stable-id passes do not reproduce native's *single
ordered insertion-order fan-out*, and ore growth/spread runs late in Rust vs. early (rungs B/C) in
native.

---

## Depends-on (outgoing edges)

Each edge: the OTHER service this one calls into / reads, the via-symbol, and evidence. Most are
"the driver invokes this rung," which is the defining structural relationship — the scheduler owns
the order, the rung owns the work.

- **abstract-object** — via `ObjectClass::Reveal 0x005F4EC0` / `Conceal 0x005F4D30` / `UnInit
  0x005F65F0` / `Destructor 0x005F3B80` are the register/unregister triggers (they call
  `0x0055BAA0` / `0x0055BAE0`); and via the per-object whole-frame AI dispatch in rung N
  (`+0x5C` on each `ObjectClass`-derived element). Evidence: §2.3–2.4, callers of `0x0055BAA0`/
  `0x0055BAE0` = Reveal/Conceal/Destructor; loop `0x0055B608..0x0055B619`.
- **techno-foot** — via rung N's `+0x5C` object-AI fan-out (per-class AI heads
  `0x007360C0`/`0x0051BAB0`/`0x0043FB20`/`0x00414BB0`/`0x00423AC0` = unit/infantry/building/aircraft
  AI), rung A `TechnoClass__ProcessCellAction 0x6E53A0`, rung M `EMPulseClass__UpdateAll 0x004C54A0`.
  Evidence: §1 R3, §2.6 rungs A/M/N.
- **mission-radio** — via rung N: each object's whole-frame `+0x5C` call runs its mission FSM /
  radio update inside the single object pass (e.g. `InfantryClass::DoType_Sequencer 0x00520AE0`
  advancing mission/death sequences). Evidence: §2.6 N, §9 death-to-limbo (DoType sequencer in the
  object pass).
- **cell-map** — via rung B `TiberiumClass__GrowthDriver_AllTypes 0x00722C40`, rung C
  `TiberiumClass__SpreadDriver_AllTypes 0x007221B0`, rung L `FUN_00554D50` (cell relight/terrain
  cache), rung R `MapClass__UpdateCrateRegenTimers 0x0056BBE0`, rung A `RecalcBridgeShroudFlags
  0x578100`. Evidence: §2.6 rungs A/B/C/L/R.
- **factory-house** — via rung T factories (`g_FactoryClass_Array +0x5C`, live count) and rung U
  houses (`g_HouseClass_Array +0x5C`, null-guarded, live count). Evidence: §2.6 rungs T/U.
- **random-scenario** — the ladder owns the per-tick RNG draw order
  `B→C→E→J→N→P→R→U`; rungs that consume RNG: B/C (`Random__Next 0x65C780`), E
  (`RateTimer 0x4C93D0`), J (`RandomRanged 0x65C7E0`), N (per-object), P (area damage), R
  (place at random), U (AI brain). ScenarioClass cell-action timers in rung A. Evidence: §2.6 RNG
  column + "Confirmed RNG draw order"; §5 C-LADDER 11.
- **damage-helpers** — via rung D `BombClass__UpdateAll 0x00438BF0`, rung J `LightningStorm__Process
  0x0053A6C0`, rung K radiation sites (`DAT_00B04BD4 +0x5C`), rung P wave splash
  `Wave_splash_forces 0x0053CBE0`, and rung N (combat ReceiveDamage runs inside each object's pass).
  Evidence: §2.6 rungs D/J/K/P.
- **drawing-helpers** — via rung I `LaserDrawClass__UpdateAllAI 0x00550150`, rung G disk-lasers, rung
  Q `AlphaShapeClass__PurgeDisabled 0x00420E90` (these AI-tick draw-effect objects; the actual blit
  is the render pass). Evidence: §2.6 rungs G/I/Q.
- **bridge-helpers** — via rung A 120-frame `RecalcBridgeShroudFlags 0x578100` (bridge shroud flag
  recompute). Evidence: §2.6 rung A.
- **frontier-ai** — via rung F teams (`g_TeamClass_Array +0x5C`) and rung U house AI brain. Evidence:
  §2.6 rungs F/U. (Project rule: AI not yet in scope — flagged but not implemented.)
- **frontier-render** — via rung S tactical AI (`g_Tactical->+0x5C`) and rung V last-ref recenter
  (`FUN_006D6070`); both explicitly **out-of-sim** (render/UI layer), to be matched in the render/app
  layer never inside `sim/`. Evidence: §3 "Out-of-sim", §2.6 rungs S/V.
- **frontier-net** — Main_Tick (the caller) gates `Network_Keepalive` and MP ambient RNG spend on
  `g_GameMode==4`; the desync caution (§4.2 #6) is a net-layer design constraint. The driver itself
  is net-agnostic; the edge is via Main_Tick, not PerTickUpdate. Evidence: §3 mode-gates, §4.2 #6.

---

## Used-by (incoming edges)

LogicClass is the spine, so nearly every per-tick service is a dependent — it is *invoked from* the
driver in the ladder. The defining incoming edges:

- **abstract-object** — the lifecycle chokepoint (`reveal/conceal/unlimbo/uninit`) drives
  register/unregister into LogicClass's vector; objects ARE the elements of the order. (Reciprocal
  with the depends-on edge.)
- **techno-foot** — every unit/infantry/building/aircraft gets its whole-frame AI run *by* the
  LogicClass object pass (rung N). The object-AI dispatch order is LogicClass's order.
- **mission-radio** — mission FSM / radio updates execute inside the LogicClass object pass; same-tick
  read-after-write across objects (C-SCHEDULER 9) is the LogicClass-defined contract a mission relies
  on.
- **factory-house** — production (T) and economy/house (U) ticks run *only* because LogicClass calls
  their global-array `+0x5C` rungs each frame, in the fixed post-object slot.
- **cell-map** — ore growth/spread/crate-regen/relight run *only* as LogicClass rungs; their
  macro-position in the tick is LogicClass-defined (the "ore-before-objects" parity item).
- **random-scenario** — the RNG cursor advances in exactly the order LogicClass's ladder draws; any
  reorder of a LogicClass rung shifts every later RNG result (lockstep). RandomClass/ScenarioClass is
  a downstream consumer of the LogicClass-defined draw order.
- **damage-helpers / drawing-helpers / bridge-helpers** — their global drivers (bombs, lightning,
  rad, wave, lasers, alpha-shape, bridge-shroud) tick only as LogicClass rungs in fixed slots.
- **frontier-ai / frontier-render / frontier-net** — teams/house-AI (F/U), tactical/last-ref
  (S/V, out-of-sim), and the net-layer Main_Tick gating all hang off the LogicClass driver / its
  caller.

---

## Open / unverified edges

- **Rung F (teams), G (disk-lasers), H (particles), K (radsites), S (tactical), T (factories),
  U (houses)** RNG-consumption columns are marked **UNCHECKED** in §2.6 — the *order* is verified,
  the RNG draw inside several rungs is not.
- **Producer-class identity (YELLOW, §9):** `FUN_0054E4D0`, `FUN_005FF390`, `FUN_00554D50` — what
  registers into their arrays — and `TeamClass +0x5C @ 0x006E9140` body (no Ghidra function defined).
  These edges' exact target-service field reads are unconfirmed.
- **vtable `+0x28` (0x0055B880)** operates on a *different* sentry vector `DAT_008b40cc/d8`, not the
  element array; its LogicClass-slot semantics are **UNCHECKED** (§2.2).
- **garrison_original_owner / civilian-revert** edge to factory-house (§7 #4) — whether civilian
  revert timing depends on building-vs-infantry vector position is **UNCHECKED** (verify the
  civilian-revert path before retiring the Rust workaround).
- **Two-stream RNG split** (`g_MainRng` vs `Scen->Random`) — designed not implemented; the exact
  random-scenario edge cursor parity is unproven until split (§4.2 #7, §9).
- **frontier-net desync edge** — no live per-frame state-hash compare exists in native `Main_Tick`
  (§4.2 #6); the Rust `state_hash` net behavior is a design caution, not a verified native edge.
