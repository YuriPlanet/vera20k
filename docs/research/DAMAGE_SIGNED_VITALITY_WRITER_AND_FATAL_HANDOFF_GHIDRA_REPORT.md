# Damage Signed Vitality Writer and Fatal Handoff

**Date:** 2026-07-23  
**Mode:** coverage-map reinvestigation  
**Status:** **BOUNDED COMPLETE for a shadow-writer contract; full damage G1 remains FAILED**  
**Binary:** active Yuri's Revenge `gamemd.exe`, image base `0x00400000`  
**Implementation change:** none; this report changes no Rust source

## 1. Verdict

The narrow next step is now defined well enough to implement **shadow-first**, but it
is not ready for an authority flip.

The active binary has one common signed HP transaction in
`ObjectClass::ReceiveDamage @ 0x005F5390`. The direct common-receiver funnel is:

```text
Aircraft ─┐
Unit ─────┼─> Foot ─> Techno ─> Object
Infantry ─┘

Building ───────> Techno ─> Object
Terrain ──────────────────> Object
```

The common Object transaction owns signed damage writeback, negative healing,
healing cap, overkill writeback, HP commit, condition classification, callbacks,
trigger rereads, the exact-zero death result, and the first destruction callback.
It does **not** universally own final removal. Each concrete wrapper decides whether
fatal result `4` is retained, crashes, sinks, enters a death sequencer, becomes
PostMortem result `5`, or calls `UnInit` inline.

This closes the mechanism needed for an exact **signed vitality shadow writer**:

- exact state can be `i32 current / i32 maximum`;
- a signed mutable damage value must survive the transaction;
- HP and lifecycle must remain independent;
- the wrapper, not the vitality store, owns the fatal lifecycle handoff; and
- Terrain must not be silently folded into `GameEntity` vitality because Rust stores
  live Terrain objects in a separate authority.

It does **not** pass the full cutover plan's G1. Full receiver authority remains
blocked by the already-recorded readiness/provenance fields, opaque packet argument,
Techno common-postlude state, concrete presentation/RNG helpers, Building sound
fallback, and other authority-visible unresolved rows. Those gaps do not prevent
representation work or diagnostic mirroring, but they prevent live receiver
authority.

## 2. Scope

### In scope

- signed `ObjectClass` damage/healing HP transaction;
- direct common-receiver caller census;
- exact seven-argument forwarding boundary;
- result `4`/`5` fatal handoff for Foot, Infantry, Unit, Aircraft, Building, and
  Terrain;
- `ObjectClass::UnInit` ordering relevant to the handoff;
- current Rust live writer shape;
- the staged `feature/gsi-08-10-damage-authority` shadow representation; and
- the next safe implementation boundary.

### Out of scope

- full Techno modifier/gate certification;
- exhaustive concrete presentation, sound, particle, and RNG effects;
- projectile/impact scheduler authority;
- retail Oracle capture;
- all non-damage health writers in the whole executable;
- full readiness/ammo authority;
- veterancy and kill-credit authority; and
- a Rust implementation or authority flip.

## 3. Evidence and confidence

| Claim | Evidence | Confidence | Active YR |
|---|---|---|---|
| Object owns the common signed HP transaction | live `decompile_function(0x005F5390)` and `disassemble_function(0x005F5390)` | HIGH | yes |
| Techno delegates to Object and owns common aftermath/PostMortem | live `decompile_function(0x00701900)` plus `DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md` | HIGH for transaction position; PARTIAL for full tail | yes |
| Object has only Techno and Terrain direct code callers | live `get_function_callers(0x005F5390)` | HIGH for static direct calls | yes |
| Techno has only Foot and Building direct code callers | live `get_function_callers/get_xrefs_to(0x00701900)` | HIGH for static direct calls | yes |
| Foot is reached by Aircraft, Unit, and the Infantry body call at `0x00518042` | live `get_xrefs_to(0x004D7330)` and `get_assembly_context` | HIGH | yes |
| All concrete Techno entries forward one seven-field packet | live assembly contexts at `0x004165EC`, `0x00737D52`, `0x00518042`, `0x004D742C`, and `0x00442425` | HIGH | yes |
| Fatal membership policy is class-specific | fresh wrapper decompilation/assembly plus [DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md) | HIGH for handoff; PARTIAL for all effects | yes |
| Terrain directly calls Object and UnInit inline on fatal result `4` | live `decompile/disassemble 0x0071B920`, context at `0x0071BB73` | HIGH | yes when Wood/non-Immune gates pass |
| Object UnInit calls Limbo, clears alive, then queues deferred physical deletion | live `decompile/disassemble 0x005F65F0` | HIGH | yes |
| Rust ordinary combat remains unsigned and batched | `src/sim/combat/mod.rs` current source | HIGH | implementation fact |
| The staged exact vitality bundle is non-authoritative and unused by production writers | branch worktree `src/sim/entity_state/{mod,access}.rs` plus call-site search | HIGH | implementation fact |

The local Ghidra name at `0x00517FA0` is boundary-drifted: Ghidra did not return a
function for that exact address, while the raw vtable evidence in the prior receiver
report and the live call instruction at `0x00518042` identify the Infantry receiver
body. This report relies on address/body flow, not the missing function label.

## 4. Common signed Object transaction

`ObjectClass::ReceiveDamage @ 0x005F5390` receives a pointer to a signed damage
integer. The following order is authority-relevant:

1. Signed entry Health at Object `+0x6C` is read and `Health <= 0` exits.
2. A null/zero damage request exits before the kernel.
3. ObjectType `Immune` exits unless the bypass input permits continuing.
4. The damage kernel runs and its signed result is written back through `pDamage`.
5. For a Building whose type has `CanC4=false`, the post-kernel signed damage is
   floored to `1`. This floor also converts a zero or negative kernel result to one
   point of damage.
6. A remaining negative value is healing:
   - new HP is `oldHealth - negativeDamage`;
   - HP is capped to signed Strength at ObjectType `+0xA0`;
   - virtual callback `+0x148(7)` runs only when HP actually changed;
   - result is `0`; and
   - the signed normalized request remains visible through `pDamage`.
7. A positive value is capped inclusively to entry Health when
   `damage >= entryHealth`; the capped amount is written back through `pDamage`.
8. The Yellow transition uses the integer half-Strength boundary. Red uses the
   Rules double threshold. Their ordering and result precedence must be preserved.
9. HP is committed as signed `entryHealth - normalizedDamage`.
10. The special Cyborg rescue path may rewrite fatal state before the final death
    decision.
11. Trigger/callback work is synchronous. Later checks reread live fields instead of
    trusting the entry snapshot.
12. Only exact `Health == 0` reaches the common death gate.
13. Kill attribution selects the attacking object callback or source-House callback
    according to the verified identity branch.
14. Virtual `+0xDC(1)` is invoked and result `4` is returned.
15. The common function does not itself impose one universal `UnInit` policy.

Fresh assembly anchors include the entry gates at `0x005F53A1..0x005F53CD`, kernel
writeback at `0x005F540F..0x005F5414`, Building floor at
`0x005F5448..0x005F5454`, healing at `0x005F5468..0x005F548F`, overkill writeback
at `0x005F54A4..0x005F54C2`, HP commit at `0x005F5505..0x005F550D`, and exact-zero
death handling at `0x005F5765..0x005F57AF`.

### Consequence for Rust state

An unsigned event or `u16::saturating_sub` cannot represent this transaction:

- negative healing disappears;
- signed mutable damage writeback disappears;
- Building's post-kernel floor cannot be represented correctly;
- overkill reports the queued amount instead of remaining HP;
- exact callback timing is lost; and
- lifecycle cannot be selected from the native result code at the native wrapper
  position.

## 5. Direct receiver funnel census

### 5.1 Common calls

Live caller/xref checks found:

| Callee | Direct code callers |
|---|---|
| `ObjectClass::ReceiveDamage @ 0x005F5390` | `TechnoClass::ReceiveDamage @ 0x00701900`; `TerrainClass::Take_Damage @ 0x0071B920` |
| `TechnoClass::ReceiveDamage @ 0x00701900` | `FootClass::ReceiveDamage @ 0x004D7330`; `BuildingClass::ReceiveDamage @ 0x00442230` |
| `FootClass::ReceiveDamage @ 0x004D7330` | `AircraftClass::ReceiveDamage @ 0x004165C0`; Infantry body call `0x00518042`; `UnitClass::ReceiveDamage @ 0x00737C90` |

The corresponding vtable slots remain the virtual entry points used by gameplay.
The table above proves the internal common-call funnel, not that every runtime
invocation is a static direct call.

### 5.2 Packet forwarding

The packet order remains:

```text
pDamage, distance, warhead, attacker, arg5, arg6, sourceHouse
```

Fresh assembly shows seven pushes immediately before each common call. The semantic
identity of every field is not upgraded here: the unresolved argument remains opaque.
The implementation rule is therefore to carry it losslessly and in order, not to
rename it with guessed behavior.

No wrapper is allowed to:

- narrow `pDamage` to unsigned;
- copy the packet and discard writeback;
- reorder source identities;
- recompute the kernel independently; or
- infer fatal removal solely from HP.

## 6. Fatal result and lifecycle handoff matrix

| Receiver | Result `5` | Result `4` handoff | Membership on wrapper return |
|---|---|---|---|
| Foot | immediate return | no direct UnInit in Foot; concrete class continues | unchanged by Foot |
| Infantry | immediate return | ordinary death enters retained death sequencer; verified exceptional Cyborg branches may UnInit inline or retain a successful crash | class/branch dependent |
| Unit | immediate return | ordinary fatal path reaches inline UnInit; sinking or successful crash paths retain the object | class/branch dependent |
| Aircraft | immediate return | UnitLost/destruction animation, then crash helper; failed/grounded crash calls UnInit inline, successful crash retains | class/branch dependent |
| Building | retained PostMortem path | ordinary duration-positive path calls UnInit inline; Selling/Explodes duration-zero path stays live until Building Update | class/branch dependent |
| Terrain | immediate return | fatal terrain effects, virtual `+0xDC(1)`, then virtual `+0xF8` UnInit inline | removed from live membership before return |

Fresh handoff anchors:

- Aircraft inline UnInit call: `0x004166A3`.
- Infantry conditional inline UnInit calls: `0x0051861D` and `0x00518B9A`.
- Building inline UnInit call: `0x0044269A`, dominated by the verified positive
  duration test.
- Unit conditional UnInit calls include `0x0073847F` and `0x007384A5`; the
  `0x0073819B` call belongs to a failed passenger/ejection subpath, illustrating
  why all `+0xF8` calls cannot be collapsed into one "unit dies" action.
- Terrain Destroy then UnInit: `0x0071BB63..0x0071BB73`.

Result `4` is therefore a **fatal receiver result**, not a universal deletion command.
Result `5` is not delayed result `4`; it is a distinct retained PostMortem outcome.

## 7. Object UnInit contract at the handoff boundary

`ObjectClass::UnInit @ 0x005F65F0` performs:

1. defuse an attached bomb when present;
2. for Foot objects, run the passenger EMP/cleanup call;
3. call the common cleanup/detach helper at `0x007258D0`;
4. call virtual Limbo at vtable `+0xD4`;
5. clear Object `IsAlive` at `+0x90`;
6. append the object pointer to the pending-delete vector when the append path
   succeeds; and
7. return while physical destruction remains deferred.

Assembly order is pinned at `0x005F65F3..0x005F667D`. Conceal/Logic removal occurs
through Limbo before the alive byte is cleared and before pending-delete append.

This establishes two separate timings:

- **representation removal** may be synchronous inside the damage wrapper; and
- **physical storage deletion** is deferred through the pending-delete list.

Rust's lifecycle authority can model this cleanly, but damage must call it at the
class-specific native handoff position.

## 8. Terrain is a separate vitality authority

The prior concrete-Techno reconciliation did not headline Terrain because Terrain is
not a Techno subclass. The direct Object caller census proves it still participates in
the common signed Object HP transaction.

`TerrainClass::Take_Damage @ 0x0071B920`:

1. rejects null warheads;
2. requires Warhead `Wood`;
3. rejects TerrainType `Immune`;
4. forwards the same seven-field packet directly to Object;
5. returns immediately for result `5`;
6. on result `4`, runs its terrain-specific fatal effects;
7. calls the destruction callback and then UnInit inline.

The `SpawnsTiberium` branch has additional fatal effects, but stock
`TIBTRE01..03` declare both `SpawnsTiberium=yes` and `Immune=yes` in
`ini/rules.ini`. Therefore the branch is active engine behavior while stock
Tiberium Trees remain gated from ordinary Wood damage. Non-Immune terrain and
modified rules can reach it.

Current Rust already stores Terrain separately in
`ProductionState::terrain_objects` as `TerrainObjectState { health: i32,
max_health: i32, lifecycle }`. Its damage helper:

- accepts only positive `base_damage`;
- performs a separate Verses calculation;
- uses saturating subtraction;
- immediately limbos/destroys the Terrain object; and
- does not run the common Object signed transaction.

The GameEntity shadow migration must therefore either:

1. explicitly exclude Terrain and preserve a separate Terrain writer contract; or
2. later introduce a shared pure Object vitality transaction callable by both stores
   without merging their ownership.

Option 2 is the better long-term mechanism. It shares behavior without pretending
Terrain is a `GameEntity`.

## 9. Current Rust disparity map

### 9.1 Main `dev` worktree

Ordinary combat currently:

- carries `damage_events: Vec<(u64, u16, u64, InternedId)>`;
- loops the whole event batch;
- applies `target.health.current.saturating_sub(damage)`;
- accumulates every zero-HP target in `dead_entities`;
- waits until a later death phase to classify animation versus immediate UnInit; and
- returns `immediate_uninit_ids` for World to process after combat.

Relevant current sites are `src/sim/combat/mod.rs:1172`,
`src/sim/combat/mod.rs:1829..1914`, and the separate death-AoE HP write at
`src/sim/combat/mod.rs:1078`.

This differs from the binary in mechanism and same-tick visibility:

- damage is unsigned;
- healing is absent;
- damage writeback is absent;
- multiple events can continue to touch an already-zero target in the batch;
- class-specific fatal handling is deferred and simplified;
- `has_animation` is used as a transitional death-lifetime selector; and
- later native-ordered targets do not observe inline wrapper removal at the same
  point.

Other production HP writers bypass ordinary combat, including:

- C4: `src/sim/world/world_orders.rs:873`;
- Lightning Storm: `src/sim/superweapon/lightning_storm.rs:266`;
- Genetic Converter: `src/sim/superweapon/genetic_converter.rs:160,199`;
- Iron Curtain Infantry kill: `src/sim/superweapon/iron_curtain.rs:56`;
- crush: `src/sim/movement/movement_tick.rs:1794`;
- bridge-collapse force kill: `src/sim/world/bridge_orchestrator.rs:1048`;
- aircraft self-destruction/despawn: `src/sim/aircraft/mod.rs:650,798`;
- repair-depot healing: `src/sim/docking/building_dock.rs:372`; and
- building repair: `src/sim/production/production_sell.rs:844`.

Some of these are legitimate non-receiver lifecycle or repair owners. Others should
eventually enter the receiver. They must be classified before any vitality authority
flip; a grep-only list is not yet a semantic writer inventory.

### 9.2 Staged damage-authority worktree

The `feature/gsi-08-10-damage-authority` worktree contains:

- `VitalityState { current: i32, maximum: i32 }`;
- a non-authoritative `EntityStateShadow`;
- snapshot/hash exclusion;
- post-load shadow rebuild;
- diagnostic comparison classes; and
- exact-candidate test seams.

However, production call-site search finds no use of `mirror_vitality`,
`set_vitality_candidate`, `compare_vitality`, or `exact_vitality` outside tests and
constructor checks. Live gameplay still reads and writes legacy `Health { u16, u16 }`.

The completed code is therefore **representation substrate**, not writer migration.
That is the correct staged state, but it must not be reported as signed damage
authority.

## 10. Coverage ledger

| Row | Question | Status | Disposition |
|---|---|---|---|
| C1 | Exact common HP writer entry identified? | RESOLVED | Object `0x005F5390` |
| C2 | Signed damage writeback identified? | RESOLVED | kernel and overkill both write `pDamage` |
| C3 | Negative healing and cap identified? | RESOLVED | signed subtract-negative, cap to Strength |
| C4 | Healing callback timing identified? | RESOLVED | callback `+0x148(7)` iff HP changed |
| C5 | Exact-zero fatal gate identified? | RESOLVED | `Health == 0`, after synchronous work |
| C6 | Direct common receiver funnel complete? | RESOLVED | Techno plus Terrain; concrete Techno chain enumerated |
| C7 | Concrete packet forwarding preserved? | RESOLVED | seven pushes at every common call |
| C8 | Every packet field semantically named? | DEFERRED | opaque argument remains a full-G1 blocker; carry unchanged |
| C9 | Fatal wrapper handoff identified per class? | RESOLVED | matrix in §6 |
| C10 | Final physical deletion timing identified? | RESOLVED | UnInit queues deferred delete |
| C11 | Terrain included? | RESOLVED | direct Object caller, separate Rust store |
| C12 | Full wrapper effects/RNG certified? | DEFERRED | full G1 remains failed |
| C13 | Rust signed representation exists? | RESOLVED | shadow branch only |
| C14 | Rust production writer migration exists? | RESOLVED as absent | no production facade call sites |
| C15 | All Rust HP writers semantically classified? | DEFERRED | required before cutover |
| C16 | Retail-derived parity check exists? | DEFERRED | required before authority |

## 11. Tiny-detail ledger

1. Entry Health is a signed dword, not unsigned.
2. Dead/nonpositive entry Health exits before mutation.
3. Zero request exits before kernel work.
4. Kernel output is written back through the original damage pointer.
5. Building `CanC4=false` floors the post-kernel value to `1`.
6. That Building floor also defeats a negative healing result.
7. Healing uses subtract-negative, not a separate unsigned add API.
8. Healing caps to signed Strength.
9. Healing callback runs only when HP changed.
10. Positive overkill comparison is inclusive.
11. Overkill writes remaining entry HP back as the applied damage.
12. Yellow classification uses integer half Strength.
13. Red classification uses the Rules double threshold.
14. HP commit precedes later rescue/trigger/death decisions.
15. Synchronous callbacks require fresh field rereads.
16. Only exact zero reaches common death handling.
17. Result `4` does not mean universal immediate UnInit.
18. Result `5` is a retained PostMortem outcome.
19. Foot performs no universal lifecycle removal.
20. Aircraft allocation precedes its destruction-list selection draw.
21. Unit and Aircraft retention choices are not interchangeable.
22. Ordinary Infantry death may remain in live order for its sequencer.
23. Selling/Explodes Building death may remain live at zero HP until Update.
24. Terrain calls Object directly rather than through Techno.
25. Terrain fatal result calls destruction before UnInit.
26. Limbo precedes IsAlive clear in Object UnInit.
27. Pending-delete append follows representation removal.
28. Rust Terrain state is signed but behaviorally separate.
29. Rust GameEntity shadow is signed but currently non-authoritative.
30. Snapshot/hash exclusion of the shadow is required until cutover.

## 12. Safe implementation handoff

### 12.1 Next slice: signed transaction plumbing in shadow

The next code slice may:

1. introduce a signed damage packet/result representation that preserves the mutable
   damage writeback;
2. add a pure Object-vitality transition function for the verified rows in §4;
3. compute that transition in shadow without changing live HP, control flow, RNG,
   lifecycle, snapshots, or hashes;
4. classify comparison results as equality, expected representation gap, semantic
   divergence, or uncomparable;
5. include synthetic negative-healing, Building-floor, exact-overkill, callback-intent,
   and exact-zero cases; and
6. expose an explicit lifecycle intent in the shadow result without executing it.

The pure result must carry enough ordered facts for a later executor, at minimum:

```text
normalized_signed_damage
old_health
new_health
maximum_health
receiver_result
health_changed
health_callback_intent
kill_attribution_intent
destroy_callback_intent
fatal_wrapper_handoff_required
```

This is a behavioral transaction result, not a second lifecycle authority.

### 12.2 Writer migration rhythm

For each live writer:

1. classify whether it is receiver damage, repair/healing, or a distinct lifecycle
   force-kill;
2. preserve its native owner rather than routing everything through one generic HP
   helper;
3. mirror the legacy write and exact shadow from one computed operation;
4. compare without affecting gameplay;
5. record mismatches by writer/operation; and
6. keep direct legacy readers authoritative.

Do not make `mirror_vitality` a blind replacement for every assignment. A crush
force-kill, Building repair tick, Terrain Wood hit, and ordinary weapon receiver are
different native mechanisms even when they all change an HP field.

### 12.3 Lifecycle integration

The shadow transaction may report a fatal result, but it must not:

- set `dying`;
- conceal;
- remove Logic membership;
- call UnInit;
- append pending delete; or
- choose animation/crash/sink/PostMortem lifetime.

Those actions stay with the concrete wrapper/lifecycle executor and occur only at the
verified class-specific point.

### 12.4 Terrain integration

Keep `TerrainObjectState` separately owned. Reuse the pure Object vitality transition
only after the Terrain wrapper supplies its Wood/Immune gates and receives the result
for its own fatal continuation. Do not move Terrain objects into `EntityStore` merely
to share arithmetic.

### 12.5 Authority blockers

No live authority flip until:

- every GameEntity and Terrain HP writer is classified;
- the opaque packet field is resolved or excluded from the exact authority scope;
- full Techno/concrete wrapper G1 gaps are closed;
- callback/trigger execution exists at the required synchronous positions;
- class-specific fatal continuations are implemented;
- projectile/impact timing is authoritative;
- retail Oracle comparisons pass;
- exact state is serialized and world-hashed at one coordinated version bump; and
- unauthorized direct legacy access is mechanically rejected.

## 13. Required validation

Shadow-stage validation:

1. negative damage heals and caps without mutating legacy gameplay;
2. armor-class healing rejection remains distinguishable from a valid negative result;
3. Building `CanC4=false` converts negative/zero to one positive point;
4. `damage == oldHealth` writes back oldHealth and produces exact zero;
5. `damage > oldHealth` writes back oldHealth, not the request;
6. callback intent appears only when healing changed HP;
7. zero HP does not itself alter lifecycle axes;
8. result `4` does not itself execute UnInit;
9. result `5` remains distinct;
10. shadow enable/disable produces identical state hash, snapshot bytes, RNG cursor,
    Logic order, pending-delete order, and gameplay output;
11. Terrain is absent from GameEntity shadow iteration; and
12. production grep confirms the bounded migrated writers and lists every remaining
    direct legacy writer.

These are regression and migration checks. They do not certify gamemd parity. The
authority stage still needs named retail/gamemd-derived executable comparisons.

## 14. Adversarial review

### A1. Could result `4` be simplified to `uninit(id)`?

No. Ordinary Infantry, sinking/crashing Unit, successful Aircraft crash, and
Selling/Explodes Building paths retain live representation after result `4`.

### A2. Could zero HP be the lifecycle trigger?

No. HP, Object alive, limbo, Logic membership, and pending deletion are independent
native axes. Result `5` restores/retains life, while several result-4 wrappers retain
zero-HP objects.

### A3. Could the signed state be authoritative before the full receiver?

No. Exact storage alone does not supply gate order, callbacks, mutable damage
writeback, triggers, wrapper effects, or lifecycle timing.

### A4. Could Terrain be ignored because it is not a GameEntity?

No. It is a live direct caller of the same Object HP transaction. It should remain a
separate store, but it belongs in the behavioral writer inventory.

### A5. Could current batched death handling be equivalent?

Not proven and therefore DRIFT. Native wrappers can remove or retain each target
synchronously inside the receiver. A later batch changes what subsequent operations
can observe and can process repeated events against an already-fatal target.

### A6. Could all direct HP assignments be routed through ReceiveDamage?

No. Repair, crush, Iron Curtain Infantry death, out-of-bounds aircraft cleanup, and
other lifecycle mechanisms may have distinct native owners. Each writer requires
classification before migration.

### A7. Could the unresolved packet argument be dropped for the vitality-only slice?

It may remain semantically opaque during pure shadow arithmetic, but the packet shape
must preserve it unchanged. Dropping it would make later full receiver reconstruction
unsafe and would falsely imply G1 closure.

## 15. Open questions log

| ID | Status | Question | Next step |
|---|---|---|---|
| OQ1 | RESOLVED | What owns signed damage/healing HP writeback? | Object receiver |
| OQ2 | RESOLVED | Is the direct common-receiver funnel complete? | Techno and Terrain callers; concrete Techno chain enumerated |
| OQ3 | RESOLVED | Does fatal result imply one removal policy? | no; use class matrix |
| OQ4 | RESOLVED | Where does physical deletion occur? | deferred after UnInit queues the object |
| OQ5 | RESOLVED | Is Terrain part of this behavior? | yes, as a separate-store direct Object caller |
| OQ6 | DEFERRED | What is the exact semantic identity of the opaque packet argument? | focused full-G1 binary trace |
| OQ7 | DEFERRED | Are all Techno readiness/firepower/house/postlude fields authority-ready? | complete existing G1 blocker rows |
| OQ8 | DEFERRED | Are every concrete wrapper effect and RNG draw closed? | finish concrete receiver investigation/Oracle cases |
| OQ9 | DEFERRED | Which direct Rust HP assignments are receiver writers versus distinct owners? | repository-wide semantic writer inventory |
| OQ10 | DEFERRED | Does the shadow transaction match retail for all required boundaries? | retail Oracle capture and comparison |

All unresolved questions are explicitly deferred and remain authority blockers. None
is silently converted into an implementation assumption.

## 16. Supersession and negative facts

- The concrete damage inventory must include Terrain in addition to the five Techno
  concrete classes.
- The Infantry receiver body remains identified by vtable/body evidence despite the
  missing Ghidra function boundary at `0x00517FA0`.
- Do not equate fatal result `4`, HP zero, `dying`, concealment, UnInit, or physical
  deletion.
- Do not use `has_animation` as the native class-lifecycle selector.
- Do not narrow damage to `u16`.
- Do not treat the signed shadow representation as a completed writer migration.
- Do not merge Terrain ownership into `EntityStore`.
- Do not claim full G1 passed from this bounded closure.

## 17. Sources read

- `DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md`
- [DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md)
- [DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md)
- [DAMAGE_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_MATH_GHIDRA_REPORT.md)
- [ANIMCLASS_BUILDING_OBJECT_DAMAGE_RUNTIME_SPAWNS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_BUILDING_OBJECT_DAMAGE_RUNTIME_SPAWNS_GHIDRA_REPORT.md)
- [TERRAIN_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TERRAIN_CLASS_GHIDRA_REPORT.md)
- [TIBTRE_TERRAIN_OBJECT_LIFECYCLE_AND_SEEDING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_TERRAIN_OBJECT_LIFECYCLE_AND_SEEDING_GHIDRA_REPORT.md)
- [traces/TIBTRE_NONIMMUNE_DAMAGE_REMOVES_SPAWNER_TRACE_2026-05-27.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/TIBTRE_NONIMMUNE_DAMAGE_REMOVES_SPAWNER_TRACE_2026-05-27.md)
- `docs/plans/2026-07-13-damage-authoritative-cutover-plan.md`
- `docs/plans/2026-07-22-entity-state-authority-substrate-design.md`
- current Rust source in `src/sim/combat`, `src/sim/world/lifecycle.rs`,
  `src/sim/terrain_object.rs`, and direct HP-writer call sites
- staged worktree source in `src/sim/entity_state`

Fresh binary evidence was collected read-only on 2026-07-23 from the connected
`gamemd.exe` program through decompile, disassembly, callers, xrefs, instruction
search, and assembly-context queries at the addresses cited above.
