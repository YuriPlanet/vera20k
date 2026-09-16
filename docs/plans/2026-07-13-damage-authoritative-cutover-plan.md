# Authoritative Damage Cutover Staged Master Plan

> **For Codex:** Execute only Tasks 1-4 from this revision. They are the
> self-contained evidence/handoff stage. Tasks 5-24 are non-normative
> implementation sequencing and acceptance criteria until G0a/G0b regenerate
> them with the verified literal fields, mappings, paths, and code shapes. After
> Tasks 1-4 finish, the mandatory G0a `/review-plan` refresh makes Tasks 5-21
> executable by replacing the evidence-dependent fields, mappings, and callback
> shapes with verified results. Task 22 becomes executable only after the later
> G0b integration refresh inserts literal prerequisite paths and symbols. Tasks
> 23-24 are the authority/final-verification stage and
> are hard-gated by the research, projectile-timing, and retail-oracle conditions
> named below.

**Goal:** Replace scattered entity-health calculations with one ordered,
native-shaped damage receiver without changing gameplay until active Yuri's
Revenge evidence proves the new mechanism.

**Architecture:** Exact IEEE inputs live in `rules/` and low-level software
numeric operations live in `util/`. `sim/combat/damage/` owns typed call facts,
pure receiver stages, ordered effect intents, shadow comparison, and the
synchronous resolver. Producers invoke the resolver at their verified native
call points; there is no new global damage phase and `sim/` gains no dependency
on rendering, UI, sidebar, audio, or networking.

**Design Doc:**
`docs/plans/2026-07-13-damage-authoritative-cutover-design.md`

**Plan status:** The lettered child units under Tasks 1-4 are executable now;
the umbrella task headings are synthesis boundaries and must never be assigned
as single work items. Tasks 5-24 are a staged
implementation map and acceptance ledger, not executable instructions in this
revision. G0a must regenerate their evidence-dependent definitions, and G0b
must later insert integration dependency paths. Task 23 Step 0 is the non-
authority-changing Oracle preflight that can pass G3 after the other nine gates
are ready; no authority-
changing Task 23 step, and no Task 24 completion work, may run until all ten
gates pass. This restriction is a correctness result, not a scheduling preference.

---

## Grounding Summary

- `ApplyWarheadDamage` (`0x00489180`) owns falloff, Verses, healing armor
  exclusion, and the signed `MaxDamage` cap.
- The verified CellSpread scale is `256.0f`, not the stale `128` claim.
- The Rules constructor fallback for missing `MaxDamage` is `1000`; both stock
  merged INIs set the live value to `10000`; parsed `MinDamage=1` is not active.
- Strictly positive ordinary attacker damage computes
  `(country firepower × per-unit firepower) × base`, converts once, then applies
  veteran/elite, civilian-garrison, tank-bunker, and open-topped stages with a
  conversion after each enabled later stage. Ordinary non-positive damage skips
  country/per-unit/veteran scaling; the special stored-zero path still executes
  enabled containment stages.
- The containment labels above supersede the stale deploy/garrison/gattling
  labels in the older country/armor report; bunker and open-topped tests can
  both succeed.
- `TechnoClass::ReceiveDamage` (`0x00701900`) applies defender divisors and the
  positive minimum-one rule before the receiver gates.
- `DamageReducesReadiness` mutates ammo/readiness and requests its animation; it
  does not absorb or replace HP damage.
- `AffectsAllies=no` requires a non-null source object before the alliance test;
  an attackerless source-house value alone is not an ally gate.
- `ObjectClass::ReceiveDamage` (`0x005f5390`) owns kernel invocation, the
  building minimum, inclusive overkill, condition transitions, health writes,
  lethal bookkeeping, and ordered trigger calls.
- `BuildingClass::ReceiveDamage` (`0x00442230`) includes RNG-consuming and
  result-dependent wrapper behavior, but its lethal removal call chain must be
  rechecked because an audited vtable read identifies `+0x4EC` as
  `DestructionEffects`, not the stale `Limbo` label.
- `Apply_area_damage` (`0x00489280`) first captures a fixed target-record list,
  then calls each receiver synchronously in recorded order; it does not sort.
- `TechnoClass::Fire_At` (`0x006fdd50`) launches a munition and does not apply
  entity HP damage. A newly appended bullet can still detonate later in the
  same live Logic-vector pass.
- Current Rust instead computes final `u16` damage in fire/radiation/AoE paths,
  defers ordinary HP writes to combat Phase 4, and has independent death-weapon
  and lightning health writes.
- The existing `sim/combat/damage/` service is isolated scaffolding with 37
  useful regression tests, but its `f64`/`as i32` path and incomplete envelope
  are not native parity evidence.
- INI authority includes exact CellSpread/PercentAtMax/Verses values,
  `MaxDamage`, `VeteranCombat`, `VeteranArmor`, double `ConditionYellow`/
  `ConditionRed`, containment
  multipliers, warhead gates, type immunities, veterancy abilities, and country
  multipliers; several are not currently parsed exactly.
- The current worktree has uncommitted overlap in combat tests, world tick
  orchestration, and world hashing, while the Oracle directories are owned by
  another task. Execution must coordinate those files rather than overwrite
  them.

## Authority Gates

The following gates are cumulative. A failed gate stops at the boundary named
below; it does
not permit an approximation.

| Gate | Pass condition | Evidence owner |
|---|---|---|
| G0a — Core executable-plan refresh | After Tasks 1-4, `/review-plan` replaces every receiver/numeric evidence-dependent type field, ability map/default, concrete effect/lifecycle variant, callback checkpoint, and Oracle record with literal verified definitions; the refreshed Tasks 5-21 have no conditional implementation choice | Plan owner; blocks Task 5, but does not wait for G2 |
| G0b — Integration refresh | After the approved G2, GT, GS, GV, and GP plans exist, `/review-plan` replaces every generic dependency file/symbol reference and branch-injection point with literal paths, symbols, and signatures | Plan owner; blocks Task 22 only |
| G1 — Receiver evidence | Tasks 1-3 leave no authority-critical gate, formula, callback, trigger, wrapper, lifecycle transition, target filter, distance conversion, or producer argument marked `UNKNOWN`/`UNCHECKED` for an in-scope route | Damage research docs plus live read-only Ghidra evidence |
| G2 — Projectile timing | An approved and implemented projectile/impact plan exposes damage calls at verified munition detonation scheduler positions, including same-frame appended-bullet behavior | Separate projectile lifecycle design/plan; this plan only owns the damage-call adapter |
| GT — Attached trigger substrate | An approved and implemented trigger plan parses object trigger tags, owns the TriggerGraph/EventMap/ActionMap state, and exposes synchronous native damage-event dispatch for the verified `0x26`-`0x2c` calls including both `0x29` calls | Separate trigger substrate design/plan; this plan owns call order and payload only |
| GS — Entity state substrate | An approved and implemented state plan provides signed i32 current/max Health and Infantry FearLevel, one authoritative Techno ammo/readiness owner, native-bit veterancy/per-unit firepower/per-unit armor state, controller/victim relationships, reconciled last-attacker ownership, and persistent producer provenance when 3C requires it. It inventories and migrates every reader/writer—including crate powerups, firing, docking, periodic radiation, hash, and snapshot paths | Separate entity-state design/plan; this plan owns receiver reads/writes only |
| GV — Kill-credit/veterancy substrate | An approved and implemented veterancy plan reproduces object-versus-house kill attribution, XP calculation/rerouting, promotion, VeteranRatio/Cap inputs, and same-tick visibility to later attacker calculations | Separate veterancy design/plan; this plan owns ordered kill-credit calls only |
| GP — Particle persistence substrate | Every receiver-created authoritative particle-system field is serialized, restored, and completely world-hashed, or live particle creation remains blocked; snapshot load preserves active damage particles and their deterministic update state | Separate particle-state design/plan; this plan owns the create/remove effect call only |
| G3 — Retail oracle | Task 23 preflight reports zero required mismatches for every required hook/case and records the retail binary hash, raw numeric bits, ordered writes/calls, RNG positions, frame cursor, and lifecycle membership; a classified known drift still fails this gate | Coordinated Oracle task handoff plus Rust fixture runner |
| G4 — Integration ownership | No active task owns the authority files; one integration owner is reserved for the authority edit plus hash/snapshot/golden rebaseline; immediately before Task 23 Step 2 that owner re-reads and records the next free snapshot version. Actual rebaseline completion is Task 23 Step 8/Task 24 evidence, not a prerequisite for this reservation gate | Integration coordinator |

### Executable-task granularity

- Tasks 1-3 are umbrella evidence phases. Execute only their lettered child
  units, at most three read-only investigations per wave, then assign one
  separate synthesis/reconciliation unit. No worker owns an umbrella report and
  its source investigations in the same pass.
- G0a must replace the current Task 5, Task 6, and Task 15 umbrella definitions
  with bounded child tasks. At minimum, split bit wrappers, decimal grammar,
  arbitrary-precision decimal rounding, extended decode/narrow, add/sub,
  multiply/divide, comparisons/conversions, non-Building wrappers, Building
  wrapper, and PostMortem/lifecycle work.
- G0b must split Task 22 by producer and Task 23 into Oracle executor preflight,
  runtime serialization/hash, direct/weapon-AoE cutover, death/radiation/
  lightning cutover, legacy removal, and snapshot/baseline ownership. It must
  insert literal G2/GT/GS/GV/GP and snapshot fixture/golden paths rather than asking an executor to
  discover and edit a broad category.
- Every regenerated child task owns a literal file list, one verification
  command at a time, and a nonzero matched-test requirement. If a task cannot be
  described that narrowly, it remains a gate, not executable work.

## Evidence Corrections Applied During Planning

These corrections narrow or amend statements in the approved design. They are
binding for this plan.

1. The three post-veterancy `Fire_At` modifiers are civilian garrison, tank
   bunker, and open-topped. There is no deploy/gattling interpretation in this
   cluster.
2. Readiness reduction is an ordered side effect and HP damage continues; the
   old phrase “ammo absorption” must not appear in code or tests.
3. `AffectsAllies` checks source-object presence. `source_house` remains useful
   for kill credit and producer provenance, but does not substitute for a
   missing source object in the ally gate.
4. Area dispatch is two-stage: capture a fixed target-record list, then resolve
   each record synchronously in native order. This is not a live re-query of
   spatial membership between receivers.
5. Direct and weapon-AoE authority cannot be placed in the current Rust firing
   phase. It belongs at projectile/effect impact after G2 passes.
6. The audited Building vtable identity wins over the stale removal report;
   concrete Building authority remains blocked until Task 2 re-traces the real
   call chain.
7. `Fire_At` scales only strictly positive ordinary `Damage`; it groups
   `(house firepower * per-unit firepower) * Damage` before the first conversion.
   Non-positive ordinary values skip country/per-unit/veteran stages, while the
   special stored-zero branch still runs enabled containment conversions.
8. A missing `Verses` key parses the native 11-token fallback, a present empty
   value retains constructor ones, and a present nonempty list that yields fewer
   than 11 `strtok` tokens reaches a native null dereference. The 0x80-byte
   `ReadString` truncation occurs before trim/tokenization.
9. Generic `ReadDouble` is a `%f`/binary32 prefix parse followed by mandatory f64
   spill/reload boundaries; it is not the Verses atoi/strtod reader.

## Key Technical Decisions

- **Use bit-backed native inputs and an integer-backed x87 subset:** Host
  `f32`/`f64` execution is not used by simulation damage math. — **Confidence:
  high**
  - **Source:** approved design numeric contract; [DAMAGE_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_MATH_GHIDRA_REPORT.md);
    `GATE_DAMAGE_VERSES_F64_RESOLUTION_GHIDRA_REPORT.md`.
- **Keep source object and source house as separate call facts:** This preserves
  alliance-gate semantics, attackerless attribution, and null-source cases. —
  **Confidence: high**
  - **Source:** [WARHEADTYPECLASS_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEADTYPECLASS_REINVESTIGATION_GHIDRA_REPORT.md) and
    `DAMAGE_HELPERS_ENGINE_SUBSTRATE_SERVICE_STUDY.md`.
- **Model readiness reduction as an effect, never a damage multiplier:** HP
  processing continues after the verified readiness mutation. — **Confidence:
  high for identity; medium for arithmetic until Task 1**
  - **Source:** `TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md`; exact arithmetic is a
    G1 item.
- **Capture area targets before receiver dispatch:** The scratch collector
  preserves airborne-first/table/list order and records exact lepton distance;
  it never calculates final damage. — **Confidence: high for two-stage/order;
  medium for filters and special distances until Task 3**
  - **Source:**
    `TARGETDEATH_APPLY_AREA_DAMAGE_LIVE_VECTOR_ITERATION_RESWARM_20260528.md`.
- **Represent the receiver as ordered pure stages plus explicit effect intents:**
  Shadow mode can evaluate those stages without mutating the world or consuming
  RNG; the authoritative executor later commits each intent at its verified
  position. — **Confidence: medium**
  - **Source:** approved design and current Rust ownership boundaries. Task 1
    must determine whether any callback is re-entrant and requires a staged
    refresh. `/review-plan` must recheck this decision before Task 12.
- **Run normal weapon receiver calls at impact, never in `Fire_At`:** Same-frame
  impact remains possible, but only through the live scheduler position. —
  **Confidence: high**
  - **Source:** [L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md) and
    `AAHEATSEEKER2_FIRST_TICK_DAMAGE_LATENCY_GHIDRA_REPORT.md`.
- **Keep runtime shadow diagnostics transient:** They are neither serialized nor
  hashed and never assert, mutate state, consume RNG, or choose gameplay. —
  **Confidence: high**
  - **Source:** approved design AD-5/AD-8 and `Simulation`'s existing skipped
    event-buffer pattern.
- **Add authoritative runtime fields only in the coordinated authority tail:**
  Rule inputs and shadow scratch can land additively; serialized/hashed receiver
  state and the snapshot version move together. — **Confidence: high**
  - **Source:** approved design determinism section and current
    `world_hash.rs`/`snapshot.rs` ownership.
- **Do not reserve snapshot version 26:** Task 23 reads the next free value after
  synchronizing with other sessions. — **Confidence: high**
  - **Source:** current `SNAPSHOT_VERSION = 25` plus dirty hash/RNG work.

## Open Questions

### Resolved During Planning

- **Cell unit:** CellSpread uses 256 leptons per cell; the stale 128 claim is
  discarded.
- **MaxDamage:** Missing key uses 1000; stock merged rules override it to 10000;
  `MinDamage` is not consumed.
- **Attacker containment identity/order:** civilian garrison → tank bunker →
  open-topped, each with its own native conversion; bunker and open-topped can
  stack.
- **Readiness behavior:** damage reduces readiness/ammo as a side effect and
  does not reduce HP damage.
- **Ally gate source:** a source object is required; source house alone does not
  activate `AffectsAllies=no`.
- **Normal-fire timing:** damage belongs to munition/effect impact, not the fire
  call or the current deferred Phase 4 tuple.
- **Building `+0x4EC`:** audited evidence identifies `DestructionEffects`; the
  conflicting synchronous-`Limbo` claim is not implementation authority.

### Deferred to the G1/G2/G3 Gates

- Exact `ignore_defenses` predicates and every Techno gate branch are resolved
  in Task 1.
- Exact readiness arithmetic, ammo field selection, animation call, and
  continuation order are resolved in Task 1.
- Object health callback ownership, healing cap ownership, kill attribution,
  trigger IDs/arguments/order, and both event-`0x29` calls are resolved in Task
  1.
- Missing-key Veteran/Elite ability arrays, Building `ImmuneToPsionics`, the
  general `Immune` flag, and the disputed UnsellableTransport receiver gate are
  resolved in Tasks 1-2.
- PostMortem interpolation, repeated hits, delay timer ownership, and which
  lethal effects survive restoration are resolved in Task 2.
- Infantry, Unit/Foot, Aircraft, and Building wrapper pre/post behavior,
  concrete removal timing, and Building RNG/audio/animation ordering are
  resolved in Task 2.
- Complete area filters, bridge/layer handling, deduplication, aircraft/building
  distance adjustments, pointer validity, and special non-HP effects are
  resolved in Task 3.
- Exact argument provenance and scheduler position for direct fire, weapon AoE,
  death weapons, radiation, and lightning are resolved in Task 3; the missing
  projectile lifecycle is supplied by G2.
- Attached-object trigger tags, action-map ownership, and synchronous action
  execution are supplied by GT; a damage-only adapter cannot infer them from
  the current polling runtime.
- Signed i32 Health, unified Techno ammo/readiness, controller relationships,
  native f32 veterancy/per-unit multipliers, signed Infantry FearLevel,
  non-duplicated last-attacker state, every active writer such as crate
  powerups, and any 3C-proven persistent radiation provenance are supplied by
  GS before any authoritative receiver field is attached.
- Kill attribution, XP routing, promotion, and same-tick veterancy visibility
  are supplied by GV; `MarkDestroyed` cannot substitute for native
  Killed/KilledByHouse callbacks.
- Receiver-created particle systems remain blocked until GP makes their full
  update state snapshot-persistent and completely world-hashed.
- Accepted decimal syntax, exceptional numeric behavior, x87 control state, and
  every conversion boundary are certified by Task 4/Task 20.
- Whether receiver callbacks require a refresh between pure stages is resolved
  by Task 1 and reviewed before Task 12.

## File Map

| Action | Path | Responsibility |
|---|---|---|
| Create | [docs/research/DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md) | Task 1A Techno prefix, source predicates, readiness, and rule/runtime reads |
| Create | [docs/research/DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md) | Task 1B Object HP, callbacks, triggers, and re-entry |
| Create | [docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md) | Task 1C defaults and house/country/difficulty assembly |
| Create | `docs/research/DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md` | G1 Techno/Object gates, numeric prefix, callbacks, triggers, defaults, and source semantics |
| Create | [docs/research/DAMAGE_NONBUILDING_RECEIVERS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_NONBUILDING_RECEIVERS_REINVESTIGATION_2026-07-13.md) | Task 2A Infantry, Unit/Foot, and Aircraft wrappers |
| Create | [docs/research/DAMAGE_BUILDING_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_BUILDING_RECEIVER_REINVESTIGATION_2026-07-13.md) | Task 2B Building wrapper, RNG, effects, and lifecycle |
| Create | [docs/research/DAMAGE_POSTMORTEM_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_POSTMORTEM_REINVESTIGATION_2026-07-13.md) | Task 2C delay-kill state machine and expiry owner |
| Create | [docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md) | G1 concrete wrappers, PostMortem, RNG, and lifecycle ownership |
| Create | [docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md) | Task 3A ordered collection, filters, coordinate/distance conversion, and receiver arguments |
| Create | `docs/research/DAMAGE_PROJECTILE_IMPACT_TIMING_REINVESTIGATION_2026-07-13.md` | Task 3B normal projectile/effect scheduling and G2 adapter facts |
| Create | [docs/research/DAMAGE_SPECIAL_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_SPECIAL_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md) | Task 3C death/radiation/lightning argument and scheduler facts |
| Create | [docs/research/DAMAGE_AREA_AND_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_AND_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md) | G1/G2 area collection, impact timing, and producer argument inventory |
| Consume existing private authority | `vera20k-oracle:docs/research/DAMAGE_ORACLE_CAPTURE_CONTRACT_2026-07-13.md` | Existing G3 fixture schema and handoff acceptance record retained by the private Oracle repository; coordinate only future versioned updates |
| Consume existing private authority | `vera20k-oracle:docs/research/schemas/damage-oracle-v1.schema.json` | Existing closed private Oracle fixture schema; extensions require a separately coordinated version bump |
| Create | `docs/plans/2026-07-13-damage-authoritative-cutover-owned-files.txt` | Exact execution-owned path manifest used for formatting and final dirty-tree accounting |
| Create | `src/util/native_float/mod.rs` | Public bit-wrapper and software-x87 façade |
| Create | `src/util/native_float/bits.rs` | `NativeF32Bits` and `NativeF64Bits` value types |
| Create | `src/util/native_float/decimal.rs` | Native-compatible decimal/percent parsing to exact IEEE bits |
| Create | `src/util/native_float/ext80.rs` | Deterministic x87 extended operations and `Math__ftol` conversion |
| Create | `src/util/native_float/tests.rs` | Raw-bit, exceptional, boundary, and operation-order tests |
| Modify | `src/util/mod.rs` | Export the low-level native-float module |
| Create | `src/rules/damage_rules.rs` | Armor enum plus exact global/country/containment damage inputs |
| Modify | `src/rules/mod.rs` | Export `damage_rules` |
| Modify | `src/rules/ruleset.rs` | Parse, validate, expose, and retain exact damage-rule fields |
| Modify | `src/rules/warhead_type.rs` | Add exact native numeric fields and verified receiver flags while retaining legacy fields until cutover |
| Modify | `src/rules/object_type.rs` | Add validated armor, immunity/readiness/delay-kill inputs, and native ability sets |
| Create | `src/sim/combat/damage/types.rs` | Named world-lepton coordinate plus raw call, source, result, block, and rollout contracts |
| Create | `src/sim/combat/damage/views.rs` | Immutable attacker/target/warhead/house receiver views |
| Create | `src/sim/combat/damage/effects.rs` | Ordered simulation effect intents with no presentation dependency |
| Create | `src/sim/combat/damage/native_math.rs` | Damage-specific expression sequencing over `Ext80` |
| Create | `src/sim/combat/damage/envelope.rs` | Ordered Techno/Object/concrete receiver stage orchestration |
| Create | `src/sim/combat/damage/resolver.rs` | Synchronous world integration at verified producer call points |
| Create | `src/sim/combat/damage/shadow.rs` | Raw-call/frozen-legacy observations and transient diagnostics |
| Create | `src/sim/combat/damage/runtime.rs` | Receiver runtime state types; attached to entities/houses only in Task 22 |
| Create | `src/sim/combat/damage/oracle_tests.rs` | Strict consumer for the coordinated retail fixture handoff |
| Modify | `src/sim/combat/damage/{mod,attacker,gates,kernel,receive}.rs` | Replace incomplete f64 scaffolding and expose the façade |
| Create | `src/sim/combat/combat_aoe/collector.rs` | Ordered fixed-record target and exact-distance capture |
| Create | `src/sim/combat/combat_aoe/layer.rs` | Verified layer/bridge/air/ground selection |
| Create | `src/sim/combat/combat_aoe/tests.rs` | Native-order and distance fixtures |
| Modify | `src/sim/combat/combat_aoe.rs` | Retain module façade; remove final damage math after authority |
| Create | `src/sim/combat/damage_integration_tests.rs` | Cross-producer shadow, ordering, same-tick, and authority tests |
| Create | `docs/research/DAMAGE_ORACLE_RUST_COMPARISON_2026-07-13.md` | G3 fixture coverage and mismatch report |
| External prerequisite | GT plan paths inserted by G0b | Parse attached trigger tags and provide synchronous native damage-event/action ownership |
| External prerequisite | `src/sim/components.rs` plus all GS caller paths inserted by G0b | Migrate signed Health/Fear, native veterancy and per-unit multipliers, ammo/controller/last-attacker, crate writers, and persistent producer provenance |
| External prerequisite | GP particle store/snapshot/hash paths inserted by G0b | Persist and completely hash receiver-created particle systems before their effects become authoritative |
| Modify | `src/sim/combat/mod.rs` | Shadow adapters, death/radiation calls, resolver wiring, and legacy removal |
| Modify | `src/sim/radiation.rs` | Carry verified source facts and raw signed damage inputs |
| Modify | `src/sim/superweapon/lightning_storm.rs` | Route verified lightning impacts through the receiver |
| Modify | `src/sim/superweapon/mod.rs` | Pass resolver context without creating a sim-to-audio/render dependency |
| Modify | `src/sim/world/mod.rs` | Own transient scratch and authoritative resolver context at verified phases |
| Modify | `src/sim/game_entity.rs` | Attach verified receiver runtime fields only at authority |
| Modify | `src/sim/house_state.rs` | Attach verified baked house damage multipliers only at authority |
| Modify | `src/sim/world/world_hash.rs` | Hash every new authoritative receiver/house state field |
| Modify | `src/sim/snapshot.rs` | Take the next free snapshot version in the coordinated authority change |
| Modify | `src/sim/combat/combat_tests.rs` | Retire legacy ProneDamage assertions and retain targeted live regressions |

The existing oversized files receive narrow wiring only. New calculation,
collector, trigger, shadow, and test logic goes in the new submodules so no new
cohesion problem is added to `ruleset.rs`, `combat/mod.rs`, `world/mod.rs`, or
`world_hash.rs`.

## Interface Changes

The plan introduces these contracts in dependency order:

1. `NativeF32Bits`, `NativeF64Bits`, `Ext80`, and `X87Context` expose exact bit
   input, control-aware arithmetic/comparison/store, and qword `Math__ftol`
   receipts with explicit low-EAX consumption. No simulation
   API exposes host floats.
2. `ArmorClass`, `DamageRules`, `CountryDamageRules`, and nested damage rule
   groups on `WarheadType`/`ObjectType` replace string indexing and lossy
   percentage reads for new consumers. Legacy fields remain until Task 23.
3. `DamageEvent` carries target, source object, source house, nullable warhead,
   signed post-attacker incoming damage, per-target lepton distance, and
   `ignore_defenses`.
4. `DamageOutcome` carries the final signed damage, native result state
   including `PostMortem`, and a structured block/result reason.
5. `DamageEffect` is an ordered sim-only intent. The calculator does not access
   `EntityStore`, RNG, sound buffers, rendering, UI, sidebar, or networking.
6. `AreaDamageTarget` carries only target ID and exact lepton distance.
7. `CompactShadowObservation` contains a raw call and a producer-specific frozen
   legacy result. It is transient and cannot affect gameplay.
8. `DamageRuntimeState` and `HouseDamageState` become serialized/hashed only in
   the coordinated authority task.
9. Task 10 owns `WorldLeptonCoord` in `damage/types.rs`; Task 16 and the later
   G2 impact adapter import that one type. G2 supplies an impact-side adapter
   whose damage portion is equivalent to:

```rust
/// Native Cartesian world `CoordStruct`: X/Y/Z are signed world leptons.
/// This is not a map-cell coordinate or an isometric screen vector.
pub(crate) struct WorldLeptonCoord {
    pub x_leptons: i32,
    pub y_leptons: i32,
    pub z_leptons: i32,
}

pub(crate) struct ProjectileImpactDamageCall {
    pub target_id: u64,
    pub source_object_id: Option<u64>,
    pub source_house: Option<crate::sim::intern::InternedId>,
    pub warhead_id: crate::sim::intern::InternedId,
    pub incoming_damage: i32,
    pub impact_coord: WorldLeptonCoord,
    pub ignore_defenses: bool,
}
```

This plan does not prescribe the projectile storage type or scheduler owner;
the separate projectile design must preserve the verified live Logic-vector
semantics. It only requires the impact call above at the detonation boundary.

## Sim Checklist

- [ ] No host `f32`/`f64` arithmetic appears in `src/sim/combat/damage/` or its
  live callers; native inputs are bit wrappers and operations use `Ext80`.
- [ ] Every authoritative runtime field is serialized and included in
  `world_hash.rs`; GP proves receiver-created particle systems are both restored
  and completely hashed, while transient scratch/diagnostics are skipped and
  hash-neutral.
- [ ] No `sim/` file imports from `render/`, `ui/`, `sidebar/`, `audio/`, or
  `net/`.
- [ ] Tick placement is documented per producer; normal weapons resolve at G2
  impact, area records resolve synchronously, and no shared late damage phase
  is introduced.
- [ ] EntityStore and active-object iteration remain deterministic; fixed area
  target records preserve verified native order without sorting.
- [ ] Scenario/main RNG ownership and draw position are verified for every
  concrete wrapper effect before authority.
- [ ] Shadow observations neither consume RNG nor alter EntityStore, active
  order, triggers, sound/world-effect queues, snapshot bytes, or hash bytes.
- [ ] Snapshot/golden changes have one owner and use the next free version read
  at Task 23 execution time.

## Risk Areas

- **Projectile timing:** Moving HP damage into the firing phase would change
  target visibility, deletion, retaliation, RNG, and same-frame behavior. G2
  is a hard prerequisite.
- **Concrete wrappers:** Common HP math is not the complete receiver. Infantry,
  Unit/Foot, Aircraft, and Building result handling and removal differ.
- **Re-entrant callbacks/triggers:** A precomputed effect list is unsafe if a
  callback changes state read by a later branch. Task 1 determines whether the
  executor refreshes views between stages.
- **Numeric boundaries:** Ordinary host floats, early rounding, extra clamps,
  divide-by-zero guards, and Rust casts can all change native results.
- **Area traversal:** Current Rust quantizes distance to cells and uses a
  different enumeration/fallback shape. A one-target order change is DRIFT.
- **Negative healing:** The current `u16` live path cannot represent it; signed
  health transitions and strength caps need explicit tests.
- **RNG/audio/visual aftermath:** Building fire/spark/sound/light updates are
  player-visible and affect RNG position even when HP matches.
- **Lifecycle recursion:** Death weapons can kill more entities while death
  processing is active; current Rust does not add those deaths to its existing
  dead list.
- **Dirty worktree:** `combat_tests.rs`, `world/mod.rs`, and `world_hash.rs`
  overlap other work. Oracle directories are outside this plan's ownership.
- **Snapshot compatibility:** Attaching runtime state before coordinated hash
  and snapshot changes would create hidden desync or load drift.

## Parity-Critical Items

| Task | Item | Why it matters | Verification |
|---|---|---|---|
| 1 | Techno/Object gate and callback order | A single reordered early return or side effect changes HP/state and later calls | Live decompile/callsites plus ordered oracle writes |
| 1 | Readiness mutation | It changes ammo/readiness but must not absorb HP damage | Before/after ammo, animation call, and final HP capture |
| 1 | Source-object ally predicate | Attackerless area calls must not be incorrectly blocked | Null-source/allied-source-house retail case |
| 2 | Building/other concrete wrappers | Result-dependent RNG, sounds, effects, and lifecycle are observable | Concrete-class oracle cases and active-vector membership |
| 2 | PostMortem | Delay-kill restores HP/life and returns state 5 at a specific point | Endpoint/repeated-hit/timer-expiry retail cases |
| 3 | Projectile and producer timing | Same-frame versus later-frame damage changes the entire tick consequence chain | Logic cursor/frame capture for each producer |
| 3/16 | Area order and distance | Every target must receive the same incoming value with its own exact distance in native order | Airborne/table/list fixtures and raw distance capture |
| 5-6 | IEEE parse and x87 operations | One-bit or one-conversion drift changes damage around boundaries | Raw bits and retail oracle matrix |
| 7-9 | Rule defaults and flags | Missing-key/default differences affect all stock and modded content | Missing-key plus merged-stock parse tests |
| 11 | Attacker conversion sequence | Rounding after each multiplier is part of the mechanism | Retail capture after every stage |
| 11 | Kernel grouping/cap/healing | Falloff, Verses, signed cap, and special armor rules own final magnitude | Kernel oracle below/equal/above boundaries |
| 12-15 | Receiver result/effect order | HP equality alone does not prove trigger, aftermath, or lifecycle parity | Ordered effect/result fixtures and concrete oracle |
| 17-19 | Shadow cut points | Feeding a legacy final value into the new receiver double-applies math | Raw/frozen observation tests for every producer |
| 20 | Pure-intent Oracle pass | Rust-vs-Rust tests cannot certify numeric/pure-stage behavior | Zero pure-stage mismatches; `G3: PENDING_EXECUTOR` |
| 23 | Full retail G3 preflight | Executor, RNG, timing, and lifecycle need retail evidence before authority | Zero required mismatches before any cutover edit |
| 21-24 | Synchronous commit and authority flip | A late global queue changes same-tick reads/removals and AoE later-target state | Same-tick multi-target and death-recursion tests |
| 23-24 | Hash/snapshot and RNG state | Hidden state or one missed draw creates multiplayer desync | World-hash fold tests, snapshot round trip, RNG receipt |

---

## Tasks

### Shared evidence preflight for Tasks 1-3

Run this once immediately before dispatching 1A-3C. Prefer the repo-local
research-index MCP `research_brief`/`research_handoff`/`research_graph` tools
when exposed; otherwise use these CLI equivalents:

```powershell
git status --short -- docs/research
Get-ChildItem docs/research -Recurse -File |
  Where-Object { $_.Name -match 'DAMAGE|RECEIVE|PROJECTILE|RADIATION|LIGHTNING|VETERANCY|CRATE' } |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 40 FullName,LastWriteTime
python tools/research_index/brief.py --system damage "Techno Object ReceiveDamage gates callbacks triggers readiness" --anchor 0x00701900 --anchor 0x005f5390 --anchor 0x00489180
python tools/research_index/handoff.py --system damage "concrete Infantry Unit Aircraft Building receiver PostMortem lifecycle"
python tools/research_index/brief.py --system damage "Apply_area_damage projectile impact death weapon radiation lightning timing" --anchor 0x00489280 --anchor 0x006fdd50
python tools/research_index/graph.py evidence 0x00701900
python tools/research_index/graph.py evidence 0x005f5390
python tools/research_index/graph.py evidence 0x00489280
```

Read every cited section used by a child unit before decompiling. If a matching
report has a recent modification time or `git status` shows another session's
output, coordinate ownership and extend that work rather than creating a
duplicate. Record the preflight timestamp, selected docs, index validation
state, and any ownership conflict in each child report.

### Mandatory Cargo protocol for regenerated implementation tasks

Tasks 5-24 are non-executable until G0a/G0b regeneration. That regeneration
must replace every shorthand Cargo block below with this protocol before each
individual command:

```powershell
function Assert-CargoIdle {
  $owners = Get-Process cargo,rustc -ErrorAction SilentlyContinue
  if ($owners) {
    $owners | Select-Object ProcessName,Id,CPU
    throw 'cargo/rustc is already active; coordinate its owner before continuing'
  }
}

function Invoke-NonzeroCargoTest([string]$filter) {
  Assert-CargoIdle
  $output = & cargo test -p vera20k $filter -- --nocapture 2>&1
  $output
  if ($LASTEXITCODE -ne 0) { throw "cargo test failed: $filter" }
  $counts = @($output | Select-String '^running ([0-9]+) tests?$' |
    ForEach-Object { [int]$_.Matches[0].Groups[1].Value })
  if (($counts | Measure-Object -Sum).Sum -le 0) {
    throw "cargo test matched zero tests: $filter"
  }
  $results = @($output | Select-String '^test result:')
  if (-not $results -or ($results | Where-Object { $_.Line -notmatch '^test result: ok\.' })) {
    throw "non-ok test result: $filter"
  }
}
```

One idle check does not authorize a sequence of Cargo commands. Call the helper
again for every filter, and call `Assert-CargoIdle` immediately before the final
`cargo check -q`.

### Task 1: Close the Techno/Object Receiver Evidence Gate

**Why:** The common receiver cannot be coded authoritatively while readiness,
`ignore_defenses`, defaults, callbacks, and triggers remain partly inferred.

**Files:**
- Create: [docs/research/DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md)
- Create: [docs/research/DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md)
- Create: [docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md)
- Create: `docs/research/DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md`
- Modify only when a live check disproves it:
  [docs/research/RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md)
- Modify only when a live check disproves it:
  [docs/research/GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md)

**Pattern:** Use `/re-investigate` discipline: read current reports, enumerate
claims before editing, verify load-bearing facts from function bodies and
callsites, then write a Rust-facing handoff. Ghidra access is read-only.

**Bounded execution units:**

| Unit | Sole output | Fixed scope | Acceptance |
|---|---|---|---|
| 1A — Techno prefix/gates | [DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md) | `0x00701900`, the `0x00489180` negative branch, source/null predicates, every Techno precheck, readiness, and all receiver-consumed runtime reads/writers | Ordered branch table; exact widths/defaults/arguments; writer inventory includes crate firepower/armor and no unresolved in-scope branch |
| 1B — Object callbacks | [DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md) | `0x005f5390`, HP writes/caps, condition classification, health-change callbacks, trigger calls `0x26`-`0x2c`, object/house kill routing, XP callback boundaries, and re-entry | Ordered read/write/call table including both `0x29` calls; every refresh checkpoint decided |
| 1C — Rule/house assembly | [DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md) | Constructor/parser/default and house/country/difficulty assembly for every field read by 1A/1B | Full native-field-to-INI/default/stock/current-Rust matrix; missing key and inheritance behavior stated |
| 1S — Reconcile | `DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md` | Reconcile 1A-1C and cited prior reports; no broad new investigation | One contradiction ledger and one Rust-facing contract; any unresolved load-bearing fact fails G1 |

Dispatch 1A-1C as separate read-only work items (one per worker at most), wait
for all three, then dispatch 1S. The 1S owner must not silently choose between
conflicting reports; it either resolves the conflict with a narrow live check or
leaves G1 failed.

**Step 1: Build the claim matrix before opening new decompilation**

The new report must begin with this table and one row per branch/call:

```markdown
| Sequence | Native owner | Predicate inputs | pDamage before/after |
|---:|---|---|---|
| 1 | Techno prefix | ... | ... |

| Sequence | Side effect/callback | Arguments | Can mutate a later read? |
|---:|---|---|---|
| 1 | ... | ... | yes/no + evidence |
```

Populate it from the existing reports first. Mark conflicting prose as
`CONFLICT` rather than choosing by label.

**Step 2: Recheck the native bodies and callsites**

At minimum, verify the full reachable path around:

- `TechnoClass::ReceiveDamage` at `0x00701900`;
- `ObjectClass::ReceiveDamage` at `0x005f5390`;
- `ApplyWarheadDamage` at `0x00489180` for the negative-damage branch operand,
  because one older report labels the compared argument as distance while the
  corrected gate/design identify the armor index;
- each helper/vtable target used for TypeImmune, Iron Curtain/force-shield,
  warp state, bunker routing, psionic/poison/radiation immunity,
  `AffectsAllies`, psychedelic handling, readiness, flee/scatter, threat,
  particle maintenance, retaliation, and `WasAttacked`;
- health-change callback and cap-to-Strength ownership;
- kill-credit writes and every native trigger call from `0x26` through `0x2c`,
  including the two distinct `0x29` calls;
- source-object-null plus allied-source-house behavior; and
- `ignore_defenses=false/true` at each individual branch.

For every binary claim, record the actual decompile/read-memory/xref operation
inline. Verify receiver identity from receiver pointer, argument flow, vtable
bytes, and callsites rather than the local label alone.

**Step 3: Resolve rule/runtime defaults needed by the body**

Record constructor, derived-constructor, and missing-key behavior for:

- Veteran and Elite ability arrays, including STRONGER index 1 and FIREPOWER
  index 2;
- Building versus non-Building `ImmuneToPsionics`;
- general `Immune`, `TypeImmune`, `Insignificant`, and
  `ImmuneToPsionicWeapons`;
- `DamageReducesReadiness`, `ReadinessReductionMultiplier`, `InitialAmmo`, and
  the runtime ammo/readiness field; and
- country/difficulty/house assembly for Firepower and armor category values.

For each runtime multiplier or rank read, follow write xrefs as well as reads.
The inventory explicitly covers crate firepower/armor/veterancy powerups,
promotion, map/spawn initialization, save/load, and any reset/removal path. A
manual Oracle setup of a multiplier does not prove the live writer; an unowned
active writer keeps GS/GV and authority blocked.

The 1C matrix is exhaustive over fields consumed by the receiver and concrete
wrappers, not limited to this example list. It must include `ConditionYellow`,
`ConditionRed`, `Sparky`, `BuildingDamageSound`, the global damage-fire
animation/list inputs, `SelfHealC4` and any verified C4/precheck flag read by a
wrapper, all Veteran/Elite ability defaults, and all class/type immunity fields.
For each row record native storage width, INI key or non-INI source, constructor
fallback, inheritance/override order, stock merged value, and the current Rust
parser/owner or `MISSING`. G0a may generate Tasks 7-10 only from this table.

**Step 4: Write the exact readiness contract**

The report must show input widths/signedness, operation order, conversion
points, clamp/comparison behavior, ammo write, animation call, and proof that HP
processing continues. Do not use the phrase “ammo absorption.”

**Step 5: State the integration consequence**

For each callback/trigger, answer whether a later receiver branch can observe
its mutation. If yes, the report requires staged view refresh after that effect;
if no, cite the body/call-chain evidence. This result determines the Task 12
executor shape.

**Step 6: Verify the report**

Run:

```powershell
python tools/research_index/validate.py --system damage "ReceiveDamage Techno Object readiness triggers"
python tools/research_index/index.py
python tools/research_index/validate.py --system damage "ReceiveDamage Techno Object readiness triggers"
```

Expected: the final validation is valid, every new local link resolves, and the
report's authority matrix has no `UNKNOWN`, `UNCHECKED`, or `CONFLICT` in a G1
row. If a row remains unresolved, retain the evidence and stop before Task 11;
shadow types may still proceed, but authority does not.

### Task 2: Close Concrete Receiver, PostMortem, and Lifecycle Evidence

**Why:** Common HP math cannot stand in for concrete Infantry, Unit/Foot,
Aircraft, or Building wrappers and their player-visible aftermath.

**Files:**
- Create:
  [docs/research/DAMAGE_NONBUILDING_RECEIVERS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_NONBUILDING_RECEIVERS_REINVESTIGATION_2026-07-13.md)
- Create:
  [docs/research/DAMAGE_BUILDING_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_BUILDING_RECEIVER_REINVESTIGATION_2026-07-13.md)
- Create:
  [docs/research/DAMAGE_POSTMORTEM_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_POSTMORTEM_REINVESTIGATION_2026-07-13.md)
- Create:
  [docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md)
- Modify only when live evidence requires a correction:
  [docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md)
- Correct or explicitly supersede the conflicting section in:
  [docs/research/TARGETDEATH_BUILDINGCLASS_DESTRUCTION_REMOVAL_OWNER_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TARGETDEATH_BUILDINGCLASS_DESTRUCTION_REMOVAL_OWNER_RESWARM_20260528.md)

**Pattern:** Narrow `/re-investigate` with `re-decoder-ring` only for vtable,
adjustor-thunk, receiver-identity, or function-boundary ambiguity.

**Bounded execution units:**

| Unit | Sole output | Fixed scope | Acceptance |
|---|---|---|---|
| 2A — Non-Building wrappers | [DAMAGE_NONBUILDING_RECEIVERS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_NONBUILDING_RECEIVERS_REINVESTIGATION_2026-07-13.md) | Active Infantry, Unit/Foot, and Aircraft receiver overrides and their pre/post calls | Raw-vtable/callsite identity plus ordered nonlethal/lethal state machine for each class |
| 2B — Building wrapper | [DAMAGE_BUILDING_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_BUILDING_RECEIVER_REINVESTIGATION_2026-07-13.md) | `0x00442230`, audited `0x004415f0`, effects/RNG/audio lookup, and actual removal chain | All result bands, RNG draws, effect identities, and membership transitions; stale `+0x4EC=Limbo` claim corrected |
| 2C — PostMortem | [DAMAGE_POSTMORTEM_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_POSTMORTEM_REINVESTIGATION_2026-07-13.md) | `CausesDelayKill`, eligibility, interpolation, repeated hits, restoration, return 5, and expiry | Exact field/formula/timer contract plus stock OilExplosionWH fixtures |
| 2S — Reconcile | [DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md) | Reconcile 2A-2C and prior reports | One complete class dispatch/lifecycle handoff; any unresolved class remains blocked |

Dispatch 2A-2C independently, then dispatch 2S. A single worker must not be
given the combined concrete-wrapper/PostMortem umbrella.

**Step 1: Anchor the concrete entries from live vtables/callsites**

Start with `BuildingClass::ReceiveDamage` at `0x00442230` and the audited
`BuildingClass::DestructionEffects` target at `0x004415f0`. Identify the active
Infantry, Unit/Foot, and Aircraft overrides from raw vtable bytes and callsites;
record address plus verified role instead of trusting imported names.

**Step 2: Capture each wrapper as an ordered state machine**

Use one table per concrete class:

```markdown
| Sequence | Predicate/read | Call/write/RNG draw | Return/result | Same-tick membership |
|---:|---|---|---|---|
| 1 | ... | ... | ... | active/limbo/pending-delete/deleted |
```

Cover nonlethal Damaged/Yellow/Red results, ordinary Dead, and any class-only
return. Record exact RNG owner and draw order, sound/effect identity lookup,
light/animation changes, retaliation/threat writes, passenger/garrison cleanup,
active-vector membership, limbo, pending deletion, and final removal owner.

**Step 3: Reconcile Building `+0x4EC`**

Trace callers and callees from the raw slot through `DestructionEffects` until
the actual lifecycle owner is identified. The stale synchronous-`Limbo` claim
must be removed or marked superseded with the live evidence next to it.

**Step 4: Fully decode `CausesDelayKill`/PostMortem**

Capture eligibility, input damage/distance, floating operation order,
conversions, interpolation endpoints, timer/latch fields, repeated-hit
selection, health/life restoration, return state 5, effects retained from the
lethal path, effects reversed, and timer-expiry owner. Include stock
`OilExplosionWH` against every stock `EligibleForDelayKill` type.

**Step 5: Verify the report and index**

Run:

```powershell
python tools/research_index/index.py
python tools/research_index/validate.py --system damage "concrete receiver Building PostMortem lifecycle"
```

Expected: the report identifies every in-scope concrete receiver and leaves no
authority-critical wrapper, RNG draw, PostMortem field/formula, or lifecycle
transition unresolved. Otherwise stop before Task 14 and keep the affected
class shadow-only.

### Task 3: Close Area Collection and Producer Timing Evidence

**Why:** Receiver parity fails if target records, distances, argument provenance,
or scheduler positions differ even when the arithmetic is perfect.

**Files:**
- Create:
  [docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md)
- Create:
  `docs/research/DAMAGE_PROJECTILE_IMPACT_TIMING_REINVESTIGATION_2026-07-13.md`
- Create:
  [docs/research/DAMAGE_SPECIAL_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_SPECIAL_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md)
- Create:
  [docs/research/DAMAGE_AREA_AND_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_AND_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md)
- Modify only when live evidence corrects it:
  [docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md)
- Modify only when live evidence corrects it:
  [docs/research/TARGETDEATH_APPLY_AREA_DAMAGE_LIVE_VECTOR_ITERATION_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TARGETDEATH_APPLY_AREA_DAMAGE_LIVE_VECTOR_ITERATION_RESWARM_20260528.md)

**Pattern:** `/re-investigate` from exact dispatcher and producer callsites;
trace arguments forward into receiver calls and backward to the gameplay source.

**Bounded execution units:**

| Unit | Sole output | Fixed scope | Acceptance |
|---|---|---|---|
| 3A — Area dispatcher | [DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md) | `0x00489280` collection, order, filters, layers, native Cartesian world-lepton coordinate conversion, distance, fixed records, and receiver arguments | Complete ordered collector contract and worked ground/air/Building coordinate fixtures |
| 3B — Projectile impact | `DAMAGE_PROJECTILE_IMPACT_TIMING_REINVESTIGATION_2026-07-13.md` | `0x006fdd50` through munition/effect insertion, live Logic-vector scheduling, detonation, and receiver | Same-frame and delayed traces; exact G2 call fields and scheduler owner |
| 3C — Special producers | [DAMAGE_SPECIAL_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_SPECIAL_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md) | Death weapon, radiation, and lightning only | One argument-provenance/tick-position table per producer, including recursion, RNG owner, and provenance lifetime/storage |
| 3S — Reconcile | [DAMAGE_AREA_AND_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_AND_PRODUCER_TIMING_REINVESTIGATION_2026-07-13.md) | Reconcile 3A-3C and classify all dispatcher xrefs | Complete in-scope route inventory; G1 rows resolved and G2 gaps stated without approximation |

Dispatch 3A-3C independently, then dispatch 3S. The 3S owner may classify an
xref as excluded only with active-YR reachability evidence.

**Step 1: Recheck `Apply_area_damage` at `0x00489280`**

Prove and document:

- the fixed-record capture boundary;
- airborne-first, CellSpread table, and per-cell linked-list order;
- CellSpread-zero behavior;
- every candidate filter, dedup rule, map/layer/bridge condition, building and
  aircraft distance adjustment, signed coordinate conversion, and distance
  unit;
- target-record lifetime and what happens when an earlier receiver changes or
  removes a later recorded target;
- exact call arguments: incoming damage, distance, warhead, source object,
  source house, and `ignore_defenses`; and
- non-HP dispatcher effects and whether they are inside this plan's entity
  receiver scope.

Walk at least one fixture from map cell plus subcell/height inputs through the
native Cartesian `CoordStruct` conversion to `WorldLeptonCoord { x_leptons,
y_leptons, z_leptons }`, then to the final signed lepton distance for ground,
aircraft, and Building targets. Name every reference frame and unit; do not use
`glam::IVec3`, map-cell axes, or isometric screen axes as an interchangeable
type.

**Step 2: Inventory every reachable native dispatcher caller**

Start from all xrefs to `0x00489280`, classify active YR versus dormant TS
legacy, and map the active calls to normal projectile impact, weapon AoE, death
weapon, radiation, lightning, or an excluded mechanism. Record the receiver
arguments for every in-scope source.

**Step 3: Capture timing for each in-scope source**

For normal weapons, trace `Fire_At` (`0x006fdd50`) through munition/effect
creation, live Logic-vector insertion, AI/detonation, warhead detonation, area
dispatch, and receiver. Include a same-frame appended-bullet case and a delayed
impact case. Repeat the scheduler-position analysis for death weapons,
radiation, and lightning.

For radiation, trace source object, source house, warhead, and damage facts from
detonation into every later periodic receiver call. Current Rust folds
`RadDetonation` in its creation tick while `RadiationState`/`RadSite` do not
retain those facts. If native periodic damage retains any of them, 3C must name
the persistent site/field state, width, initialization, expiry, serialization,
and hash ownership required from GS; extending only the transient detonation
record is forbidden.

**Step 4: Produce the G2 adapter contract**

The report must include the exact provenance of every field in
`ProjectileImpactDamageCall` from the Interface Changes section and name the
required scheduler owner. If current Rust lacks that owner, stop direct/weapon-
AoE authority and run a separate `/brainstorm projectile impact scheduling`,
then `/write-plan` only after that design is approved. This damage plan may
continue through shadow work while G2 is open.

**Step 5: Verify the report**

Run:

```powershell
python tools/research_index/index.py
python tools/research_index/validate.py --system damage "Apply_area_damage producer timing projectile impact"
```

Expected: G1 area rows are fully resolved, and each in-scope producer has one
verified native call point and argument-provenance row. G2 remains failed until
the separate projectile implementation is present and tested.

### Task 4: Coordinate and Accept the Retail Damage Oracle Handoff

**Why:** Hand-computed values and Rust-vs-Rust tests cannot certify x87,
receiver, order, RNG, timing, or lifecycle parity.

**Files:**
- Consume existing private authority: `vera20k-oracle:docs/research/DAMAGE_ORACLE_CAPTURE_CONTRACT_2026-07-13.md`
- Consume existing private authority: `vera20k-oracle:docs/research/schemas/damage-oracle-v1.schema.json`
- Read only in VERA20k: `parity/`
- Read only in the private sibling: `vera20k-oracle:tools/oracle_harness/`
- Read only in the private sibling: `vera20k-oracle:tools/oracle_hook_manifest/`
- Read only in the private sibling: `vera20k-oracle:tools/oracle_instrument/`
- Read only in the private sibling: `vera20k-oracle:tools/oracle_protocol/`

**Pattern:** Consumer handoff. Another task owns the Oracle tooling and runtime;
that private Oracle task also owns the contract and schema. This VERA task defines
consumer requirements, validates the signed immutable handoff read-only, and never
edits or drives private tool directories concurrently.

**Bounded execution units:**

| Unit | Dependency | Output/acceptance |
|---|---|---|
| 4A — Schema | None | Create and self-validate the closed v1 JSON Schema; do not claim fixture completeness |
| 4B — Ownership handoff | Oracle owner available | Record owner, binary/tool/fixture hashes and canonical paths; no capture-tool edits |
| 4C — Final case acceptance | 1S, 2S, and 3S complete | Reconcile every discovered field/callback/RNG/timing branch against the schema and case matrix, validate the immutable manifest, and issue the acceptance report |

4A and 4B may run alongside Tasks 1-3. 4C must run afterward. If 1S/2S/3S
discover a required field that v1 cannot express, do not extend v1: bump the
schema version, coordinate the Rust DTO change, and revalidate the handoff.

**Step 1: Record ownership before touching fixture paths**

Use the Codex task list or direct coordination to identify the Oracle owner.
The report records task name, handoff timestamp, retail binary path, SHA-256,
fixture root, manifest path, and capture-tool revision. If no owner can provide
the handoff, G3 remains failed.
Before accepting the handoff, set `$OracleRoot` to
`<local>/Documents/vera20k-oracle` and `$VeraRoot` to
`.`, then require the private facade's
`workspace-status --json --retail-root <retail-root>` result to be `READY` with
`vera20k_root` equal to `$VeraRoot`. Never infer either root from the other.

**Step 2: Create and require the closed v1 schema**

The Oracle owner writes the following JSON Schema verbatim to
`vera20k-oracle:docs/research/schemas/damage-oracle-v1.schema.json`; the VERA task
must not recreate it beneath `$VeraRoot`. The canonical handoff must
validate against it. `additionalProperties: false` is intentional: an Oracle
owner who needs another field bumps `schema_version`, adds a new schema file,
and coordinates the Rust consumer instead of silently extending v1.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://vera20k.local/schemas/damage-oracle-v1.schema.json",
  "title": "VERA20k retail damage Oracle manifest v1",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema_version", "binary_sha256", "program", "capture_tool_revision", "hooks"],
  "properties": {
    "schema_version": { "const": 1 },
    "binary_sha256": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
    "program": { "const": "gamemd.exe" },
    "capture_tool_revision": { "type": "string", "minLength": 1 },
    "hooks": {
      "type": "array",
      "minItems": 1,
      "items": { "$ref": "#/$defs/hook" }
    }
  },
  "$defs": {
    "tagged_value": {
      "oneOf": [
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "width_bits", "signed", "decimal"],
          "properties": {
            "kind": { "const": "integer" },
            "width_bits": { "enum": [8, 16, 32, 64] },
            "signed": { "type": "boolean" },
            "decimal": { "type": "string", "pattern": "^-?[0-9]+$" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "bits"],
          "properties": {
            "kind": { "const": "f32_bits" },
            "bits": { "type": "string", "pattern": "^0x[0-9a-fA-F]{8}$" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "bits"],
          "properties": {
            "kind": { "const": "f64_bits" },
            "bits": { "type": "string", "pattern": "^0x[0-9a-fA-F]{16}$" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "significand_bits", "exponent_sign_bits"],
          "properties": {
            "kind": { "const": "ext80_bits" },
            "significand_bits": { "type": "string", "pattern": "^0x[0-9a-fA-F]{16}$" },
            "exponent_sign_bits": { "type": "string", "pattern": "^0x[0-9a-fA-F]{4}$" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "width_bits", "hex"],
          "properties": {
            "kind": { "const": "pointer" },
            "width_bits": { "enum": [32, 64] },
            "hex": { "type": "string", "pattern": "^0x[0-9a-fA-F]+$" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "value"],
          "properties": {
            "kind": { "const": "boolean" },
            "value": { "type": "boolean" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "name", "discriminant"],
          "properties": {
            "kind": { "const": "enum" },
            "name": { "type": "string", "minLength": 1 },
            "discriminant": { "$ref": "#/$defs/integer_value" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind", "hex"],
          "properties": {
            "kind": { "const": "bytes" },
            "hex": { "type": "string", "pattern": "^(?:[0-9a-fA-F]{2})*$" }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": ["kind"],
          "properties": { "kind": { "const": "null" } }
        }
      ]
    },
    "integer_value": {
      "type": "object",
      "additionalProperties": false,
      "required": ["kind", "width_bits", "signed", "decimal"],
      "properties": {
        "kind": { "const": "integer" },
        "width_bits": { "enum": [8, 16, 32, 64] },
        "signed": { "type": "boolean" },
        "decimal": { "type": "string", "pattern": "^-?[0-9]+$" }
      }
    },
    "rng_receipt": {
      "type": "object",
      "additionalProperties": false,
      "required": ["stream", "counter_before", "counter_after", "value"],
      "properties": {
        "stream": { "type": "string", "minLength": 1 },
        "counter_before": { "$ref": "#/$defs/integer_value" },
        "counter_after": { "$ref": "#/$defs/integer_value" },
        "value": { "$ref": "#/$defs/tagged_value" }
      }
    },
    "cursor": {
      "type": "object",
      "additionalProperties": false,
      "required": ["frame", "logic_index", "phase"],
      "properties": {
        "frame": { "$ref": "#/$defs/integer_value" },
        "logic_index": { "$ref": "#/$defs/integer_value" },
        "phase": { "type": "string", "minLength": 1 }
      }
    },
    "membership": {
      "type": "object",
      "additionalProperties": false,
      "required": ["active", "limbo", "pending_delete", "deleted"],
      "properties": {
        "active": { "type": "boolean" },
        "limbo": { "type": "boolean" },
        "pending_delete": { "type": "boolean" },
        "deleted": { "type": "boolean" }
      }
    },
    "termination_receipt": {
      "type": "object",
      "additionalProperties": false,
      "required": ["kind", "code", "instruction_address", "fault_address", "access"],
      "properties": {
        "kind": { "enum": ["seh_exception", "process_exit", "abort"] },
        "code": { "type": "string", "pattern": "^0x[0-9a-fA-F]{8}$" },
        "instruction_address": { "type": ["string", "null"], "pattern": "^0x[0-9a-fA-F]{8}$" },
        "fault_address": { "type": ["string", "null"], "pattern": "^0x[0-9a-fA-F]{8}$" },
        "access": { "enum": ["read", "write", "execute", "unknown"] }
      }
    },
    "observation": {
      "type": "object",
      "additionalProperties": false,
      "required": ["sequence", "kind", "owner", "address", "field", "before", "after", "arguments", "returned", "rng", "cursor", "membership"],
      "properties": {
        "sequence": { "type": "integer", "minimum": 0 },
        "kind": { "enum": ["read", "write", "call", "rng", "cursor", "membership"] },
        "owner": { "type": "string", "minLength": 1 },
        "address": { "type": ["string", "null"], "pattern": "^0x[0-9a-fA-F]{8}$" },
        "field": { "type": ["string", "null"] },
        "before": { "$ref": "#/$defs/tagged_value" },
        "after": { "$ref": "#/$defs/tagged_value" },
        "arguments": {
          "type": "object",
          "additionalProperties": { "$ref": "#/$defs/tagged_value" }
        },
        "returned": { "$ref": "#/$defs/tagged_value" },
        "rng": { "oneOf": [{ "$ref": "#/$defs/rng_receipt" }, { "type": "null" }] },
        "cursor": { "oneOf": [{ "$ref": "#/$defs/cursor" }, { "type": "null" }] },
        "membership": { "oneOf": [{ "$ref": "#/$defs/membership" }, { "type": "null" }] }
      }
    },
    "case_result": {
      "type": "object",
      "additionalProperties": false,
      "required": ["termination", "return_value", "final_fields", "rng_receipts", "cursor", "membership"],
      "properties": {
        "termination": { "oneOf": [{ "$ref": "#/$defs/termination_receipt" }, { "type": "null" }] },
        "return_value": { "$ref": "#/$defs/tagged_value" },
        "final_fields": {
          "type": "object",
          "additionalProperties": { "$ref": "#/$defs/tagged_value" }
        },
        "rng_receipts": {
          "type": "array",
          "items": { "$ref": "#/$defs/rng_receipt" }
        },
        "cursor": { "oneOf": [{ "$ref": "#/$defs/cursor" }, { "type": "null" }] },
        "membership": { "oneOf": [{ "$ref": "#/$defs/membership" }, { "type": "null" }] }
      }
    },
    "case": {
      "type": "object",
      "additionalProperties": false,
      "required": ["id", "inputs", "ordered_observations", "result"],
      "properties": {
        "id": { "type": "string", "pattern": "^[a-z0-9][a-z0-9-]*$" },
        "inputs": {
          "type": "object",
          "additionalProperties": { "$ref": "#/$defs/tagged_value" }
        },
        "ordered_observations": {
          "type": "array",
          "items": { "$ref": "#/$defs/observation" }
        },
        "result": { "$ref": "#/$defs/case_result" }
      }
    },
    "hook": {
      "type": "object",
      "additionalProperties": false,
      "required": ["address", "role", "cases"],
      "properties": {
        "address": { "type": "string", "pattern": "^0x[0-9a-fA-F]{8}$" },
        "role": { "type": "string", "minLength": 1 },
        "cases": {
          "type": "array",
          "minItems": 1,
          "items": { "$ref": "#/$defs/case" }
        }
      }
    }
  }
}
```

Every non-applicable observation slot uses the tagged `null` value or JSON
`null` exactly as declared; it is never omitted. This makes Task 20's Rust DTOs
strict and deterministic instead of relying on permissive maps.
Every parser source buffer uses the `bytes` tagged value with exact hex bytes;
never serialize parser input as a Unicode string or reconstructed decimal.
Normal cases set `result.termination` to JSON `null`. An isolated native-fault
case sets `termination` to the exact exception/exit receipt, uses tagged `null`
for `return_value`, and records only genuinely observed pre-termination fields;
it never fabricates a normal return.

**Step 3: Require the minimum case matrix**

- Kernel `0x00489180`: zero, healing armor 7/8, direct/mid/edge−1/edge/edge+1
  distances, fractional Verses, percent/bare parse, double-truncation boundary,
  cap below/equal/above, fallback 1000, stock 10000, accepted exceptional inputs.
- `Fire_At` `0x006fdd50`: baseline, country/per-unit, veteran, elite, civilian
  garrison, bunker, open-topped, bunker+open-top, Wave/special stored-zero path,
  and per-unit firepower after the active crate writer. Include negative stock
  Heal/RepairBullet damage, ordinary zero, the strictly-positive scaling gate,
  a PC-rounding-sensitive `(house * unit) * damage` grouping case, and a special
  zero case with containment enabled so the post-zero `Math__ftol` calls and
  control-word receipt are observable.
- Area `0x00489280`: CellSpread zero, airborne+ground, two same-cell targets,
  several offset cells, aircraft adjustment, Building/height case, null source,
  and earlier-target mutation/removal affecting a later recorded target.
- Techno `0x00701900`: every armor divisor, vet/elite, TypeImmune variants,
  every defense/immunity gate with both `ignore_defenses` values, readiness,
  psychedelic, attacker-null/allied-source-house, per-unit armor after the
  active crate writer, signed FearLevel updates through 300, and paired bunker
  shell/occupant routing for `PenetratesBunker=false/true` plus any verified
  `ignore_defenses` interaction.
- Object `0x005f5390`: early exits, healing/cap/callback, Building minimum-one,
  inclusive overkill, exact Yellow/Red crossings, lethal credit, and all trigger
  outcomes/order.
- Building `0x00442230`: every precheck/result band, ordinary lethal,
  eligible/ineligible delay kill, interpolation endpoints, repeated shorter and
  longer hits, Iron Curtain at death, RNG effects, and lifecycle membership.
- Concrete wrappers: nonlethal and lethal Infantry, Unit, Aircraft, Building.
- Kill/veterancy: object-versus-house attribution, allied/no-XP, f32 accumulator
  before/after, rank threshold crossing, promotion effects, and same-tick later
  attacker damage.
- Producer timing: delayed projectile, same-frame appended bullet, death-weapon
  recursion, radiation, and lightning.
- Decimal grammar/parser: empty and whitespace-only text, leading/trailing
  whitespace, leading plus/minus, leading/trailing decimal point, exponent
  forms, percent suffix with surrounding whitespace, duplicate suffixes,
  trailing junk, overflow, underflow, and every accepted/rejected form found in
  the native parser. Capture the final f32/f64 bits, not a printed decimal.
- Numeric parser hook roles are mandatory, not observations folded into a
  generic kernel case: `CCINIClass::ReadInt @ 0x005276d0`,
  `CCINIClass::ReadDouble @ 0x005283d0`, the Warhead `Verses` token loop anchored at `0x0075de06` (body
  `0x0075de0c..0x0075de58`), and `Math__ftol @ 0x007c5f00`. The Verses role
  includes the missing-key fallback parse, present empty/trimmed-empty input,
  exactly 11 post-`strtok` tokens, more than 11 tokens, consecutive/leading/
  trailing delimiters, the `ReadString` 0x80-byte buffer boundary (127 payload
  bytes plus forced NUL before trim/tokenization), and a present nonempty list
  that collapses to fewer than 11 tokens and whose isolated retail run records
  the native null-token fault/termination. If closed schema v1 cannot represent
  that receipt without ambiguity, 4C bumps the schema as required by Step 2; it
  must not omit the case.
- Exceptional numeric/control state: positive/negative zero, minimum/maximum
  subnormal, finite overflow boundaries, infinities, every reachable quiet or
  signaling NaN payload/sign, unordered comparisons, divide-by-zero/invalid
  behavior, x87 entry/exit precision-control and rounding-control words,
  store-to-f32/f64 boundaries, and `Math__ftol` qword receipts at i32 and i64
  min/max±1. Include representative values above i32 but inside i64 plus
  NaN/infinity/invalid qword-indefinite results, recording full EDX:EAX and low
  EAX separately. If active YR changes or fixes the control word, record its
  exact before/after bits and prove the call path.

**Step 4: Validate the handoff without running the owner's tools**

This is unit 4C and is blocked until the signed-off 1S, 2S, and 3S synthesis
reports have been diffed against the hook/case inventory. Append every newly
discovered callback, rule read, RNG draw, producer argument, membership state,
and result branch before validation.

Run:

```powershell
$OracleRoot = '<local>/Documents/vera20k-oracle'
$VeraRoot = '.'
$RetailRoot = '<ra2-install>'
$workspaceJson = @(& python -B (Join-Path $OracleRoot 'tools/oracle_harness/oracle.py') `
    workspace-status --json --retail-root $RetailRoot) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Private Oracle workspace-status failed.' }
$workspace = $workspaceJson | ConvertFrom-Json
if ($workspace.status -ne 'READY' -or
    [System.IO.Path]::GetFullPath([string]$workspace.vera20k_root) -cne
        [System.IO.Path]::GetFullPath($VeraRoot)) {
    throw 'Private Oracle is not linked to the expected VERA20k root.'
}
Get-FileHash (Join-Path $RetailRoot 'gamemd.exe') -Algorithm SHA256
@'
import json
import sys
from pathlib import Path
from jsonschema import Draft202012Validator

oracle_root = Path(sys.argv[1]).resolve(strict=True)
vera_root = Path(sys.argv[2]).resolve(strict=True)
manifest = json.loads((vera_root / "parity/fixtures/damage/manifest.json").read_text(encoding="utf-8"))
version = manifest["schema_version"]
schema_path = oracle_root / f"docs/research/schemas/damage-oracle-v{version}.schema.json"
schema = json.loads(schema_path.read_text(encoding="utf-8"))
Draft202012Validator.check_schema(schema)
Draft202012Validator(schema).validate(manifest)
print("damage Oracle schema: valid")
'@ | python - $OracleRoot $VeraRoot
```

The Oracle owner must copy or export the accepted immutable handoff to the VERA-owned
`parity/fixtures/damage/manifest.json` consumer path. Compare the computed binary
hash to `binary_sha256`,
enumerate every required hook/case in the report, and record missing cases as G3
failures. Do not start Task 20 until the manifest is complete and stable.
Only the 4C report may use the words “complete and stable”; 4A/4B leave that
status pending.

### Task 5: Add Exact IEEE Bit Types and Native Decimal Parsing

**Why:** Rule values must retain native-relevant bits before any damage formula
can be compared or made authoritative.

**Files:**
- Create: `src/util/native_float/mod.rs`
- Create: `src/util/native_float/bits.rs`
- Create: `src/util/native_float/decimal.rs`
- Create: `src/util/native_float/tests.rs`
- Modify: `src/util/mod.rs`

**Pattern:** New low-level reusable utility. It depends only on `std` plus the
existing `serde` dependency and may be used by `rules/` and `sim/`; it imports
no game module.

**Dependency:** G0a and the accepted 4C Oracle contract. G0a replaces this
umbrella with the bounded bit-wrapper, parser-grammar, and rounding child tasks.

**Step 1: Define the bit wrappers**

```rust
// src/util/native_float/bits.rs
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct NativeF32Bits(u32);

impl NativeF32Bits {
    pub const ZERO: Self = Self(0x0000_0000);
    pub const ONE: Self = Self(0x3f80_0000);

    #[inline]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl core::fmt::Debug for NativeF32Bits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "NativeF32Bits(0x{:08x})", self.0)
    }
}

#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct NativeF64Bits(u64);

impl NativeF64Bits {
    pub const ZERO: Self = Self(0x0000_0000_0000_0000);
    pub const ONE: Self = Self(0x3ff0_0000_0000_0000);

    #[inline]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    #[inline]
    pub const fn bits(self) -> u64 {
        self.0
    }
}


impl core::fmt::Debug for NativeF64Bits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "NativeF64Bits(0x{:016x})", self.0)
    }
}
```

Do not add `to_f32`/`to_f64` methods to the public API. Debug formatting prints
hex bits so sim callers cannot accidentally choose host arithmetic. No
`PartialOrd`/`Ord` is derived: IEEE numeric order is not raw-bit order, and a
caller that truly needs bitwise ordering must name that comparison explicitly.

**Step 2: Implement the four distinct native numeric parser paths**

`decimal.rs` implements a private base-`2^32` `BigNat` with `mul_small`,
`add_small`, shifts, comparison, subtraction, and quotient-with-remainder. It
does not expose one generic full-string parser because active YR uses different
CRT paths:

1. `CCINIClass::ReadInt @ 0x005276d0` owns `MaxDamage`. Task 4 captures its
   exact byte grammar, no-conversion behavior, overflow, and current-value
   fallback. Do not alias it to atoi or the double reader unless the capture
   proves the complete behavior identical.
2. `CCINIClass::ReadDouble @ 0x005283D0` scans a decimal *prefix* with
   `sscanf("%f")` into binary32. The value is then loaded/widened for its caller;
   if the raw string contains `%`, the widened value is multiplied by the f64
   `0.01` constant. Thus stock `VeteranCombat=1.1` originates as f32
   `0x3f8ccccd` and widens to f64 `0x3ff19999a0000000`, not direct-strtod
   `0x3ff199999999999a`.
3. A bare Verses token uses the strtod-family prefix parser and stores binary64
   directly.
4. A Verses token containing `%` uses atoi-prefix to signed integer, then
   multiplies by the f64 `0.01` constant. Fractional text before `%` is discarded.

G0a writes the literal accepted grammar from Task 4, including whitespace,
trailing junk, exponent, exceptional spellings, and overflow. The conversion
core forms an exact rational and rounds once to the destination format using
the captured CRT mode. It returns the number of consumed bytes so a caller can
model prefix acceptance; it never requires end-of-string unless the native
caller does.

Expose only the parser primitives:

```rust
pub struct ParsedPrefix<T> {
    pub value: T,
    pub consumed: usize,
}

pub fn ccini_read_int_prefix(raw: &[u8]) -> Option<ParsedPrefix<i32>>;
pub fn scan_f32_prefix(raw: &[u8]) -> Option<ParsedPrefix<NativeF32Bits>>;
pub fn strtod_f64_prefix(raw: &[u8]) -> Option<ParsedPrefix<NativeF64Bits>>;
pub fn atoi_i32_prefix(raw: &[u8]) -> i32;
pub fn widen_f32_to_f64_bits(value: NativeF32Bits) -> NativeF64Bits;
```

All numeric primitives consume native bytes, not Rust `str`; callers may pass an
ASCII literal with `b"..."`, but no helper may perform lossy UTF-8 conversion.
`atoi_i32_prefix` and malformed/no-conversion behavior use Task 4's captured
MSVC CRT result, including overflow. Percent multiplication is not performed in
Task 5; Task 6 supplies the control-word-aware x87 operation and Task 7 exposes
separate CCINI and Verses reader helpers.

**Step 3: Export the module**

```rust
// src/util/native_float/mod.rs
mod bits;
mod decimal;

pub use bits::{NativeF32Bits, NativeF64Bits};
pub use decimal::{
    ParsedPrefix, atoi_i32_prefix, ccini_read_int_prefix, scan_f32_prefix,
    strtod_f64_prefix, widen_f32_to_f64_bits,
};

#[cfg(test)]
mod tests;
```

Add `pub mod native_float;` to `src/util/mod.rs`.

Task 6 adds `mod ext80;` and the `Ext80`/`X87Compare` re-exports only after
`ext80.rs` exists, so Task 5 compiles independently.

**Step 4: Add exact-bit tests**

`tests.rs` includes at minimum:

```rust
#[test]
fn parses_stock_damage_values_to_exact_bits() {
    assert_eq!(scan_f32_prefix(b"256").unwrap().value.bits(), 0x4380_0000);
    assert_eq!(scan_f32_prefix(b"1.1").unwrap().value.bits(), 0x3f8c_cccd);
    assert_eq!(
        widen_f32_to_f64_bits(scan_f32_prefix(b"1.1").unwrap().value).bits(),
        0x3ff1_9999_a000_0000
    );
    assert_eq!(
        strtod_f64_prefix(b"1.1").unwrap().value.bits(),
        0x3ff1_9999_9999_999a
    );
    assert_eq!(scan_f32_prefix(b"0,01").unwrap().value.bits(), 0);
    assert_eq!(scan_f32_prefix(b"0,01").unwrap().consumed, 1);
}

#[test]
fn preserves_signed_zero_and_ties_to_even() {
    assert_eq!(scan_f32_prefix(b"-0").unwrap().value.bits(), 0x8000_0000);
    assert_eq!(
        scan_f32_prefix(b"1.000000059604644775390625")
            .unwrap()
            .value
            .bits(),
        0x3f80_0000
    );
}
```

Add boundary cases from the accepted Task 4 parser captures before this task is
considered final.

**Step 5: Verify**

First check for another Cargo owner, then run serially:

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k native_float -- --nocapture
```

Expected literal result: `test result: ok.` No host floating operation appears
in `src/util/native_float/decimal.rs`.

### Task 6: Implement the Deterministic x87 Subset and `Math__ftol`

**Why:** Stored IEEE bits alone do not reproduce x87 extended intermediates,
operation grouping, comparisons, or native integer conversion.

**Files:**
- Create: `src/util/native_float/ext80.rs`
- Modify: `src/util/native_float/tests.rs`

**Pattern:** New integer-backed low-level arithmetic restricted to operations
used by the verified damage paths; no dependency crate and no `unsafe` code.

**Dependency:** G0a and the regenerated Task 5 children. G0a replaces this
umbrella with bounded decode/store, add/sub, mul/div, compare, and ftol tasks.

**Step 1: Define value, control, compare, and conversion-receipt types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtClass { Zero, Finite, Infinity, NaN }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ext80 {
    class: ExtClass,
    sign: bool,
    exponent: i32,
    significand: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ext80Bits {
    pub significand_bits: u64,
    pub exponent_sign_bits: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X87Compare { Less, Equal, Greater, Unordered }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X87ControlWord(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X87Context { control: X87ControlWord }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FtolEdxEax(i64);
```

Add the corresponding re-exports from `native_float/mod.rs`. For a finite
nonzero value, the explicit integer bit is present in `significand`. The 80-bit
register encoding does not imply 64-bit arithmetic precision: the captured x87
precision-control and rounding-control bits govern every arithmetic result.

The verified word at `0x00822D80` is `0x0e7f`: PC=`10` (53 significand bits)
and RC=`11` (toward zero). `Math__ftol @ 0x007c5f00` reads the caller word and,
when it differs, loads `0x0e7f` before `FISTP qword`; it does **not** restore the
old word before returning. The context therefore remains `0x0e7f` after the
call. Task 4 records the entry and exit control word at every damage hook. Never
assume always-64-bit precision or round-to-nearest-even.

**Step 2: Implement exact load, store, and `Math__ftol` behavior**

```rust
impl Ext80 {
    pub fn from_i32(value: i32) -> Self;
    pub fn from_f32_bits(value: NativeF32Bits) -> Self;
    pub fn from_f64_bits(value: NativeF64Bits) -> Self;
    pub(crate) fn try_from_bits(bits: Ext80Bits) -> Result<Self, Ext80DecodeError>;
    pub(crate) fn to_bits(self) -> Ext80Bits;
}

impl X87Context {
    pub fn from_control_word(raw: u16) -> Self;
    pub fn store_f32(self, value: Ext80) -> NativeF32Bits;
    pub fn store_f64(self, value: Ext80) -> NativeF64Bits;
    pub fn math_ftol(&mut self, value: Ext80) -> FtolEdxEax;
}

impl FtolEdxEax {
    pub const fn signed_i64(self) -> i64;
    pub const fn edx_i32(self) -> i32;
    pub const fn eax_i32(self) -> i32;
}
```

Decode normals/subnormals and NaN sign/payload exactly. Stores use the supplied
RC bits. `math_ftol` models the qword conversion, not an i32 conversion: it
returns the full signed EDX:EAX receipt under forced `0x0e7f`, leaves the
context at `0x0e7f`, and includes the
Task 4-captured 64-bit indefinite value on invalid/i64 overflow. Damage
callsites explicitly consume low EAX via `eax_i32()`. Do not clamp, saturate, or
replace this with an i32-range overflow rule.

`try_from_bits`/`to_bits` are the crate-private Oracle boundary for the schema's
raw 10-byte extended values; they never narrow through host f64. They must
round-trip every Task 4-captured register encoding, including sign, signaling/
quiet NaN distinction, and payload. If `ExtClass` plus the fields above cannot
round-trip a reachable pseudo-denormal, unnormal, unsupported, or other raw x87
encoding, G0a changes the representation before Task 6 rather than canonicalize
it. Add the decode error and these accessors to the Task 6 child file manifest.

**Step 3: Implement only the required control-aware operations**

```rust
impl X87Context {
    pub fn add(self, lhs: Ext80, rhs: Ext80) -> Ext80;
    pub fn sub(self, lhs: Ext80, rhs: Ext80) -> Ext80;
    pub fn mul(self, lhs: Ext80, rhs: Ext80) -> Ext80;
    pub fn div(self, lhs: Ext80, rhs: Ext80) -> Ext80;
    pub fn compare(self, lhs: Ext80, rhs: Ext80) -> X87Compare;
}
```

- Addition/subtraction retain guard/round/sticky state and round to the
  control word's 24-, 53-, or 64-bit precision.
- Multiplication uses the full `u64 × u64 -> u128` product before the
  control-word rounding step.
- Division retains quotient remainder as sticky information.
- Masked exceptional behavior, signed zero, NaN propagation/payload, and
  unordered status follow Task 4 captures.
- No operation is reassociated; explicit f32/f64 stores occur only where the
  assembly performs them.

**Step 4: Add control, operation, and conversion-receipt tests**

Cover every Task 4-observed PC/RC combination, signed zero, subnormals, carry,
cancellation, NaN payload/sign and unordered comparison, divide-by-zero,
invalid operations, exact/inexact division, and values around both i32 and i64
limits. Prove the captured 53-bit sequence differs from an always-64-bit model
and from an early f64 store. Add a value above i32 but inside i64 and assert both
full EDX:EAX and low EAX so an i32 conversion cannot pass accidentally.

```rust
#[test]
fn math_ftol_returns_qword_and_callers_read_low_eax() {
    let mut x87 = X87Context::from_control_word(0x0e7f);
    let positive = Ext80::from_f64_bits(strtod_f64_prefix(b"1.999").unwrap().value);
    let negative = Ext80::from_f64_bits(strtod_f64_prefix(b"-1.999").unwrap().value);
    assert_eq!(x87.math_ftol(positive).signed_i64(), 1);
    assert_eq!(x87.math_ftol(negative).eax_i32(), -1);
}
```

All other exact expected receipts come directly from Task 4; G0a inserts their
literal control words and raw bits before Task 6 becomes executable.

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k native_float -- --nocapture
rg -n "\bf32\b|\bf64\b| as i32" src/util/native_float
```

Expected: `test result: ok.` The grep may find type names only in tests or doc
comments describing forbidden host behavior; arithmetic implementation contains
no host-float value.

### Task 7: Add Exact Global, Country, Containment, and Armor Rules

**Why:** New receiver code must not consume armor strings, rounded percentages,
or identity values invented inside simulation.

**Files:**
- Create: `src/rules/damage_rules.rs`
- Modify: `src/rules/mod.rs`
- Modify: `src/rules/ruleset.rs`
- Modify: `src/rules/error.rs` only if Task 4 proves a numeric parse needs a
  distinct error variant; otherwise use `RulesError::InvalidValue`

**Pattern:** Typed additive rule group, following `RadiationRules` and
`GarrisonRules`, with exact native bits retained beside legacy readers.

**Dependency:** G0a and Tasks 1, 4, 5, and 6. If Task 1 has not resolved house difficulty
assembly, parse the fields and expose them to shadow views but keep G1 failed;
do not invent the runtime combination.

**Step 1: Define the armor enum and exact rule groups**

```rust
// src/rules/damage_rules.rs
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::util::native_float::{
    Ext80, NativeF32Bits, NativeF64Bits, X87Context, ccini_read_int_prefix,
    scan_f32_prefix,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ArmorClass {
    None = 0,
    Flak = 1,
    Plate = 2,
    Light = 3,
    Medium = 4,
    Heavy = 5,
    Wood = 6,
    Steel = 7,
    Concrete = 8,
    Special1 = 9,
    Special2 = 10,
}

impl ArmorClass {
    pub const COUNT: usize = 11;

    pub fn parse(section: &str, value: &str) -> Result<Self, RulesError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "flak" => Ok(Self::Flak),
            "plate" => Ok(Self::Plate),
            "light" => Ok(Self::Light),
            "medium" => Ok(Self::Medium),
            "heavy" => Ok(Self::Heavy),
            "wood" => Ok(Self::Wood),
            "steel" => Ok(Self::Steel),
            "concrete" => Ok(Self::Concrete),
            "special_1" => Ok(Self::Special1),
            "special_2" => Ok(Self::Special2),
            _ => Err(RulesError::InvalidValue {
                section: section.to_string(),
                key: "Armor".to_string(),
                expected: "one of the 11 YR armor names".to_string(),
                value: value.to_string(),
            }),
        }
    }

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRules {
    pub max_damage: i32,
    pub veteran_combat: NativeF64Bits,
    pub veteran_armor: NativeF64Bits,
    pub condition_yellow: NativeF64Bits,
    pub condition_red: NativeF64Bits,
    pub occupy_damage_multiplier: NativeF32Bits,
    pub bunker_damage_multiplier: NativeF32Bits,
    pub open_topped_damage_multiplier: NativeF32Bits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountryDamageRules {
    pub firepower: NativeF64Bits,
    pub armor: NativeF64Bits,
    pub armor_infantry: NativeF32Bits,
    pub armor_units: NativeF32Bits,
    pub armor_aircraft: NativeF32Bits,
    pub armor_buildings: NativeF32Bits,
    pub armor_defenses: NativeF32Bits,
}
```

The `armor` double is retained because Task 1 must prove how House difficulty
assembly combines it. Category fields remain separate even when stock countries
omit them. These field widths are a G0a input, not permission to guess: 1C
replaces any illustrative width above that disagrees with the native field's
actual store instruction.

**Step 2: Parse exact values and defaults**

Implement
`DamageRules::from_ini(&IniFile, &mut X87Context) -> Result<Self, RulesError>`
and `CountryDamageRules::from_section(section_name, section, &mut X87Context)`.
The mutable parser context is the context created from the Task 4/G0a-captured
control word at native rule-parse entry; do not create or reset one per key.
`MaxDamage` alone uses `CCINIClass::ReadInt @ 0x005276d0`; on a missing key it
retains the constructor/current-value fallback `1000`, while a present value
uses the Task 4-captured `ccini_read_int_prefix` contract. It must not pass
through `ReadDouble`, host `parse::<i32>()`, or Verses atoi by assumption.
All floating damage-rule fields use `CCINIClass::ReadDouble` semantics; they do
**not** use the Verses atoi/strtod split. Required defaults and stock checks are:

```rust
pub(crate) fn ccini_read_double_ext(raw: &[u8], x87: &mut X87Context) -> Option<Ext80> {
    let parsed_f32 = scan_f32_prefix(raw)?.value;
    let widened_f64 = x87.store_f64(Ext80::from_f32_bits(parsed_f32));
    if raw.contains(&b'%') {
        let product = x87.mul(
            Ext80::from_f64_bits(widened_f64),
            Ext80::from_f64_bits(NativeF64Bits::from_bits(
                0x3f84_7ae1_47ae_147b,
            )),
        );
        let percent_f64 = x87.store_f64(product);
        Some(Ext80::from_f64_bits(percent_f64))
    } else {
        Some(Ext80::from_f64_bits(widened_f64))
    }
}

pub(crate) fn ccini_read_double_f32(
    raw: &[u8],
    x87: &mut X87Context,
) -> Option<NativeF32Bits> {
    ccini_read_double_ext(raw, x87).map(|value| x87.store_f32(value))
}

pub(crate) fn ccini_read_double_f64(
    raw: &[u8],
    x87: &mut X87Context,
) -> Option<NativeF64Bits> {
    ccini_read_double_ext(raw, x87).map(|value| x87.store_f64(value))
}
```

The initial `store_f64`/reload is mandatory even without `%`: native loads the
parsed f32, spills it to a local f64, and reloads that local for the caller. The
percent branch performs a second f64 spill/reload after multiplication before
the caller's own destination store. Do not retain either value only as Ext80;
exceptional-value payload/quieting and store receipts are part of Task 4. The
`%` test follows the native contains-percent predicate; the numeric prefix is
still `%f`/binary32, never atoi. A malformed present value follows the captured
native/error contract; it does not silently become the missing-key default. The
caller selects f32 or f64 storage from the verified native store. Only Task 8's
Verses reader uses atoi for percent and strtod for bare tokens.

| Source | Key | Missing-key bits/value | Stock merged value |
|---|---|---:|---:|
| `[CombatDamage]` | `MaxDamage` | `1000` | `10000` |
| `[General]` | `VeteranCombat` | f64 `1.0` | `1.1` → widened-f32 f64 bits `0x3ff19999a0000000` |
| `[General]` | `VeteranArmor` | f64 `1.0` | `1.5` |
| `[AudioVisual]` | `ConditionYellow` | f64 `0.5` | `50%` → f64 `0.5` |
| `[AudioVisual]` | `ConditionRed` | f64 `0.25` | `25%` → f64 `0.25` |
| `[CombatDamage]` | `OccupyDamageMultiplier` | f32 `1.0` | `1.2` |
| `[CombatDamage]` | `BunkerDamageMultiplier` | f32 `1.0` | `1.3` |
| `[CombatDamage]` | `OpenToppedDamageMultiplier` | f32 `1.0` | `1.2` |
| country section | `Firepower`, `Armor` | f64 `1.0` | stock omitted/identity |
| country section | five `Armor*Mult` keys | f32 `1.0` | stock omitted/identity |

Parse `MinDamage` nowhere in `DamageRules` and add no runtime accessor for it.

**Step 3: Attach the groups without removing legacy fields**

Add `pub damage: DamageRules` to `RuleSet` and `pub damage:
CountryDamageRules` to `CountryRules`. Parse both in `RuleSet::from_ini`. Add
read-only accessors returning copies or shared references; do not expose the
private country map or duplicate its lookup normalization.

`RuleSet::from_ini` owns one parser `X87Context` and passes it through damage,
country, and warhead numeric readers in the exact order fixed by 1C/G0a. If the
current parser traversal order differs from native order, the refreshed child
task must add an explicit ordered damage-rule parse phase; a helper-local
context or a hard-coded assumed control word is not acceptable.

The existing `GarrisonRules` fields remain for legacy ROF/range and live damage
readers until Task 23. New damage code reads the exact fields only.

**Step 4: Add focused tests**

Add tests in `damage_rules.rs` and `ruleset.rs` for all 11 names, mixed case,
direct invalid-enum rejection, missing defaults, merged stock bits, and country
identity. Whole-RuleSet invalid-object rejection lands with the ObjectType field
in Task 9.

For `MaxDamage`, add every Task 4 `ReadInt` grammar/no-conversion/overflow case
and prove a missing key supplies current value 1000. These are separate from the
generic-double tests below.

The stock test asserts `VeteranCombat` is `0x3ff19999a0000000` and adds the
verified leading-prefix/trailing-junk cases. A direct-strtod `1.1` result
`0x3ff199999999999a` must fail this generic-reader test.

```rust
#[test]
fn missing_and_stock_max_damage_are_distinct() {
    let missing = IniFile::from_str("[General]\n[CombatDamage]\n[AudioVisual]\n");
    let mut missing_x87 = parser_context_fixture();
    assert_eq!(
        DamageRules::from_ini(&missing, &mut missing_x87)
            .unwrap()
            .max_damage,
        1000
    );

    let stock = IniFile::from_str(
        "[General]\nVeteranCombat=1.1\nVeteranArmor=1.5\n\
         [CombatDamage]\nMaxDamage=10000\nOccupyDamageMultiplier=1.2\n\
         BunkerDamageMultiplier=1.3\nOpenToppedDamageMultiplier=1.2\n\
         [AudioVisual]\nConditionRed=25%\nConditionYellow=50%\n",
    );
    let mut stock_x87 = parser_context_fixture();
    assert_eq!(
        DamageRules::from_ini(&stock, &mut stock_x87)
            .unwrap()
            .max_damage,
        10000
    );
}
```

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_rules -- --nocapture
cargo test -p vera20k ruleset -- --nocapture
```

Expected literal result for each command: `test result: ok.` Existing legacy
consumers still compile and behave unchanged.

### Task 8: Add Native Warhead Damage Inputs and Receiver Flags

**Why:** The live receiver needs full-precision Verses, exact spread/falloff
bits, and verified flags that the current `WarheadType` omits.

**Files:**
- Modify: `src/rules/warhead_type.rs`
- Modify: `src/rules/ruleset.rs` only where construction/validation requires it

**Pattern:** Nested additive rule group. Keep `verses`, `verses_f64`,
`cell_spread`, `percent_at_max`, and `prone_damage_basis_points` for old readers
until Task 23; new damage code reads only `WarheadDamageRules`.

**Dependency:** G0a and Tasks 2, 4, 5, 6, and 7. Delay-kill parsing may land before G1
wrapper authority, but its effects remain shadow-only until Task 2 passes.

**Step 1: Define the native group**

```rust
use crate::rules::damage_rules::ArmorClass;
use crate::rules::error::RulesError;
use crate::util::native_float::{
    Ext80, NativeF32Bits, NativeF64Bits, X87Context, atoi_i32_prefix,
    strtod_f64_prefix,
};

pub(crate) enum VersesIniValue<'a> {
    Missing,
    Present(&'a [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarheadDamageRules {
    pub verses: [NativeF64Bits; ArmorClass::COUNT],
    pub cell_spread: NativeF32Bits,
    pub percent_at_max: NativeF32Bits,
    pub penetrates_bunker: bool,
    pub psychic_damage: bool,
    pub affects_allies: bool,
    pub causes_delay_kill: bool,
    pub delay_kill_frames: i32,
    pub delay_kill_at_max: NativeF32Bits,
}
```

Add `pub damage: WarheadDamageRules` to `WarheadType`.

G0a provides one crate-private `parse_native_verses(source:
VersesIniValue<'_>, x87: &mut X87Context) -> Result<[NativeF64Bits;
ArmorClass::COUNT], RulesError>` implementation so Task 20 exercises the same
reader as `RuleSet`, never a test-only reimplementation.

**Step 2: Implement exact Verses parsing**

Initialize all 11 constructor entries to `NativeF64Bits::ONE`, then reproduce
the `ReadString`/token-loop distinction exactly:

- a missing `Verses` key makes `ReadString` supply the literal 11-token
  `100%%,...,100%%` fallback at `0x00847c40`; feed that fallback through the same
  atoi-times-0.01 loop rather than treating it as an absent parse;
- a present value whose native `ReadString` result length is zero (including the
  captured trimmed-empty forms) skips the loop and retains constructor ones;
- a present nonempty value is split with native `strtok(",")` semantics, which
  collapse leading, trailing, and consecutive empty fields. Require at least 11
  resulting tokens at the safe Rust boundary and process exactly the first 11
  with the shared parser `X87Context`.

Before the length test or `strtok`, model the exact `ReadString(..., size=0x80)`
buffer: copy/truncate as native bytes, force byte 127 to NUL, then apply the
Task 4-captured trim and return-length rules. Tokenize only that bounded result,
never the original unbounded Rust value. G0a must give this helper a byte-exact
route from `IniFile`; it may not truncate a UTF-8 string at a guessed character
boundary. A long value whose delimiter, percent sign, or token 11 crosses byte
127 can change both stored bits and whether the native loop faults.

For each processed token, use native handling from Task 4:

- if the token contains `%`, call `atoi_i32_prefix`, load that integer into the
  captured `X87Context`, multiply by f64 bits `0x3f847ae147ae147b`, and
  `store_f64` once;
- otherwise call `strtod_f64_prefix` and retain its binary64 result directly;
- extra post-`strtok` tokens are ignored after token 11;
- a present nonempty list with fewer than 11 post-`strtok` tokens returns
  `RulesError::InvalidValue`
  with an expected count of 11 as a memory-safe interim behavior. It is **not**
  parity-equivalent: the live loop executes 11 iterations without a null guard, and an
  exhausted `strtok` result is passed to `strchr`. Never identity-fill the
  missing tail. Missing-key fallback parsing and present-empty constructor
  retention can finish with the same one bits, but their mechanisms and Oracle
  receipts remain distinct. Task 20 reports this safe-error/native-fault pair as
  `DRIFT`, and G3 remains failed until G0a supplies a reviewed, memory-safe way
  to reproduce the captured externally visible termination semantics exactly.

Do not trim internal token whitespace unless the retail capture proves that the
native scanner does. Do not clamp negative or above-100% Verses values.

**Step 3: Parse exact defaults and flags**

Use these verified constructor defaults:

- CellSpread f32 `0.0`;
- PercentAtMax f32 `1.0`;
- Verses all f64 `1.0`;
- `PenetratesBunker=false`;
- `PsychicDamage=false`;
- `AffectsAllies=true`;
- `CausesDelayKill=false`;
- `DelayKillFrames=5`;
- `DelayKillAtMax` f32 `1.0`.

Keep existing `Psychedelic`, `MindControl`, `Poison`, and `Radiation` fields
separate; `PsychicDamage` is not an alias for `Psychedelic` or `MindControl`.

**Step 4: Add parser tests**

Cover percent integer truncation (`50.5%`, `0.5%`, `-50%`), bare fractional
values, missing-key fallback parsing, present empty/trimmed-empty retention,
native empty-token collapse, present nonempty short-list rejection, exact
11-token and extra-token lists, plus payload lengths 126/127/128 and truncation
inside a token/delimiter/percent suffix. Also cover default-true `AffectsAllies`, stock
`PenetratesBunker`, stock PsiPulse flags, and stock OilExplosionWH delay values.

```rust
#[test]
fn percent_verses_uses_integer_prefix_before_x87_multiply() {
    let mut x87 = parser_context_fixture();
    let rules = parse_native_verses(
        VersesIniValue::Present(
            b"50.5%,0.5%,-50%,0.505,100%,100%,100%,100%,100%,100%,100%",
        ),
        &mut x87,
    )
    .unwrap();
    assert_eq!(rules[0].bits(), 0x3fe0_0000_0000_0000);
    assert_eq!(rules[1].bits(), NativeF64Bits::ZERO.bits());
    assert_eq!(rules[2].bits(), 0xbfe0_0000_0000_0000);
    assert_eq!(rules[3].bits(), 0x3fe0_28f5_c28f_5c29);
}

#[test]
fn present_short_verses_is_rejected_without_claiming_parity() {
    let mut x87 = parser_context_fixture();
    assert!(
        parse_native_verses(VersesIniValue::Present(b"100%,100%"), &mut x87).is_err()
    );
}
```

Task 20 must confirm these raw outputs against retail captures before authority.

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k warhead_type -- --nocapture
```

Expected literal result: `test result: ok.` Existing warhead consumers still
read their legacy fields and live gameplay is unchanged.

### Task 9: Add Validated Object Damage Inputs and Native Ability Sets

**Why:** Receiver gates and attacker/defender veterancy cannot be reconstructed
from the current armor string, two fearless booleans, aircraft-only ammo, and a
single mind-control boolean.

**Files:**
- Modify: `src/rules/object_type.rs`
- Modify: `src/rules/ruleset.rs`

**Pattern:** Nested additive rule group on the existing cohesive data-heavy
`ObjectType`; substantial parsing helpers stay below the impl to avoid expanding
`ruleset.rs`.

**Dependency:** G0a and Tasks 1, 2, 5, 6, and 7. Do not start this task until G1 has
resolved missing-key ability arrays, Building `ImmuneToPsionics`, general
`Immune`, and the disputed Building precheck key. If any remains unresolved,
leave Task 9 blocked instead of encoding a universal default.

**Step 1: Define ability and object damage types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeAbilitySet(u32);

impl NativeAbilitySet {
    pub const STRONGER: u8 = 1;
    pub const FIREPOWER: u8 = 2;

    pub fn contains(self, index: u8) -> bool {
        self.0 & (1_u32 << index) != 0
    }

    fn insert(&mut self, index: u8) {
        self.0 |= 1_u32 << index;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectDamageRules {
    pub armor: Option<ArmorClass>,
    pub insignificant: bool,
    pub immune: bool,
    pub type_immune: bool,
    pub immune_to_psionics: bool,
    pub immune_to_psionic_weapons: bool,
    pub immune_to_poison: bool,
    pub immune_to_radiation: bool,
    pub eligible_for_delay_kill: bool,
    pub damage_reduces_readiness: bool,
    pub readiness_reduction_multiplier: NativeF32Bits,
    pub initial_ammo: i32,
    pub veteran_abilities: NativeAbilitySet,
    pub elite_abilities: NativeAbilitySet,
}
```

Add the exact Task 2 Building-precheck field under its verified INI/native name;
do not map it to `Unsellable=` solely because an older report used the phrase
“UnsellableTransport.” The `/review-plan` pass after Task 2 inserts that one
verified field name into this struct and its tests before Task 14 starts.

**Step 2: Parse armor without changing the current constructor signature**

Keep `armor: String` temporarily for legacy consumers. Populate
`damage.armor = ArmorClass::parse(id, &armor).ok()`. At RuleSet completion,
iterate every loaded object and convert `None` into
`RulesError::InvalidValue` with its section, `Armor` key, expected 11-name set,
and raw value. Simulation damage views access the typed field only after this
RuleSet validation; delete `armor_index`'s unknown-to-zero behavior in Task 23.

**Step 3: Parse native ability tokens**

Parse the full 18-entry Veteran and Elite arrays. Split only at commas; compare
tokens case-insensitively but do not trim each token. Unknown tokens are ignored.
Apply the exact missing-key default arrays proved by Task 1. An elite entity's
effective set is Veteran OR Elite.

```rust
fn parse_native_ability_set(raw: &str, inherited: NativeAbilitySet) -> NativeAbilitySet {
    let mut result = inherited;
    for token in raw.split(',') {
        if let Some(index) = native_ability_index(token) {
            result.insert(index);
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VeterancyAbilityView {
    is_veteran: bool,
    is_elite: bool,
}

fn effective_abilities(
    veterancy: VeterancyAbilityView,
    veteran: NativeAbilitySet,
    elite: NativeAbilitySet,
) -> NativeAbilitySet {
    if veterancy.is_elite {
        NativeAbilitySet(veteran.0 | elite.0)
    } else if veterancy.is_veteran {
        veteran
    } else {
        NativeAbilitySet::default()
    }
}
```

`VeterancyAbilityView` is supplied by GV from the native f32 accumulator and
the verified 1.0f/2.0f comparisons. The current Rust `100/200` surrogate is not
an input to this function and must not survive G0a.

**Step 4: Parse receiver flags and readiness inputs**

Use the exact constructor/derived defaults from Task 1. Known stock/default
facts include TypeImmune false, ImmuneToPsionicWeapons false, ImmuneToPoison
false, EligibleForDelayKill false, DamageReducesReadiness false,
ReadinessReductionMultiplier f32 zero, and InitialAmmo `-1`. Preserve the
existing `Ammo=` field separately until Task 22 creates the runtime initial
ammo rule.

**Step 5: Add focused tests**

Cover all armor classes and invalid RuleSet rejection; exact/no-trim ability
tokens; case-insensitivity; unknown-token ignore; elite OR veteran; every new
immunity flag; readiness defaults; and stock delay-kill eligibility.

```rust
#[test]
fn native_ability_parser_does_not_trim_tokens() {
    let inherited = NativeAbilitySet::default();
    let parsed = parse_native_ability_set("FASTER, STRONGER,FIREPOWER", inherited);
    assert!(!parsed.contains(NativeAbilitySet::STRONGER));
    assert!(parsed.contains(NativeAbilitySet::FIREPOWER));
}
```

**Step 6: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k object_type -- --nocapture
cargo test -p vera20k ruleset -- --nocapture
```

Expected literal result for each command: `test result: ok.` Live simulation
still consumes legacy fields only.

### Task 10: Define Raw Calls, Views, Results, and Ordered Effect Contracts

**Why:** Interfaces must make double application, missing source objects,
PostMortem, target disappearance, and effect order explicit before formulas are
rewritten.

**Files:**
- Create: `src/sim/combat/damage/types.rs`
- Create: `src/sim/combat/damage/views.rs`
- Create: `src/sim/combat/damage/effects.rs`
- Create: `src/sim/combat/damage/runtime.rs`
- Modify: `src/sim/combat/damage/mod.rs`

**Pattern:** Replace the current monolithic test-only structs with small Copy
call/view types. No type in these files owns EntityStore, RNG, strings, or a
presentation subsystem.

**Dependency:** G0a and Tasks 1, 2, 7, 8, and 9. The effect enum contains only calls and
writes verified in the two G1 reports.

**Step 1: Define the call and result types**

```rust
// src/sim/combat/damage/types.rs
use crate::sim::intern::InternedId;

/// Native Cartesian world `CoordStruct`; every component is signed leptons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorldLeptonCoord {
    pub x_leptons: i32,
    pub y_leptons: i32,
    pub z_leptons: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageWarheadRef {
    Resolved(InternedId),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSourceKind {
    ProjectileImpact,
    AreaDetonation,
    DeathWeapon,
    Radiation,
    LightningStorm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageEvent {
    pub target_id: u64,
    pub source_object_id: Option<u64>,
    pub source_house: Option<InternedId>,
    pub warhead: DamageWarheadRef,
    pub incoming_damage: i32,
    pub distance_leptons: i32,
    pub ignore_defenses: bool,
    pub source_kind: DamageSourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DamageResultState {
    Unaffected = 0,
    Damaged = 1,
    Yellow = 2,
    Red = 3,
    Dead = 4,
    PostMortem = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageBlockReason {
    None,
    TargetGone,
    ZeroIncoming,
    ScenarioNoDamage,
    NullWarhead,
    HealingArmor,
    TypeImmune,
    Insignificant,
    IronCurtain,
    WarpingOut,
    BunkerRedirect,
    PsionicImmune,
    PsionicWeaponImmune,
    PoisonImmune,
    RadiationImmune,
    AlliedSource,
    ConcretePrecheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageOutcome {
    pub final_damage: i32,
    pub result: DamageResultState,
    pub block: DamageBlockReason,
}
```

`final_damage > 0` means HP to subtract; `< 0` means healing to add. A deliberate
`Null` reaches the native early return. Named but unresolved INI references
remain a RuleSet validation error and never become `Null`.

**Step 2: Define immutable views**

`views.rs` groups only values read by each native stage:

```rust
#[derive(Debug, Clone, Copy)]
pub struct FireDamageView {
    pub weapon_damage: i32,
    pub house_firepower: NativeF64Bits,
    pub unit_firepower: NativeF64Bits,
    pub veteran_combat: Option<NativeF64Bits>,
    pub civilian_garrison: Option<NativeF32Bits>,
    pub tank_bunker: Option<NativeF32Bits>,
    pub open_topped: Option<NativeF32Bits>,
    pub stores_zero_for_special_path: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetDamageView {
    pub stable_id: u64,
    pub owner: InternedId,
    pub category: EntityCategory,
    pub armor: ArmorClass,
    pub health: i32,
    pub strength: i32,
    pub veterancy_bits: NativeF32Bits,
    pub runtime: DamageRuntimeView,
    pub type_rules: ObjectDamageView,
}

#[derive(Debug, Clone, Copy)]
pub struct WarheadDamageView {
    pub rules: WarheadDamageRules,
    pub psychedelic: bool,
    pub mind_control: bool,
    pub poison: bool,
    pub radiation: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DamageContextView {
    pub event: DamageEvent,
    pub scenario_no_damage: bool,
    pub target: TargetDamageView,
    pub source_owner: Option<InternedId>,
    pub source_is_allied: bool,
    pub warhead: Option<WarheadDamageView>,
    pub rules: DamageRules,
}
```

`source_is_allied` is meaningful only when `source_object_id` and
`source_owner` are both present; constructors assert that invariant in debug
tests and set false for an absent source object.

`veterancy_bits` is the active gamemd f32 accumulator, not the current Rust
0/100/200 surrogate. GV may instead make the view carry exact precomputed
ability predicates, but G0a must not retain a `u16` approximation.

**Step 3: Define ordered effect intents**

`effects.rs` contains one named variant for each verified write/call. The exact
payload fields and numeric trigger values come from Tasks 1-2. The minimum
surface is:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageTriggerCall {
    pub event_id: u8,
    pub target_id: u64,
    pub source_object_id: Option<u64>,
    pub source_house: Option<InternedId>,
    pub argument: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageEffect {
    SetHealth { target_id: u64, health: i32 },
    SetAmmo { target_id: u64, ammo: i32 },
    RequestAmmoDepletedAnimation { target_id: u64 },
    SetLastAttacker { target_id: u64, source_object_id: Option<u64> },
    SetLastDamage { target_id: u64, tick: u32, distance_leptons: i32 },
    SetWasAttacked { target_id: u64, value: bool },
    SetFear { target_id: u64, fear: i32 },
    SetRetaliationTarget { target_id: u64, source_object_id: Option<u64> },
    CreateDamageParticle { target_id: u64, particle_system: InternedId },
    RemoveDamageParticle { target_id: u64 },
    DispatchDamageTrigger(DamageTriggerCall),
    RecordKillByObject { target_id: u64, source_object_id: u64 },
    RecordKillByHouse { target_id: u64, source_house: InternedId },
    MarkDestroyed { target_id: u64 },
    SetDelayKill { target_id: u64, state: DelayKillState },
    ClearDelayKill { target_id: u64 },
    RequestConcreteEffect { target_id: u64, effect: ConcreteDamageEffect },
    RequestLifecycle { target_id: u64, action: DamageLifecycleAction },
}
```

`SetFear` uses the Task 1-verified native integer width and includes a 300-value
fixture. Kill attribution remains distinct from destruction marking because the
native object and house callbacks have different downstream behavior. GV must
provide the exact kill-XP/veterancy callback before either record effect becomes
authoritative. Particle intents remain non-live until GP proves complete
serialization and hashing of the authoritative particle store.

`ConcreteDamageEffect` has explicit variants for the verified light, sound,
animation, debris/fire/spark RNG request, garrison/passenger, and class-specific
effects. `DamageLifecycleAction` has explicit verified states such as conceal,
limbo, uninit, mark-delete, or destruction-effects call; include only the
actions Task 2 proves. A generic catch-all variant is forbidden.

**Step 4: Define the resumable pure receiver machine**

The receiver never precomputes past a callback/effect that can change a later
read. It emits one effect/checkpoint at a time and resumes from an explicit
stage against a refreshed owned context.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcreteReceiverKind {
    Infantry,
    Unit,
    Aircraft,
    Building,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageMachineStage {
    ConcretePrecheck,
    TechnoPrefix,
    ObjectCore,
    TechnoAftermath,
    ConcretePost,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageMachine {
    pub event: DamageEvent,
    pub kind: ConcreteReceiverKind,
    pub stage: DamageMachineStage,
    pub stage_cursor: u16,
    pub current_damage: i32,
    pub predicted_health: i32,
    pub outcome: Option<DamageOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageMachineStep {
    Apply {
        effect: DamageEffect,
        refresh_context: bool,
    },
    Redirect {
        target_id: u64,
        post_attacker_damage: i32,
    },
    Complete(DamageOutcome),
}

impl DamageMachine {
    pub(crate) fn begin(kind: ConcreteReceiverKind, context: &DamageContextView) -> Self;
    pub(crate) fn advance(
        &mut self,
        context: &DamageContextView,
        tick: u32,
        x87: &mut X87Context,
    ) -> DamageMachineStep;
}
```

G0a replaces `stage_cursor`'s documented numeric meanings with the literal
Task 1/Task 2 branch table and tests every transition. If research shows a
callback can mutate a later read, its emitted step sets `refresh_context=true`;
the resolver must rebuild the Copy context before the next `advance` call.
Each call yields at most one Copy effect, and the resolver releases all machine
borrows before applying it. A nested damage call therefore owns a separate
local `DamageMachine` on the Rust call stack, matching native synchronous
re-entry; there is no single shared program Vec to overwrite. Shadow drivers
stop at the first executor-dependent effect unless they own a persistent cloned
state capable of applying it.

**Step 5: Define runtime state types without attaching them**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DelayKillState {
    pub started_tick: u32,
    pub expires_tick: u32,
    pub selected_delay: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DamageRuntimeState {
    pub last_damage_tick: u32,
    pub last_damage_distance: i32,
    pub was_attacked: bool,
    pub delay_kill: Option<DelayKillState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HouseDamageState {
    pub firepower: NativeF64Bits,
    pub armor_infantry: NativeF32Bits,
    pub armor_units: NativeF32Bits,
    pub armor_aircraft: NativeF32Bits,
    pub armor_buildings: NativeF32Bits,
    pub armor_defenses: NativeF32Bits,
}
```

Use the exact Task 1 field widths and add any proven receiver-consumed state in
the `/review-plan` revision before implementation. Do not attach either struct
to `GameEntity`/`HouseState` or derive a default until Task 22 establishes exact
initialization and snapshot ownership.

Per-unit firepower/armor, native veterancy/FearLevel, ammo/readiness,
controller, and last-attacker fields are deliberately absent: GS owns their one
authoritative representation and every gameplay writer. Do not duplicate them
inside `DamageRuntimeState`.

**Step 6: Make `damage/mod.rs` a façade**

Declare the new modules, but do not re-export names that collide with the old
`CombatMods`, `ImmunityInputs`, tuple `ArmorClass`, `DamageGate`, `DamageState`,
or `DamageOutcome` yet. Existing attacker/gates/kernel/receive files continue
to compile against those compatibility definitions until Tasks 11-13 rewrite
their final consumers. New tests import through `damage::types` and
`damage::views`. Task 13 removes the compatibility definitions and turns
`mod.rs` into the final façade.

**Step 7: Add contract tests**

Test null versus resolved warhead, signed incoming/final damage, all six result
states, source-object absent with source house present, allied invariant, and
stable discriminants for native receiver results.

**Step 8: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k sim::combat::damage -- --nocapture
```

Expected literal result: `test result: ok.` `rg` finds no production caller of
the new resolver because it does not exist yet.

### Task 11: Implement Exact Attacker Stages and the Warhead Kernel

**Why:** These are the stable, already-verified numeric cores and expose x87 or
rule-bit errors before the larger receiver envelope is added.

**Files:**
- Create: `src/sim/combat/damage/native_math.rs`
- Modify: `src/sim/combat/damage/mod.rs` to declare/re-export `native_math`
- Rewrite: `src/sim/combat/damage/attacker.rs`
- Rewrite: `src/sim/combat/damage/kernel.rs`

**Pattern:** Small pure functions over Copy views. They read no EntityStore,
consume no RNG, allocate nothing, and use no host floats.

**Dependency:** G0a and Tasks 4-10. Task 20 remains the parity certificate; unit tests in
this task are regression and operation-order checks.

**Step 1: Add native expression helpers**

```rust
// src/sim/combat/damage/native_math.rs
use crate::util::native_float::{Ext80, NativeF32Bits, NativeF64Bits, X87Context};

#[inline]
pub(crate) fn mul_f32_ftol(
    x87: &mut X87Context,
    value: i32,
    multiplier: NativeF32Bits,
) -> i32 {
    let product = x87.mul(Ext80::from_i32(value), Ext80::from_f32_bits(multiplier));
    x87.math_ftol(product).eax_i32()
}

#[inline]
pub(crate) fn mul_f64_ftol(
    x87: &mut X87Context,
    value: i32,
    multiplier: NativeF64Bits,
) -> i32 {
    let product = x87.mul(Ext80::from_i32(value), Ext80::from_f64_bits(multiplier));
    x87.math_ftol(product).eax_i32()
}

#[inline]
pub(crate) fn div_f32_ftol(
    x87: &mut X87Context,
    value: i32,
    divisor: NativeF32Bits,
) -> i32 {
    let quotient = x87.div(Ext80::from_i32(value), Ext80::from_f32_bits(divisor));
    x87.math_ftol(quotient).eax_i32()
}

#[inline]
pub(crate) fn div_f64_ftol(
    x87: &mut X87Context,
    value: i32,
    divisor: NativeF64Bits,
) -> i32 {
    let quotient = x87.div(Ext80::from_i32(value), Ext80::from_f64_bits(divisor));
    x87.math_ftol(quotient).eax_i32()
}
```

Do not check a divisor for zero unless the native body does; masked x87 behavior
and `ftol` own that result.

**Step 2: Implement `Fire_At` attacker arithmetic**

```rust
// src/sim/combat/damage/attacker.rs
pub(crate) fn fire_damage(view: FireDamageView, x87: &mut X87Context) -> i32 {
    let mut damage = if view.stores_zero_for_special_path {
        0
    } else if view.weapon_damage > 0 {
        let house_times_unit = x87.mul(
            Ext80::from_f64_bits(view.house_firepower),
            Ext80::from_f64_bits(view.unit_firepower),
        );
        let scaled = x87.mul(house_times_unit, Ext80::from_i32(view.weapon_damage));
        let mut scaled = x87.math_ftol(scaled).eax_i32();
        if let Some(multiplier) = view.veteran_combat {
            scaled = mul_f64_ftol(x87, scaled, multiplier);
        }
        scaled
    } else {
        view.weapon_damage
    };

    if let Some(multiplier) = view.civilian_garrison {
        damage = mul_f32_ftol(x87, damage, multiplier);
    }
    if let Some(multiplier) = view.tank_bunker {
        damage = mul_f32_ftol(x87, damage, multiplier);
    }
    if let Some(multiplier) = view.open_topped {
        damage = mul_f32_ftol(x87, damage, multiplier);
    }
    damage
}
```

Only strictly positive ordinary weapon damage enters the country/per-unit and
veteran stages. Their native grouping is `(house * unit) * weapon_damage`, with
one conversion after all three factors; changing it to `(damage * house) * unit`
is drift under PC rounding. Zero and negative ordinary damage preserve the raw
weapon value through those stages. The Wave/special path stores zero and skips
those stages, but it does **not** return: every enabled civilian-garrison,
bunker, and open-top stage still executes, including its `Math__ftol` control-
word effect. Every enabled veteran/containment stage has its own conversion.
Predicate construction uses Task 1/Task 2 evidence; the function does not guess
containment from entity category alone.

**Step 3: Implement the kernel in instruction order**

```rust
// src/sim/combat/damage/kernel.rs
pub(crate) struct KernelInput {
    pub incoming_damage: i32,
    pub distance_leptons: i32,
    pub armor: ArmorClass,
    pub cell_spread: NativeF32Bits,
    pub percent_at_max: NativeF32Bits,
    pub verses: NativeF64Bits,
    pub max_damage: i32,
}

pub(crate) fn apply_warhead_damage(input: KernelInput, x87: &mut X87Context) -> i32 {
    if input.incoming_damage < 0 {
        return if input.armor.index() >= ArmorClass::Concrete.index() {
            0
        } else {
            input.incoming_damage
        };
    }

    let raw_base = Ext80::from_i32(input.incoming_damage);
    let stored_base = x87.store_f32(raw_base);
    let percent_product = x87.mul(
        raw_base,
        Ext80::from_f32_bits(input.percent_at_max),
    );
    let stored_percent_damage = x87.store_f32(percent_product);
    let base = Ext80::from_f32_bits(stored_base);
    let percent_damage = Ext80::from_f32_bits(stored_percent_damage);
    let spread_product = x87.mul(
        Ext80::from_f32_bits(input.cell_spread),
        Ext80::from_f32_bits(NativeF32Bits::from_bits(0x4380_0000)),
    );
    let spread_leptons = x87.math_ftol(spread_product).eax_i32();

    let falloff = if matches!(
        x87.compare(percent_damage, base),
        X87Compare::Less | X87Compare::Greater
    )
        && spread_leptons != 0
    {
        let delta = x87.sub(base, percent_damage);
        let scaled = x87.mul(
            delta,
            Ext80::from_i32(spread_leptons.wrapping_sub(input.distance_leptons)),
        );
        let divided = x87.div(scaled, Ext80::from_i32(spread_leptons));
        let interpolated = x87.add(divided, percent_damage);
        x87.math_ftol(interpolated).eax_i32()
    } else {
        input.incoming_damage
    };

    let nonnegative = if falloff <= 0 { 0 } else { falloff };
    let versed_product = x87.mul(
        Ext80::from_i32(nonnegative),
        Ext80::from_f64_bits(input.verses),
    );
    let versed = x87.math_ftol(versed_product).eax_i32();
    if versed >= input.max_damage {
        input.max_damage
    } else {
        versed
    }
}
```

The two explicit `store_f32` calls model the native `FST float` boundaries
for damage-as-float and damage×PercentAtMax. The falloff expression reloads
those stored f32 values; retaining either value as an extended temporary would
be mechanism drift.

Before merging, compare the `wrapping_sub` instruction behavior and the healing
predicate to the Task 4 raw capture. If live assembly shows the healing test is
distance rather than armor despite the corrected design/GATE report, stop and
resolve the document conflict; do not choose one based on prose age.

The `matches!(Less | Greater)` guard is deliberate: the native `TEST AH,0x40`
skips the falloff branch for both Equal and Unordered. A `!= Equal` predicate is
wrong for NaN inputs.

Zero incoming, scenario-no-damage, and null-warhead checks remain in the
envelope because they need block diagnostics, but their order must match the
kernel entry in Task 12.

**Step 4: Replace hand-computed tests with stage-boundary tests**

Cover the exact worked fixture (100 base, PAM 0.25, distance 128, Verses 0.5 →
31), flat damage, spread zero, edge/over-edge, double truncation, healing
armor 7/8, fractional Verses, negative Verses, and cap below/equal/above.

Add attacker tests that capture each intermediate by invoking prefixes of the
view: baseline, `(house * unit) * damage`, a PC-rounding-sensitive regrouping
counterexample, veteran, negative Heal/RepairBullet, ordinary zero, civilian
garrison, bunker, open-topped, bunker+open-top, and special stored-zero with an
enabled containment stage. The negative/zero cases prove country/per-unit and
veteran scaling were skipped; the special case proves containment and its
`Math__ftol` control-word effect still ran.

**Step 5: Prove no host float or ProneDamage remains in the new core**

```powershell
rg -n "\bf32\b|\bf64\b| as i32|ProneDamage|prone_damage" src/sim/combat/damage
```

Expected: no host-float arithmetic or ProneDamage consumption in non-test
damage code. Bit-wrapper names and comments are permitted.

**Step 6: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k sim::combat::damage::attacker -- --nocapture
cargo test -p vera20k sim::combat::damage::kernel -- --nocapture
```

Expected literal result for each command: `test result: ok.` No production
health write changes.

### Task 12: Implement the Techno Numeric Prefix and Receiver Gates

**Why:** Defender modifiers, minimum-one, readiness, bunker routing, immunity,
and ally semantics must run in one verified order before Object HP logic.

**Files:**
- Rewrite: `src/sim/combat/damage/gates.rs`
- Create/modify: `src/sim/combat/damage/envelope.rs`
- Modify: `src/sim/combat/damage/mod.rs` to declare/re-export `envelope`
- Modify: `src/sim/combat/damage/effects.rs`

**Pattern:** One-step pure stage transitions over immutable owned views. A call
yields at most one Copy effect and never applies it to the world.

**Dependency:** G0a, Tasks 10-11, and G1 from Task 1 must pass. Run
`/review-plan` on this plan after Task 1 so the exact branch table, field names,
callback refresh result, machine/view contracts, and native arithmetic helpers
are checked before code is written.

**Step 1: Define the Techno-prefix transition**

```rust
pub(crate) fn advance_techno_prefix(
    machine: &mut DamageMachine,
    context: &DamageContextView,
    x87: &mut X87Context,
) -> DamageMachineStep;
```

The helper advances exactly one 1A table row or emits one effect. It updates
`machine.stage_cursor` only when that row is complete. A bunker redirect yields
`DamageMachineStep::Redirect`; the resolver handles it synchronously without
reapplying attacker modifiers. There is no effect Vec or whole-stage result to
precompute past a callback.

**Step 2: Implement the exact verified sequence**

The body follows Task 1's numbered table exactly:

1. zero incoming, scenario no-damage, and null-warhead early returns in their
   verified owner/order;
2. only under the exact Task 1 positive-damage/`ignore_defenses` predicate,
   apply the country-category and per-unit armor divisor in the verified
   grouping, then `ftol`;
3. under that same positive-only region, apply the VeteranArmor divisor when
   its exact ability predicate is true, then `ftol`;
4. under that same region, apply minimum-one only to a positive incoming path;
5. TypeImmune and every `ignore_defenses`-sensitive defense branch;
6. readiness/ammo mutation and depleted-animation intent, while preserving the
   damage value and continuing;
7. bunker redirect/force-shield handling;
8. psionic, psionic-weapon, poison, radiation, and other verified warhead/type
   immunity branches;
9. `AffectsAllies`: test only when `source_object_id.is_some()` and use the
   source object's owner relation; and
10. psychedelic/mind-control result/effect behavior as separate verified
    branches.

Use `div_f32_ftol`/`div_f64_ftol` with no protective zero guard. At every return,
set a specific `DamageBlockReason`; no generic nullification result is allowed.
Negative healing bypasses every positive-only defender divide and minimum-one
stage exactly as the verified body does; it is never converted to +1. G0a copies
the exact predicate from 1A into the generated transition table.

**Step 3: Preserve readiness side-effect order**

Update the machine's predicted ammo value, yield `SetAmmo`, then on the next
advance yield the verified animation request if its predicate fires. Continue
with the same HP damage.
If Task 1 proves a callback can affect a later read, return a stage checkpoint
to the resolver instead of precomputing past it; the `/review-plan` revision
must show that code shape before implementation.

**Step 4: Add one test per branch and interaction**

Tests include country/unit/veteran divisors, positive min-one, every gate with
both `ignore_defenses` values, readiness no-HP-absorption, bunker redirect,
source-present ally, source-absent allied house, and psychedelic behavior.

Add paired shell/occupant fixtures with `PenetratesBunker=false` and `true`, plus
both `ignore_defenses` values wherever 1A proves an interaction. Assert the
exact receiver object, shell callback behavior, redirect/no-redirect result,
and that attacker arithmetic is not applied twice.

```rust
#[test]
fn attackerless_source_house_does_not_activate_affects_allies_gate() {
    let context = allied_context_with_no_source_object();
    let mut machine = DamageMachine::begin(ConcreteReceiverKind::Unit, &context);
    advance_until_prefix_exits(&mut machine, &context);
    assert_eq!(machine.current_damage, context.event.incoming_damage);
}
```

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k sim::combat::damage::gates -- --nocapture
```

Expected literal result: `test result: ok.` No live producer calls the stage.

### Task 13: Implement Object HP Core, Classification, Credit, and Triggers

**Why:** One owner must perform kernel invocation, signed health transition,
native result classification, lethal attribution, and ordered damage events.

**Files:**
- Rewrite: `src/sim/combat/damage/receive.rs`
- Modify: `src/sim/combat/damage/effects.rs`
- Modify: `src/sim/combat/damage/mod.rs` to remove the Task 10 compatibility
  definitions only after their Task 13 replacements compile

**Pattern:** Pure Object-stage program that emits exact typed trigger calls. GT
owns attached tags, trigger/action maps, and synchronous execution; do not
reinterpret native damage events as the current polling event subset.

**Dependency:** G0a, G1, and Tasks 7-12. The exact trigger table and callback
re-entrancy result come from Task 1.

**Step 1: Define the Object-stage entry**

```rust
pub(crate) fn advance_object_core(
    machine: &mut DamageMachine,
    context: &DamageContextView,
    x87: &mut X87Context,
) -> DamageMachineStep;
```

Each call executes one numbered Object row or yields one effect. The machine
stores post-Techno damage, predicted health, classification, and the next row;
no whole effect list is built.

**Step 2: Implement entry and kernel ownership**

Follow Task 1's order for alive/damage-zero/Insignificant checks. Call
`apply_warhead_damage` only when the verified `ignore_defenses` branch does so.
Apply the Building-only post-kernel minimum-one under its exact verified flag
gate. No non-Building minimum is added at this stage.

**Step 3: Implement signed health transition exactly**

- For negative final damage, add healing and cap at Strength in the exact owner
  and position proved by Task 1.
- For positive damage, use the inclusive native comparison: when damage is
  greater than or equal to current HP, assign remaining HP as damage before the
  health write. Do not use `saturating_sub`.
- Yield the verified health callback/effect at its native position, refresh the
  owned context if Task 1 proves re-entrancy, then yield `SetHealth` once on the
  next verified transition.
- Preserve integer widths, signedness, and wrapping behavior from the native
  body.

**Step 4: Classify every result state**

Implement Unaffected, Damaged, Yellow, Red, and Dead using the verified crossing
tests. Yellow uses the integer `Strength >> 1` boundary. Red uses exact
`Strength * ConditionRed` x87/double grouping and comparison; use the separate
double `ConditionYellow` wherever Task 1 proves the aftermath reads it. Do not collapse a
crossing result into a final-health band if native checks old and new health.
PostMortem remains a Task 15 concrete-wrapper result.

**Step 5: Emit lethal bookkeeping and triggers in exact order**

Yield distinct kill-credit and destruction effects plus each Task 1 trigger call
with its exact numeric ID, source object, house, and argument payload. Emit both
`0x29` calls as separate ordered effects. `DamageTriggerCall` was defined in
Task 10 so `DamageEffect` compiles when introduced; Task 13 fills its verified
IDs/payloads and does not redeclare it.

`DamageEffect::DispatchDamageTrigger(DamageTriggerCall)` is executed through the
GT-supplied synchronous adapter. If trigger execution can mutate a later
receiver read, the machine emits a refresh checkpoint and the resolver rebuilds
the target/source view before continuing. Task 13 can compile/test pure order
before GT lands; Task 22 is blocked on GT.

**Step 6: Remove the temporary root compatibility types**

After `receive.rs` compiles against the new contracts, all four old stage
consumers have been rewritten. Delete the old `CombatMods`, `ImmunityInputs`,
tuple `ArmorClass`, `DamageGate`, `DamageState`, and old `DamageOutcome` from
`damage/mod.rs`; re-export the new types/views/effects explicitly. Run `rg` for
each deleted name before proceeding and update only the new qualified imports.

**Step 7: Add exhaustive Object tests**

Cover entry early-outs, deliberate null, negative healing and cap, special
armor healing rejection, Building minimum-one, non-Building zero, damage equal
to HP, damage above HP, every crossing result, kill credit, callback order, and
the full ordered trigger sequence including distinct `0x29` records.

**Step 8: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k sim::combat::damage::receive -- --nocapture
cargo test -p vera20k damage_trigger_intents -- --nocapture
```

Expected literal result for each command: `test result: ok.` No trigger action
is wired into live world state yet.

### Task 14: Implement Techno Aftermath as Ordered Effect Intents

**Why:** Matching HP while omitting last-damage fields, flee/readiness, particle
maintenance, retaliation, threat, or `WasAttacked` remains receiver DRIFT.

**Files:**
- Modify: `src/sim/combat/damage/envelope.rs`
- Modify: `src/sim/combat/damage/effects.rs`
- Modify: `src/sim/combat/damage/runtime.rs`

**Pattern:** A pure one-step aftermath stage that yields named intents after the
Object result and before concrete wrapper result handling.

**Dependency:** G0a, G1, and Tasks 10-13.

**Step 1: Define the aftermath entry**

```rust
pub(crate) fn advance_techno_aftermath(
    machine: &mut DamageMachine,
    context: &DamageContextView,
    tick: u32,
    x87: &mut X87Context,
) -> DamageMachineStep;
```

**Step 2: Transcribe the verified order**

For each non-early-return result, yield the exact Task 1 intents one at a time for
last-damage tick/distance, flee/scatter or readiness behavior, threat and
retaliation bookkeeping, particle create/remove/lifetime, and `WasAttacked`.
Preserve every result-specific early return. Do not reuse current Phase-4 fear
or last-attacker logic unless Task 1 proves the same predicate and position.

Particle requests carry a resolved interned particle-system ID. Missing named
content is a RuleSet validation failure; native fallback systems are resolved
from `RuleSet::combat_damage` before the event reaches sim.

**Step 3: Add order tests**

For each Object result, assert the exact effect sequence and absence/presence of
each field write. Include attackerless damage, allied source, particle already
present, damage-band transition, and flee/readiness predicates.

**Step 4: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k techno_aftermath -- --nocapture
```

Expected literal result: `test result: ok.` No RNG or live entity mutation
occurs in the pure stage.

### Task 15: Implement Concrete Wrappers and PostMortem Programs

**Why:** Concrete-class prechecks, result effects, RNG requests, and lifecycle
ownership complete the receiver mechanism and determine whether Dead remains
Dead or becomes PostMortem.

**Files:**
- Modify: `src/sim/combat/damage/envelope.rs`
- Modify: `src/sim/combat/damage/effects.rs`
- Modify: `src/sim/combat/damage/runtime.rs`

**Pattern:** One explicit wrapper function per concrete class, sharing the
Techno/Object core but not a guessed common lethal handler.

**Dependency:** G0a, G1 from Task 2, and Tasks 10-14. Run `/review-plan` after Task 2
to verify wrapper names, payloads, and lifecycle sequence before coding.

**Step 1: Define wrapper dispatch without C++ inheritance**

```rust
pub(crate) fn advance_concrete_wrapper(
    machine: &mut DamageMachine,
    context: &DamageContextView,
    tick: u32,
    x87: &mut X87Context,
) -> DamageMachineStep;
```

Use the `ConcreteReceiverKind` defined in Task 10; do not redeclare it. This
helper handles one concrete pre/post row. `DamageMachine::advance` dispatches
between the wrapper, Techno prefix, Object core, Techno aftermath, and concrete
post stages in native order and yields after every effect. It is Rust-native
orchestration, not a vtable/inheritance copy.

**Step 2: Implement one wrapper per class**

Each wrapper transcribes Task 2's numbered table, including result-specific
sound/light/animation intents, exact RNG request owner/order, debris/fire/spark
effects, retaliation/threat, passenger/garrison cleanup, destruction effects,
and lifecycle requests. Every class has its own Dead path and membership
assertions.

The Building wrapper uses the verified field behind its precheck and the real
`DestructionEffects` call chain. It must not call `Limbo` solely from the stale
`+0x4EC` label.

**Step 3: Implement PostMortem at the verified Building position**

After the Object result and any preceding Building effects proved by Task 2,
evaluate `EligibleForDelayKill` plus warhead `CausesDelayKill`. Use the exact
control-aware x87 operation order to select delay, apply repeated-hit selection,
yield the exact latch/timer writes one at a time, restore health/life to the verified values, retain or undo
the effects named by Task 2, and return `DamageResultState::PostMortem`.

Timer expiry is represented by an explicit `DelayKillState` consumer owned by
the Task 2-verified tick system. It is not folded into ordinary healing.

**Step 4: Add wrapper tests**

For Infantry, Unit, Aircraft, and Building, cover nonlethal and lethal results,
effect order, RNG request order, and lifecycle intent sequence. For Building,
cover every precheck/result band, ordinary lethal, eligible/ineligible delay
kill, endpoints, repeated shorter/longer hit, and timer expiry.

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k concrete_receiver -- --nocapture
cargo test -p vera20k post_mortem -- --nocapture
```

Expected literal result for each command: `test result: ok.` All effects remain
intents; live RNG/state is untouched.

### Task 16: Split Area Collection from Receiver Math

**Why:** Native area dispatch records ordered target/distance facts, then sends
the same incoming damage to each receiver. Current Rust instead computes final
losses, quantizes distance to cells, and mixes collection with ProneDamage and
Verses.

**Files:**
- Modify: `src/sim/combat/combat_aoe.rs`
- Create: `src/sim/combat/combat_aoe/collector.rs`
- Create: `src/sim/combat/combat_aoe/layer.rs`
- Create: `src/sim/combat/combat_aoe/tests.rs`

**Pattern:** Keep `combat_aoe.rs` as the module façade and move cohesive logic
into submodules. A reusable skipped scratch preserves deterministic membership
and avoids per-detonation allocation after IDs have been seen once.

**Dependency:** G0a, G1 area evidence from Task 3, and Tasks 7-10. If the current map
occupancy representation cannot expose the verified per-cell linked-list order,
stop and design the occupancy-order substrate change; sorting by stable ID or
EntityStore order is not a substitute.

**Step 1: Define record and scratch types**

```rust
// src/sim/combat/combat_aoe/collector.rs
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AreaDamageTarget {
    pub target_id: u64,
    pub distance_leptons: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AreaDamageFrame {
    start: usize,
    end: usize,
    depth: usize,
}

#[derive(Debug, Default)]
pub(crate) struct AreaDamageScratch {
    next_epoch: u64,
    seen_epoch: BTreeMap<u64, u64>,
    target_arena: Vec<AreaDamageTarget>,
    frames: Vec<AreaDamageFrame>,
}

impl AreaDamageScratch {
    pub(crate) fn begin_frame(&mut self) -> (usize, u64) {
        self.next_epoch = self.next_epoch.wrapping_add(1);
        if self.next_epoch == 0 {
            self.seen_epoch.clear();
            self.next_epoch = 1;
        }
        (self.target_arena.len(), self.next_epoch)
    }

    pub(crate) fn push_once(&mut self, epoch: u64, target: AreaDamageTarget) {
        if self.seen_epoch.insert(target.target_id, epoch) != Some(epoch) {
            self.target_arena.push(target);
        }
    }

    pub(crate) fn seal_frame(&mut self, start: usize) -> AreaDamageFrame;
    pub(crate) fn target(&self, frame: AreaDamageFrame, ordinal: usize)
        -> Option<AreaDamageTarget>;
    pub(crate) fn finish_frame(&mut self, frame: AreaDamageFrame);
    pub(crate) fn prune_seen_to_active_ids(&mut self, active_ids: &[u64]);
}
```

The BTreeMap is used only for membership, never output iteration. At the safe
tick boundary, `prune_seen_to_active_ids` removes IDs no longer in the active
entity set, bounding memory by active IDs plus IDs observed during the current
tick; wrap-only cleanup is insufficient for long matches with heavy churn.

`seal_frame` pushes a LIFO frame whose range is stable by index. The dispatcher
copies one target out with `target`, releases the scratch borrow, and only then
invokes the receiver. A nested death-weapon AoE appends a child range after the
outer range and truncates only its own range in `finish_frame`; outer records
remain intact. `finish_frame` validates LIFO depth and truncates the arena to
`start`. No slice/reference into the arena survives a receiver, trigger, or
lifecycle call.

**Step 2: Implement exact layer and distance functions**

`layer.rs` names the source reference frame and unit for every offset. It
implements only the Task 3-verified predicates for ground, airborne, bridge,
building footprint/height, and any special distance adjustment. Every signed
shift, subtraction, square/root operation, rounding, and clamp follows the
native body. Include one worked coordinate fixture per target category in a
module comment.

**Step 3: Capture the fixed target-record list in native order**

Expose:

```rust
pub(crate) fn collect_area_damage_targets<'world>(
    impact: WorldLeptonCoord,
    cell_spread: NativeF32Bits,
    world: AreaDamageWorldView<'world>,
    scratch: &mut AreaDamageScratch,
) -> AreaDamageFrame;
```

`WorldLeptonCoord` is the native Cartesian world `CoordStruct` frame with
signed X/Y/Z measured in leptons. It is a distinct type from map cells and
isometric screen coordinates. `layer.rs` owns the Task 3-verified conversions
into this type and their worked fixtures.

Call `scratch.begin_frame()`, append airborne records in native order, then CellSpread
table cells in native table order, then objects in each cell's native linked-list
order. Apply filters and dedup exactly where Task 3 places them. CellSpread zero
still visits the verified impact-cell path. Do not calculate falloff, Verses,
ProneDamage, final damage, or health.

The caller iterates by ordinal, copies `AreaDamageTarget`, and resolves it
synchronously before requesting the next record. It always calls
`finish_frame`, including target-gone and early-return paths. Add the concrete
outer `[A, B]` fixture where A's death creates a nested AoE; B must still be
visited exactly once after the nested call returns.

**Step 4: Keep existing callers compiling and add a shadow-only wrapper**

Leave the existing `apply_aoe_damage` name/signature/body in place because
current combat, lightning, and excluded Genetic Converter code still call it.
Add `apply_legacy_aoe_damage_for_shadow` as a thin wrapper that calls the
existing helper and is used only for frozen legacy results in Tasks 18-19. No
new caller may treat its `u16` output as receiver input. Task 23 deletes the
shadow wrapper and all in-scope calls, while the excluded Genetic Converter may
retain `apply_aoe_damage` until its own verified adapter exists.

**Step 5: Add native-order/distance tests**

Cover CellSpread zero, airborne plus ground, two targets in one cell, several
table offsets, dedup, bridge/layer cases, exact lepton distances, aircraft
adjustment, Building/height adjustment, and earlier-record removal visibility.
Assert the entire ordered `AreaDamageTarget` slice, not a sorted set.

**Step 6: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k combat_aoe -- --nocapture
rg -n "verses|ProneDamage|prone_damage|final_damage|saturating_sub" src/sim/combat/combat_aoe/collector.rs src/sim/combat/combat_aoe/layer.rs
```

Expected literal test result: `test result: ok.` The grep returns no calculation
or health-application match in the new collector/layer files.

### Task 17: Add Hash-Neutral Shadow Infrastructure

**Why:** Every producer needs the same raw call and a separate frozen legacy
result, while comparison must remain invisible to gameplay, RNG, snapshots, and
world hashes.

**Files:**
- Create: `src/sim/combat/damage/shadow.rs`
- Modify: `src/sim/combat/damage/types.rs`
- Modify: `src/sim/combat/damage/mod.rs`
- Modify: `src/sim/world/mod.rs`

**Pattern:** Follow `Simulation`'s skipped event-buffer pattern. Scratch is
allocated once, cleared per tick, and never serialized or hashed.

**Dependency:** G0a and Tasks 10-16.

**Step 1: Define frozen legacy and comparison records**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyDamagePhase {
    CombatPhase4,
    DeathWeaponPhase6,
    LightningPhase45,
    VerifiedImpactPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrozenLegacyResult {
    pub computed_loss: u16,
    pub phase: LegacyDamagePhase,
    pub application: LegacyApplication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyApplication {
    Pending,
    Applied {
        health_before: i32,
        health_after: i32,
        actual_delta: i32,
        invulnerability_skipped: bool,
        classified_dead: bool,
    },
    TargetGone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacySideEffectObservation {
    HealthWrite { target_id: u64, before: i32, after: i32 },
    LastAttackerWrite { target_id: u64, source_object_id: Option<u64> },
    DeathQueued { target_id: u64 },
    InvulnerabilitySkip { target_id: u64 },
    TargetGone { target_id: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShadowStatus {
    PureComplete,
    ExecutorBlocked,
    TargetGone,
    TimingBlocked,
    EvidenceBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompactShadowObservation {
    pub ordinal: u32,
    pub raw_call: DamageEvent,
    pub legacy: FrozenLegacyResult,
    pub predicted: Option<DamageOutcome>,
    pub checkpoint: Option<DamageMachine>,
    pub predicted_effects: std::ops::Range<usize>,
    pub legacy_effects: std::ops::Range<usize>,
    pub status: ShadowStatus,
}
```

Predicted effects and observed legacy side effects live in two flat scratch
Vecs; each observation stores slice ranges. `computed_loss` is never overwritten
when Phase 4 later attaches the actual application receipt, so an invulnerability
skip, target-gone case, overkill clamp, or second hit can distinguish intended
magnitude from actual health delta/result. Tests resolve the ranges through
accessors; there is no allocation per observation.

**Step 2: Define the reusable scratch and rollout switch**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageRollout {
    LegacyShadow,
    NativeReceiver,
}

pub(crate) const DAMAGE_ROLLOUT: DamageRollout = DamageRollout::LegacyShadow;

pub(crate) fn production_damage_rollout() -> DamageRollout {
    DAMAGE_ROLLOUT
}

#[derive(Debug, Default)]
pub(crate) struct DamageScratch {
    pub area: AreaDamageScratch,
    observations: Vec<CompactShadowObservation>,
    shadow_effects: Vec<DamageEffect>,
    legacy_effects: Vec<LegacySideEffectObservation>,
    next_ordinal: u32,
}
```

Receiver programs are local one-step `DamageMachine` values, not a shared
scratch field; nested damage therefore cannot overwrite an outer program.
`begin_tick(active_ids)` clears diagnostic vectors without shrinking, prunes
the area's seen-ID map to active IDs, asserts the area frame stack is empty,
and resets ordinal zero. The
production switch is a compile-time constant, not serialized state and not a
network option. Task 23 deletes the switch and legacy branch rather than
shipping two gameplay authorities.

Every producer exposes one crate-private internal entry point that accepts a
`DamageRollout` argument. Its production wrapper always passes
`production_damage_rollout()`; integration tests pass
`DamageRollout::NativeReceiver` explicitly. The mode is never read from a
snapshot, command, map, INI, replay, or network packet. Task 23 deletes the
argument and production wrapper split when only the native receiver remains.

**Step 3: Attach only skipped scratch to Simulation**

```rust
/// Transient damage calculation/diagnostic buffers. Never serialized or hashed.
#[serde(skip)]
pub(crate) damage_scratch: crate::sim::combat::damage::DamageScratch,
```

Initialize it in every `Simulation` constructor. Do not add it to
`world_hash.rs`, snapshot payloads, replay data, or app-visible event drains.

**Step 4: Implement read-only observation**

`record_shadow` snapshots immutable views and advances a local `DamageMachine`
one step at a time. It records a `PureComplete` outcome only when the machine
reaches `Complete` without requiring a world mutation, trigger, lifecycle call,
or RNG draw. At the first yielded effect/checkpoint that must be executed to
make a later read valid, it stores the machine/effect and returns
`ExecutorBlocked`; it does not fabricate the later outcome. It must not call the
effect executor, trigger runtime, lifecycle helpers, RNG, sound/world-effect
queues, or logging that can affect timing. Differences are data, never panics or
debug assertions.

If timing or G1 evidence is missing, record `TimingBlocked`/`EvidenceBlocked`
without fabricating a predicted result.

Tasks 17-19 never call an unchanged-context per-event result a full comparison.
Persistent cross-target consequences—such as target A's trigger mutating target
B before B's receiver—are compared only in the Task 22/23 executor fixtures,
which own a reconstructed world, cloned RNG/GT state, and synchronous effects.

**Step 5: Add neutrality tests**

Construct two deterministic worlds from the same serialized fixture/seed (the
current `Simulation` is not `Clone`). Run the shadow-only call on one and leave
the other untouched, then assert identical serialized authoritative state,
world hash, all RNG logical states, active-object order, event queues, and
health. Separately assert the expected observation exists.

**Step 6: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_shadow -- --nocapture
```

Expected literal result: `test result: ok.` Snapshot version remains unchanged.

### Task 18: Add Normal Direct-Fire and Weapon-AoE Shadow Adapters

**Why:** These producers currently precompute different final `u16` values and
run at the wrong phase; the adapter must expose raw facts without changing that
legacy authority.

**Files:**
- Modify: `src/sim/combat/mod.rs:1193-1195`
- Modify: `src/sim/combat/mod.rs:1602-1627`
- Modify: `src/sim/combat/mod.rs:1849-1884`
- Modify: `src/sim/combat/mod.rs:2375-2449`
- Create/modify: `src/sim/combat/damage_integration_tests.rs`

**Pattern:** Producer-specific raw/frozen adapter. Current `CombatEmit` tuples
and Phase-4 application remain live in this task.

**Dependency:** G0a and Tasks 3, 11, 16, and 17. G2 may remain open; observations then
record the timing blocker explicitly.

**Step 1: Assign one transient ordinal per legacy damage tuple**

Extend the shadow-only compatibility record, not the authoritative event, with
an ordinal. Preserve existing vector insertion and Phase-4 iteration order.
Phase 4 attaches applied/target-gone information to the same observation without
changing the live subtraction: preserve `computed_loss`, fill
`LegacyApplication::{Applied,TargetGone}`, and append observed health,
invulnerability, last-attacker, and death-queue side effects to the flat legacy
range.

**Step 2: Construct raw attacker output once**

At the fire calculation cut point, build `FireDamageView` and call the exact
attacker stage. This yields signed post-attacker `incoming_damage`. Do not apply
Verses, falloff, ProneDamage, defender armor, or a health clamp to the raw call.
The existing formula separately computes the frozen `u16` live value.

**Step 3: Record direct-fire shadow facts**

For the legacy direct path, construct a `DamageEvent` with exact source facts,
distance, warhead, and raw incoming value. While G2 is open, set
`ShadowStatus::TimingBlocked`: the target snapshot at `Fire_At` is not an impact
snapshot. Once G2 lands, this adapter moves to the impact call and becomes a
timing-aligned pure/checkpoint comparison. It remains `ExecutorBlocked` at any
effect whose trigger/RNG/lifecycle mutation must be applied; only Tasks 22-23
perform the full stateful comparison.

**Step 4: Record weapon-AoE shadow facts**

Use `collect_area_damage_targets` for raw ordered target/distance records and
`apply_legacy_aoe_damage_for_shadow` for frozen old values. Match by target ID
without sorting either sequence; explicitly record missing/extra/order
differences. Every raw target receives the same incoming damage and its own
distance. Advance a local `DamageMachine` only to `PureComplete` or the first
executor checkpoint. Never feed the old `u16` value into the new receiver.
Iterate the returned `AreaDamageFrame` by copied record and always call
`finish_frame`; never retain an arena slice across observation work.

**Step 5: Preserve live gameplay exactly**

The current `damage_events` tuple, Phase-4 coarse invulnerability, subtraction,
fear, last attacker, and Phase-6 dead list stay untouched except for read-only
observation correlation. Existing ProneDamage consumption remains only in the
legacy adapter until Task 23.

**Step 6: Add integration tests**

Cover direct and AoE raw/frozen separation, multi-hit ordering, missing target,
known ProneDamage/Verses/falloff differences, hash/RNG neutrality, and G2 timing
status. Assert live health matches the pre-task legacy fixture.

**Step 7: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_shadow_direct -- --nocapture
cargo test -p vera20k damage_shadow_aoe -- --nocapture
```

Expected literal result for each command: `test result: ok.` Existing combat
health outputs are byte-identical to the pre-task fixtures.

### Task 19: Add Radiation, Death-Weapon, and Lightning Shadow Adapters

**Why:** These paths currently calculate or apply health independently and need
source-specific raw cuts before a single authority can replace them.

**Files:**
- Modify: `src/sim/radiation.rs:79-86`
- Modify: `src/sim/combat/mod.rs:785-800`
- Modify: `src/sim/combat/mod.rs:1036-1097`
- Modify: `src/sim/combat/mod.rs:1773-1847`
- Modify: `src/sim/superweapon/lightning_storm.rs:242-269`
- Modify: `src/sim/superweapon/mod.rs:239-240`
- Modify: `src/sim/world/mod.rs:2287-2355`
- Modify: `src/sim/combat/damage_integration_tests.rs`

**Pattern:** Three explicit adapters because their current legacy formulas and
application phases differ. No shared guessed source identity or phase.

**Dependency:** G0a, Tasks 3 and 16-18. Each source must have a Task 3 timing and
argument-provenance row; otherwise its observation is `EvidenceBlocked`.

**Step 1: Carry verified raw radiation source facts**

Extend `RadDetonation` only with fields proved by Task 3: source object, source
house, warhead, raw signed level-derived damage input, impact coordinate, and
`ignore_defenses`. Preserve the existing site fold/cadence and frozen legacy
calculation. Remove no current f64/Verses/u16 operation until Task 23.

**Step 2: Observe death-weapon calls before direct HP subtraction**

At the verified death detonation phase, collect raw target records and record
receiver programs while `apply_legacy_aoe_damage_for_shadow` plus the direct
health subtraction remain live. Capture death recursion/order as diagnostics;
do not append newly killed targets to the old dead list in shadow mode because
that would change gameplay.

**Step 3: Observe lightning at its verified strike phase**

Construct raw calls from the Task 3 lightning provenance. Keep existing bolt
spawn, direct `saturating_sub`, and Building-state refresh live. The shadow path
does not push sound/world effects or touch RNG.

**Step 4: Add source-specific tests**

- Radiation: cadence, level formula cut, immunity inputs, source identity,
  Verses drift, and no live change.
- Death weapon: target order, recursive lethal observation, no duplicate live
  death processing, and no health change from shadow.
- Lightning: center/scatter record order, raw/frozen separation, current live
  health preserved, and RNG/sound/world-effect neutrality.

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_shadow_radiation -- --nocapture
cargo test -p vera20k damage_shadow_death_weapon -- --nocapture
cargo test -p vera20k damage_shadow_lightning -- --nocapture
```

Expected literal result for each command: `test result: ok.` Snapshot version
and all legacy live outcomes remain unchanged.

### Task 20: Run the Retail Numeric and Pure-Intent Oracle Pass

**Why:** Verify numeric stages, target records, results, and pure effect intents
before building the mutation executor; the full G3 gate remains pending until
Task 23 can exercise resolver, RNG, timing, and lifecycle cases.

**Files:**
- Create: `src/sim/combat/damage/oracle_tests.rs`
- Modify: `src/sim/combat/damage/mod.rs`
- Create: `docs/research/DAMAGE_ORACLE_RUST_COMPARISON_2026-07-13.md`
- Read only: `parity/fixtures/damage/manifest.json` and referenced fixture files
- Read only: the versioned `docs/research/schemas/damage-oracle-v{schema_version}.schema.json`

**Pattern:** Unit-test fixture consumer beside crate-private damage APIs. It
reads the stable Task 4 handoff and executes only pure stages; it never launches
or instruments gamemd.

**Dependency:** G0a/G1, stable Task 4 handoff, and Tasks 5-19. G2 may remain open
because executor/timing cases are deferred to Task 23 preflight.

**Step 1: Define strict manifest records**

Use `serde` structs with `#[serde(deny_unknown_fields)]` for the Task 4 schema.
Represent raw bit strings and exact source bytes with validated newtypes; the
`bytes` variant decodes an even-length hex string without UTF-8 conversion.
Represent integer width/signedness with tagged enums and termination receipts
with an exact code/instruction/fault-address/access tuple. Read `schema_version`
first, select the exact schema/DTO version accepted by 4C, and reject any other
version; do not hard-code v1. Reject duplicate case IDs, unknown hook roles,
wrong binary hash, or missing ordered observations. If 4C bumped the schema,
G0a must land the matching typed DTO before Task 20 is executable.

**Step 2: Dispatch every hook to the matching Rust stage**

- Any case with `result.termination != null` runs in a dedicated child process,
  never inside the parent Cargo-test process. The parent selects exactly one case,
  captures the child's Windows exception/exit code and any required instruction/
  fault-address/access receipt, then performs the comparison and writes the
  report. If the regenerated runner cannot capture every required termination
  field, G0a leaves the case and G3 blocked; it must not weaken the schema or let
  the child terminate the whole test/report run;
- `CCINIClass::ReadInt` cases call `ccini_read_int_prefix` and compare consumed
  bytes, signed i32 result, missing/current-value fallback, and every captured
  no-conversion/overflow receipt. The `MaxDamage` fixture must use this role;
- `CCINIClass::ReadDouble` cases call `scan_f32_prefix`, then the Task 7 generic
  reader with a context created from the captured entry control word; compare
  consumed-prefix length, source f32 bits, the mandatory initial widened-f64
  spill/reload, the post-percent f64 spill/reload when present, final caller
  f32/f64 store bits, and entry/exit control word;
- Warhead Verses-loop cases call the Task 8 reader with the captured parser
  context. Compare atoi-prefix percent and strtod-prefix bare tokens separately,
  require distinct missing-fallback/present-empty/exact-11/extra/empty-token-
  collapse/short-list cases plus the 0x80-byte `ReadString` truncation
  boundaries. A safe Rust short-list error against the captured native fault is
  `FAIL`/`DRIFT`, never `PURE_PASS`, and blocks G3 until exact externally visible
  termination equivalence is proved and implemented;
- `Math__ftol` cases call `X87Context::math_ftol` and compare full signed qword,
  EDX, low EAX, and control word before/after. Every captured x87 arithmetic,
  compare, and store observation inside the other roles dispatches to the named
  `X87Context` operation and compares its Ext80/IEEE receipt through the raw
  Task 6 bit accessors—never an f64 narrowing—before the owning stage is allowed
  to pass;
- kernel cases call `apply_warhead_damage`;
- Fire_At cases call `fire_damage` and compare every stage boundary recorded by
  the fixture;
- area cases call the collector, copy records by `AreaDamageFrame` ordinal,
  compare them, and finish the frame before receiver execution;
- Techno/Object cases advance their exact stages and compare pDamage/result and
  yielded effects only up to the first executor-dependent checkpoint;
- concrete cases drive the pure receiver machine without applying live effects;
  only a machine that reaches `Complete` without a required effect has a final
  predicted state, while checkpointed cases are `PENDING_EXECUTOR`;
- executor, RNG-counter, scheduler-cursor, and lifecycle-membership cases are
  loaded and reported as `PENDING_EXECUTOR`, not counted as passes.

**Step 3: Classify zero tolerance**

Every difference in a bit, integer, result code, target position, effect/write,
RNG counter, frame cursor, or membership is a mismatch. A known legacy
difference can be described in the report, but cannot be counted as a pass for
the new path. Hand-computed expected values are not substituted for a missing
retail record.

**Step 4: Write the comparison report**

Record binary/manifest hashes, case counts by hook, exact pass/fail IDs, and the
first differing field for every failure. The report separates `PURE_PASS`,
`FAIL`, and `PENDING_EXECUTOR`; it must not label G3 passed in this task.

**Step 5: Verify serially**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_oracle -- --nocapture
```

Expected literal result: `test result: ok.` The report names the retail SHA-256,
reports zero pure-stage mismatches, lists every deferred executor/timing case,
and states `G3: PENDING_EXECUTOR`. A pure-stage failure blocks Task 21.

### Task 21: Implement the Synchronous Resolver and Effect Executor

**Why:** A verified pure program becomes authoritative only through an executor
that commits each effect at the native position and can refresh views after
re-entrant callbacks.

**Files:**
- Create: `src/sim/combat/damage/resolver.rs`
- Modify: `src/sim/combat/damage/envelope.rs`
- Modify: `src/sim/combat/damage/mod.rs`
- Create/modify: `src/sim/combat/damage_integration_tests.rs`

**Pattern:** Define a narrow sim-only mutation trait in the damage module and
implement it for a test world first. Production `Simulation` wiring waits for
Task 22, keeping `DAMAGE_ROLLOUT=LegacyShadow`.

**Dependency:** G0a/G1 and Task 20's pure-stage pass. G2 and full G3 are not
required for resolver unit tests.

**Step 1: Define the mutation boundary**

```rust
pub(crate) trait DamageMutationSink {
    fn build_context(&self, event: DamageEvent) -> Option<DamageContextView>;
    fn apply_damage_effect(
        &mut self,
        effect: DamageEffect,
        tick: u32,
        x87: &mut X87Context,
    );
    fn refresh_context(&self, event: DamageEvent) -> Option<DamageContextView>;
}

pub(crate) struct DamageResolverContext<'a> {
    pub sim: &'a mut crate::sim::world::Simulation,
    pub rules: &'a crate::rules::ruleset::RuleSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolveDamageResult {
    Resolved(DamageOutcome),
    TargetGone,
}

pub(crate) fn resolve_damage<S: DamageMutationSink>(
    sink: &mut S,
    event: DamageEvent,
    tick: u32,
    x87: &mut X87Context,
) -> ResolveDamageResult;
```

Contexts are owned Copy snapshots assembled from typed rules and runtime state.
Do not store raw references/pointers across a mutation.

`Simulation` itself does not implement this trait and does not pretend to own a
`RuleSet`. Unit tests implement it for a fake sink. Task 22 implements it for
`DamageResolverContext<'_>`, which borrows the validated non-null `RuleSet`
passed into the current tick and delegates GT calls to the landed trigger owner
inside `Simulation`.

**Step 2: Execute at verified checkpoints**

Build the initial context and a local `DamageMachine`. Call `advance` once,
copy the yielded `DamageMachineStep`, release the context/machine borrow, then
apply that single effect. Refresh after exactly the callbacks Task 1 marked
re-entrant and continue. A missing target returns `TargetGone` with no write. A bunker redirect
starts the verified redirected receiver call with the same post-attacker damage
and no second attacker stage.

A re-entrant trigger or death effect calls `resolve_damage` recursively with
the same mutable `X87Context`; the outer `DamageMachine` remains a local stack
value and no arena borrow is live. `Math__ftol`'s persistent control-word change
is therefore visible to the nested call exactly in sequence.

Effect execution uses existing lifecycle helpers for reveal/conceal/limbo/
uninit/delete and existing sim event queues for sounds/world effects. The damage
module names intents but never imports app/audio/render modules.

**Step 3: Preserve RNG owner and order**

`RequestConcreteEffect` executes the exact Scenario or main RNG draw sequence
from Task 2. It emits sim sound/world-effect records only after the same native
predicate and at the same stage. Shadow calculation never invokes this method.

**Step 4: Test with a deterministic fake sink**

The fake sink records every applied effect, supports callback-induced state
mutation, and models target disappearance. Cover normal, healing, redirect,
re-entrant trigger, PostMortem, Dead/lifecycle, and target-gone paths. Assert no
effect is duplicated and program order equals the retail fixture.

**Step 5: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_resolver -- --nocapture
```

Expected literal result: `test result: ok.` Production still uses legacy
authority.

### Task 22: Wire Verified Producer Call Points Behind the Non-Live Rollout

**Why:** Every in-scope route must pass end-to-end tests at its native phase
before one coordinated switch removes legacy authority.

**Files:**
- Modify: G2-owned projectile/impact files named by the approved G2 plan
- Modify: `src/sim/combat/mod.rs`
- Modify: `src/sim/radiation.rs`
- Modify: `src/sim/superweapon/lightning_storm.rs`
- Modify: `src/sim/superweapon/mod.rs`
- Modify: `src/sim/world/mod.rs`
- Modify: `src/sim/game_entity.rs`
- Modify: `src/sim/house_state.rs`
- Modify: `src/sim/combat/damage_integration_tests.rs`

**Pattern:** Both branches compile, but the production constant remains
`LegacyShadow`. Tests call `NativeReceiver` explicitly. Runtime fields are
transient/defaulted in this stage and are not hashed or serialized as authority.

**Dependency:** G0a, G0b, G1, and the landed G2, GT, GS, GV, and GP
implementations, plus Task 20's pure-stage pass and Task 21. Full G3 remains pending until Task 23 preflight. Coordinate
file ownership before editing `world/mod.rs` or projectile files.

Before Task 22 is executable, run `/review-plan` and replace “G2-owned
projectile/impact files” in this task with the literal paths and symbols from
the approved G2 plan. If that revision has not happened, Task 22 is blocked; an
executor must not select a projectile owner by guesswork.

**Step 1: Add exact runtime initialization behind skipped fields**

Attach `Option<DamageRuntimeState>` to `GameEntity` and
`Option<HouseDamageState>` to `HouseState` using `#[serde(skip)]` while the
rollout remains non-live. `Option` supplies the deterministic skipped-field
default `None` on deserialize; every fresh or rebuilt test world sets `Some`
from the exact Task 1 rule/mode/difficulty assembly before the explicit native
branch can run. Initialize house multipliers, last-damage/WasAttacked, and
delay-kill state there. Read and mutate Health, FearLevel, native veterancy,
per-unit firepower/armor, Techno ammo/readiness, controller/victim, and
last-attacker through the single GS-owned fields; do not
duplicate them in `DamageRuntimeState`. Do not add these skipped fields to
`world_hash.rs` yet.

**Step 2: Implement `DamageMutationSink` for a tick-scoped resolver adapter**

Implement the trait for `DamageResolverContext<'_>`, never directly for
`Simulation`. Build views through its borrowed validated `&RuleSet` and existing alliance, type,
invulnerability, teleport, bunker, particle, trigger, lifecycle, sound, and RNG
owners. Each effect delegates to the established owner rather than duplicating
its storage mutation inside combat.

`CreateDamageParticle`/`RemoveDamageParticle` use only the landed GP API. The
native branch is disabled if a spawned particle would enter an authoritative,
world-hashed store whose full update state is still skipped by snapshots.

No program scratch is borrowed from `Simulation`: every resolver invocation
owns its local `DamageMachine`. For area calls, use short borrows only: collect
an `AreaDamageFrame`, copy one target record from `damage_scratch.area`, release
that borrow, construct `DamageResolverContext { sim: self, rules }`, resolve,
then fetch the next record. Nested AoE appends/finishes a child frame without
touching the outer range. The producer creates the Task 4-captured
`X87Context`; nested calls receive the same mutable context. No unsafe pointer,
raw reference, global program scratch, returned arena slice, or cloned
`Simulation` is permitted.

**Step 3: Wire normal weapons at G2 impact**

Convert `ProjectileImpactDamageCall` to one direct `DamageEvent` or one ordered
area record list. For area calls, finish fixed record capture first, then resolve
each record synchronously before advancing to the next record. Never invoke the
receiver from `Fire_At` or the current Phase-2 attacker loop.

**Step 4: Wire death weapon, radiation, and lightning test branches**

Call the resolver at each Task 3-verified phase and with its verified source
facts. Death recursion immediately follows native lifecycle/order rather than
the old prebuilt dead list. Keep the production legacy branch selected.

**Step 5: Add end-to-end native-branch tests**

Cover direct impact, weapon AoE, same-frame appended bullet, delayed bullet,
earlier area target mutating/removing a later record, death-weapon recursion,
radiation cadence, lightning center/scatter, negative-warhead healing, every
result state, RNG effects, triggers, and lifecycle membership.

Assert the explicit native branch matches Task 20 fixtures. Separately assert
the production branch still yields the old health/hash receipt.

**Step 6: Verify**

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_native_integration -- --nocapture
cargo test -p vera20k damage_shadow -- --nocapture
```

Expected literal result for each command: `test result: ok.` G4 must be checked
before the next task.

### Task 23: Perform the Coordinated Authority Flip and Retire Legacy Formulas

**Why:** The project must finish with one entity receiver authority, not a
permanent mode switch or two formula families.

**Files:**
- Modify: `src/sim/combat/mod.rs`
- Modify: `src/sim/combat/combat_aoe.rs`
- Modify: `src/sim/radiation.rs`
- Modify: `src/sim/superweapon/lightning_storm.rs`
- Modify: `src/sim/world/mod.rs`
- Modify: G2 projectile/impact files
- Modify: `src/sim/game_entity.rs`
- Modify: `src/sim/house_state.rs`
- Modify: `src/sim/world/world_hash.rs`
- Modify: `src/sim/snapshot.rs`
- Modify: `src/sim/combat/combat_tests.rs`
- Modify: `src/sim/combat/damage_integration_tests.rs`
- Modify: `src/sim/combat/damage/oracle_tests.rs`
- Modify: `docs/research/DAMAGE_ORACLE_RUST_COMPARISON_2026-07-13.md`

**Pattern:** Step 0 is the final non-authority-changing G3 acquisition. The coordinated
cutover begins only after it passes and all other gates already hold. Keep
excluded mechanisms on their verified owners; do not bulk-convert arbitrary
health writes.

**Dependency:** G0a, G0b, G1, and the landed G2, GT, GS, GV, and GP
implementations, plus Tasks 1-22. Step 0 may edit only the Oracle test consumer
and comparison report named below. Before any authority-file edit, obtain G3 and
confirm no active task owns combat, world tick, hash, snapshot, RNG golden, or
G2 files.

**Step 0: Complete the full retail Oracle gate before changing authority**

Extend `oracle_tests.rs` to run the Task 21 deterministic mutation sink and the
Task 22 producer/scheduler paths for every case previously marked
`PENDING_EXECUTOR`. Compare applied writes, refresh points, actual RNG stream
and counter, frame/Logic cursor, sound/world-effect intent, active/limbo/
pending-delete membership, and final state. Run:

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
cargo test -p vera20k damage_oracle -- --nocapture
```

Expected literal result: `test result: ok.` Update
`DAMAGE_ORACLE_RUST_COMPARISON_2026-07-13.md` to state `G3: PASS`, name the
retail and manifest hashes, report zero required mismatches, and list zero
pending required cases. If any required case is missing or differs, stop before
Step 1 and leave legacy authority intact.

**Step 1: Re-read integration state**

```powershell
git status --short
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
rg -n "SNAPSHOT_VERSION" src/sim/snapshot.rs
```

Record the current snapshot value and choose exactly the next free value. Do
not assume it is 26.

**Step 2: Make runtime receiver state authoritative**

Replace the transitional skipped `Option` fields with non-optional,
serialized `GameEntity.damage_runtime: DamageRuntimeState` and
`HouseState.damage_state: HouseDamageState`. All construction/load paths must
initialize them before this change compiles; the new snapshot version rejects
the earlier schema. Add every field, enum discriminant, optional member inside
the state, and vector/map entry to `world_hash.rs` in struct/iteration order. Add snapshot
round-trip tests proving the fields survive and hash identically.

Include GP's particle-system store and every receiver-created particle field in
the same round-trip/hash audit; hashing a skipped/non-restored particle object is
not acceptable authority.

**Step 3: Remove the rollout switch and legacy live paths**

Delete `DAMAGE_ROLLOUT`, `LegacyShadow` authority selection, current
`CombatEmit.damage_events: Vec<(u64,u16,u64,InternedId)>`, Phase-4 generic HP
subtraction, legacy direct/AoE/radiation formulas, death-weapon direct HP write,
lightning direct HP write, and `apply_legacy_aoe_damage_for_shadow`. Do not
delete `apply_aoe_damage` while the excluded Genetic Converter still owns a
call; classify that remaining helper/call explicitly as out of scope.

Delete active ProneDamage consumption. Retain the parsed legacy rule field only
if another verified non-receiver system reads it; otherwise remove the dead
compatibility field after an `rg` proves zero callers.

**Step 4: Invoke one resolver at every in-scope native call point**

- normal direct and weapon AoE: G2 munition/effect impact;
- death weapon: Task 3-verified death detonation position;
- radiation: Task 3-verified periodic receiver position; and
- lightning: Task 3-verified strike position.

For area calls, capture records once and synchronously resolve each in order.
Death/lifecycle effects occur inside the receiver sequence, not in a later
generic dead-list pass.

**Step 5: Keep scope exclusions explicit**

Add route-inventory assertions/tests proving C4, Genetic Converter, bridge,
wall, overlay, tiberium, terrain-cell damage, repair, selling, crash, script
removal, temporal removal, and other lifecycle zeroing do not enter ordinary
entity `ReceiveDamage` without their own verified adapter.

**Step 6: Replace legacy regression expectations**

Retire/invert current ProneDamage tests, replace deferred Phase-4 ordering tests
with impact/receiver order, unignore the immediate-death/lifecycle coverage once
native behavior is implemented, and update radiation/death/lightning tests to
assert full receiver effects.

**Step 7: Prove the route inventory**

```powershell
rg -n "damage_events|apply_legacy_aoe_damage|apply_aoe_damage|verses_pct|apply_prone_damage_modifier|prone_damage_basis_points" src/sim
rg -n "\.health\.current\s*=|saturating_sub" src/sim
```

The first command returns no live in-scope damage formula. Classify every match
from the second command in a checked list: authoritative receiver effect,
verified excluded mechanism, repair/healing owner, or lifecycle owner. Any
unclassified in-scope write fails the cutover.

**Step 8: Bump snapshot and native-derived baselines**

Set the next free `SNAPSHOT_VERSION`, update snapshot fixtures, deterministic
hash receipts, and replay/native-derived goldens under the single integration
owner. Rust-vs-prior-Rust goldens remain regression ratchets and are not labeled
gamemd parity evidence.

### Task 24: Run Final Serial Verification and Parity Audit

**Why:** The cutover is complete only when focused tests, retail fixtures,
deterministic state, route inventory, formatting, and the final build all pass
together.

**Files:**
- Inspect every file changed by Tasks 5-23
- Read/update: `docs/plans/2026-07-13-damage-authoritative-cutover-owned-files.txt`
- Modify only a failing task's owned file; do not repair unrelated dirty work

**Pattern:** Focused checks serially, one final `cargo check -q`, literal test
result reporting, and no crate-wide formatting.

**Step 1: Format only edited Rust files**

At G0a/G0b regeneration, initialize the owned-file manifest with one repo-
relative path per generated child task. Before a child edits or creates a file,
it adds that literal path under its task ID. Never infer ownership from
`git diff --name-only`, which omits untracked files and mixes pre-existing work.

```powershell
$manifest = 'docs/plans/2026-07-13-damage-authoritative-cutover-owned-files.txt'
$owned = @(Get-Content $manifest | ForEach-Object { $_.Trim() } |
  Where-Object { $_ -and -not $_.StartsWith('#') } | Sort-Object -Unique)
$ownedRust = @($owned | Where-Object { $_.EndsWith('.rs') -and (Test-Path -LiteralPath $_) })
foreach ($file in $ownedRust) {
  rustfmt --edition 2024 -- $file
  if ($LASTEXITCODE -ne 0) { throw "rustfmt failed: $file" }
}
git status --short
git ls-files --others --exclude-standard
```

Inspect every owned diff and classify every changed/untracked path not in the
manifest as pre-existing/other-session work; do not format or edit it. The
manifest itself is the final audit record even though `docs/` is intentionally
ignored by Git.

**Step 2: Run focused tests serially**

Use the mandatory helper (which checks ownership and rejects zero matched tests)
for each filter separately:

```powershell
Invoke-NonzeroCargoTest 'native_float'
Invoke-NonzeroCargoTest 'damage_rules'
Invoke-NonzeroCargoTest 'warhead_type'
Invoke-NonzeroCargoTest 'object_type'
Invoke-NonzeroCargoTest 'sim::combat::damage'
Invoke-NonzeroCargoTest 'combat_aoe'
Invoke-NonzeroCargoTest 'damage_native_integration'
Invoke-NonzeroCargoTest 'damage_oracle'
Invoke-NonzeroCargoTest 'lightning_storm'
Invoke-NonzeroCargoTest 'radiation'
Invoke-NonzeroCargoTest 'snapshot'
Invoke-NonzeroCargoTest 'determinism'
```

Record every literal `running N tests` and `test result:` line. Each filter must
match at least one test and every result must say `test result: ok.`

**Step 3: Re-run route, float, and boundary scans**

```powershell
rg -n "\bf32\b|\bf64\b| as i32|ProneDamage|prone_damage" src/sim/combat/damage src/sim/combat/combat_aoe
rg -n "damage_events|apply_legacy_aoe_damage|verses_pct|apply_prone_damage_modifier" src/sim
rg -n "crate::(render|ui|sidebar|audio|net)|crate::sim::(render|ui|sidebar|audio|net)" src/sim/combat/damage src/sim/combat/combat_aoe
```

Expected: no host-float simulation arithmetic, retired formula, or forbidden
layer dependency. Comments/type names and verified excluded paths are listed
explicitly if they match textually.

**Step 4: Re-run G3 and deterministic receipts**

Confirm the Oracle report's manifest/binary hashes still match, zero mismatches
remain, the final world hash changes when each new authoritative field changes,
snapshot round trips retain all receiver state and receiver-created particle
systems, and two identical replays yield
identical health, effects, RNG, active order, snapshot bytes, and world hash.

**Step 5: Run the final build check**

```powershell
Assert-CargoIdle
cargo check -q
```

Expected: exit code 0 with no damage-task error. Report unrelated pre-existing
warnings or failures separately and do not modify their files.

**Step 6: Record the final parity status honestly**

The completion note may say the named damage routes passed the listed retail
fixtures. It must not claim universal damage parity from a sampled fixture set.
Uncaptured input space remains `UNVERIFIED`; any remaining in-scope mismatch is
`DRIFT`, regardless of visibility or frequency.

## Sources & References

### Design and correction bundle

- `docs/plans/2026-07-13-damage-authoritative-cutover-design.md`
- [docs/research/DAMAGE_KERNEL_CONSTANTS_REVERIFICATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_KERNEL_CONSTANTS_REVERIFICATION_2026-07-13.md)
- [docs/research/DAMAGE_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_MATH_GHIDRA_REPORT.md) — use the current formula/order
  sections; its older healing-parameter prose must be reconciled against live
  assembly in Task 11 before authority.
- [docs/research/GATE_DAMAGE_VERSES_F64_RESOLUTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATE_DAMAGE_VERSES_F64_RESOLUTION_GHIDRA_REPORT.md) — corrected
  256-lepton section and exact Verses/x87 evidence.
- [docs/research/GATE_DAMAGE_MAXDAMAGE_CLAMP_RESOLUTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATE_DAMAGE_MAXDAMAGE_CLAMP_RESOLUTION_GHIDRA_REPORT.md) —
  constructor fallback mechanism only; not authority for stock runtime value.
- [docs/research/CCINICLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CCINICLASS_GHIDRA_REPORT.md) — generic `ReadDouble` `%f`
  prefix parse and percent path.
- [docs/research/core-services-map/ini-parsing.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/core-services-map/ini-parsing.md)
- [docs/research/INI_PARSING_HELPERS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INI_PARSING_HELPERS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) —
  distinguishes generic CCINI readers from Verses atoi/strtod token parsing.
- [docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md)
  — `Math__ftol` qword conversion and `0x0e7f` control-word evidence.
- [docs/research/SPARK_LIGHT_EFFECT_TICK_ROUNDING_AND_FIRST_VISIBLE_STAGE_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPARK_LIGHT_EFFECT_TICK_ROUNDING_AND_FIRST_VISIBLE_STAGE_RESWARM_20260528.md)
  — startup/live x87 control-word path.

### Receiver, rules, and concrete wrappers

- [docs/research/RECEIVE_DAMAGE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RECEIVE_DAMAGE_GHIDRA_REPORT.md)
- [docs/research/RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md) — useful stage
  map with readiness terminology and some field identities requiring Task 1
  correction.
- [docs/research/GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md) —
  numeric order; its post-veterancy containment labels are superseded.
- [docs/research/TANK_BUNKER_COMBAT_SURFACE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TANK_BUNKER_COMBAT_SURFACE_GHIDRA_REPORT.md) — authoritative
  containment identities/order and bunker routing surface.
- `docs/research/TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md` — readiness fields and
  type layout.
- [docs/research/WARHEADTYPECLASS_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEADTYPECLASS_REINVESTIGATION_GHIDRA_REPORT.md) —
  `AffectsAllies`, delay kill, warhead gate evidence.
- [docs/research/WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md) — numeric/flag defaults;
  its old “no stock DelayKill” statement is superseded by current stock INI.
- [docs/research/DAMAGE_HELPERS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_HELPERS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) — source-
  present ally test and readiness side-effect evidence.
- [docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md) — audited
  Building wrapper and `DestructionEffects` identity.
- [docs/research/AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) — raw Building vtable correction record.
- [docs/research/TARGETDEATH_BUILDINGCLASS_DESTRUCTION_REMOVAL_OWNER_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TARGETDEATH_BUILDINGCLASS_DESTRUCTION_REMOVAL_OWNER_RESWARM_20260528.md)
  — stale `+0x4EC=Limbo` claim retained only as a conflict to correct.
- [docs/research/TARGETDEATH_RECEIVEDAMAGE_DEATH_DISPATCH_REMOVAL_TIMING_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TARGETDEATH_RECEIVEDAMAGE_DEATH_DISPATCH_REMOVAL_TIMING_RESWARM_20260528.md)
  — death-dispatch navigation; concrete timing must be rechecked in Task 2.
- [docs/research/VETERANCY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VETERANCY_SYSTEM_GHIDRA_REPORT.md) — native f32 accumulator,
  rank/ability parsing, kill XP, promotion, and crate writer paths.
- [docs/research/INFANTRYCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRYCLASS_GHIDRA_REPORT.md) — signed integer FearLevel and
  reachable 300-value behavior.
- `docs/research/CRATE_SYSTEM_GHIDRA_REPORT.md` — active firepower/armor/
  veterancy crate writers that GS/GV must migrate with receiver state.

### Area dispatch and timing

- [docs/research/combat/systems/damage_formula.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/combat/systems/damage_formula.md)
- [docs/research/combat/systems/splash_cellspread.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/combat/systems/splash_cellspread.md)
- [docs/research/TARGETDEATH_APPLY_AREA_DAMAGE_LIVE_VECTOR_ITERATION_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TARGETDEATH_APPLY_AREA_DAMAGE_LIVE_VECTOR_ITERATION_RESWARM_20260528.md)
- [docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md)
- [docs/research/AAHEATSEEKER2_FIRST_TICK_DAMAGE_LATENCY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_FIRST_TICK_DAMAGE_LATENCY_GHIDRA_REPORT.md)

### Live binary anchors

- `0x00489180` — `ApplyWarheadDamage`
- `0x00489280` — `Apply_area_damage`
- `0x006fdd50` — `TechnoClass::Fire_At`
- `0x00701900` — `TechnoClass::ReceiveDamage`
- `0x005f5390` — `ObjectClass::ReceiveDamage`
- `0x00442230` — `BuildingClass::ReceiveDamage`
- `0x004415f0` — audited `BuildingClass::DestructionEffects`
- `0x005276d0` — `CCINIClass::ReadInt` used by `MaxDamage`
- `0x007c5f00` — `Math__ftol`
- `0x007e2224` — raw f32 bits `0x43800000` (`256.0f`)
- `0x007e3808` — raw f64 bits `0x3f847ae147ae147b` (`0.01`)

Addresses are evidence anchors for reports and fixtures. They are not copied
into Rust gameplay comments as semantic names without the verified role.

### INI authority

- `ini/rules.ini:16` and `ini/rulesmd.ini:16` — `[General] VeteranCombat=1.1`
- `ini/rules.ini:19` and `ini/rulesmd.ini:19` — `[General] VeteranArmor=1.5`
- `ini/rules.ini:610-611` and `ini/rulesmd.ini:752-753` — `[AudioVisual]`
  `ConditionRed=25%` and `ConditionYellow=50%`, both retained as native doubles
- `ini/rulesmd.ini:836` — `OccupyDamageMultiplier=1.2`
- `ini/rulesmd.ini:843` — `BunkerDamageMultiplier=1.3`
- `ini/rulesmd.ini:868` — `OpenToppedDamageMultiplier=1.2`
- `ini/rules.ini:716` and `ini/rulesmd.ini:896` — `MaxDamage=10000`
- `ini/rulesmd.ini:897` — `MinDamage=1`, parsed text but not an active damage
  rule field.
- `ini/rulesmd.ini:27208-27210` — stock OilExplosionWH delay-kill keys.
- `ini/rulesmd.ini:14935`, `14957`, and `22284` — stock
  `EligibleForDelayKill=yes` examples.
- `ini/rulesmd.ini:27174-27183` — stock PsychicDamage/AffectsAllies examples.
- `ini/rulesmd.ini:23529`, `27099`, `27110`, `27440`, and `27531` — stock
  `PenetratesBunker=yes` examples.

### Current Rust integration anchors

- `src/sim/combat/mod.rs:1193-1195` — legacy final-`u16` damage tuple.
- `src/sim/combat/mod.rs:1602-1627` — current read-only firing phase.
- `src/sim/combat/mod.rs:1773-1847` — current radiation calculation.
- `src/sim/combat/mod.rs:1849-1884` — current generic Phase-4 HP subtraction.
- `src/sim/combat/mod.rs:1896-1909` — current Phase-6 dead-list processing.
- `src/sim/combat/mod.rs:1036-1097` — current death-weapon direct damage.
- `src/sim/combat/mod.rs:2375-2449` — current direct/AoE legacy formulas.
- `src/sim/combat/combat_aoe.rs:81-353` — mixed collection/final calculation.
- `src/sim/superweapon/lightning_storm.rs:242-269` — current independent
  lightning HP write.
- `src/sim/world/mod.rs:2287-2355` — current superweapon/combat phase placement.
- `src/sim/world/mod.rs:481-484` and `src/sim/world/world_hash.rs:99,131-155`
  — authoritative particle systems are ticked/hashed but currently
  `#[serde(skip)]`, motivating GP.
- `src/sim/combat/damage/` — isolated f64 scaffolding and 37 regression tests.
- `src/sim/snapshot.rs:71` — snapshot version 25 at planning time; re-read at
  Task 23.

### Freshness and ownership

- No committed damage refactor landed on or after 2026-07-13; latest relevant
  committed work predates the design.
- Current branch at planning time: `dev`, ahead of `origin/dev`.
- Uncommitted overlap exists in `src/sim/combat/combat_tests.rs`,
  `src/sim/world/mod.rs`, and `src/sim/world/world_hash.rs`.
- `parity/` and `tools/oracle_*` are owned by the separate Oracle task and are
  read-only to this plan until an explicit handoff.
