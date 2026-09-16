# Entity-State Authority Substrate Design

**Date:** 2026-07-22  
**Status:** Approved design  
**Implementation status:** Not started  
**Chosen rollout:** Staged shadow-first migration with one coordinated authority flip

## Goal

Introduce an exact entity-state substrate capable of representing the future-affecting
state required by active Yuri's Revenge mechanics without prematurely changing live
gameplay authority.

The substrate must:

- represent verified native state without lossy `u16` or approximate floating-point
  substitutions;
- keep health, lifecycle, scheduler membership, and deletion as separate authorities;
- preserve distinctions between owner, controller, attacker, attacker owner, source
  House, and last attacker;
- support incremental reader and writer migration while legacy behavior remains live;
- detect and classify old/new divergence without affecting simulation behavior;
- permit one coordinated, versioned authority flip after the required evidence and
  integration work is complete; and
- avoid inventing semantics for unresolved native fields.

This design does not implement the damage receiver, lifecycle authority, reference
authority, veterancy system, or reload scheduler. It supplies exact state boundaries
and access contracts those systems can use.

## Architecture Context

Current simulation state is spread across a flat `GameEntity` and specialized
components. Important limitations include:

- health is stored as unsigned `u16` current/maximum values even though native damage,
  healing, readiness, and intermediate operations use signed values;
- veterancy is stored as an integer even though the native state and promotion
  comparisons use an exact floating representation;
- aircraft ammo/reload state is specialized and does not establish a verified general
  Techno readiness authority;
- last-attacker state is separate, but the broader attacker/owner/source-House identity
  contract is not yet represented consistently;
- live combat queues unsigned damage and later applies a saturating subtraction;
- snapshots serialize `GameEntity`, while the world hash omits some future-affecting
  state such as veterancy, ammo/readiness, and last-attacker state; and
- lifecycle state is independent from health, but a new state design could easily
  collapse those authorities if the boundary is not explicit.

The design preserves the project architecture:

- `EntityStore` remains the deterministic owner of `GameEntity` storage.
- The project does not adopt an ECS crate or native C++ inheritance layout.
- Simulation remains independent from render, UI, audio, sidebar, and net layers.
- Lifecycle and scheduler work remains owned by its existing authority effort.
- Existing native-bit and x87 utilities are reused for exact numeric representation.

### Research basis

The design is grounded in the current research corpus, particularly:

- [docs/research/OBJECT_TECHNO_LIFECYCLE_SHARED_STATE_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_TECHNO_LIFECYCLE_SHARED_STATE_SYSTEM_MODEL_SYNTHESIS.md)
- [docs/research/OBJECT_LOGIC_LIFECYCLE_ACTIVE_MEMBERSHIP_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_LOGIC_LIFECYCLE_ACTIVE_MEMBERSHIP_SYSTEM_MODEL_SYNTHESIS.md)
- [docs/research/LOGICCLASS_OBJECT_LIFECYCLE_SPINE_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_OBJECT_LIFECYCLE_SPINE_SYSTEM_MODEL_SYNTHESIS.md)
- [docs/research/DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/DAMAGE_RECEIVER_CORE_REINVESTIGATION_2026-07-13.md)
- [docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md)
- [docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_CONCRETE_RECEIVER_REINVESTIGATION_2026-07-13.md)
- [docs/research/VETERANCY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VETERANCY_SYSTEM_GHIDRA_REPORT.md)
- [docs/research/GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md)
- `docs/plans/2026-06-04-damage-substrate-service-design.md`
- `docs/plans/2026-07-13-damage-authoritative-cutover-plan.md`

This design document does not itself certify those claims as parity. Parity verdicts
still require named gamemd/retail-derived executable checks or exhaustive proof.

## Impact Analysis

### Expected implementation surfaces

- `src/sim/components.rs`: exact state representations or compatibility projections.
- `src/sim/game_entity.rs`: embedded state bundle and eventual legacy-field removal.
- A focused entity-state module: views, mutation facade, comparison records, and
  authority boundaries.
- `src/sim/combat/damage/`: exact inputs and ordered mutation outputs.
- `src/sim/combat/mod.rs`: replacement of unsigned queued damage and saturating health
  subtraction when damage authority is ready.
- Veterancy and kill-credit code: exact experience state and ordered promotion effects.
- `src/sim/docking/aircraft_dock.rs`: migration only where research proves shared
  readiness authority; aircraft docking phase remains specialized otherwise.
- `src/sim/house_state.rs`: distinct House/country/difficulty/category combat inputs.
- `src/sim/snapshot.rs`: coordinated snapshot-version change at cutover.
- `src/sim/world/world_hash.rs`: hashing of every future-affecting authoritative field.
- Rules parsing: exact Veteran/Elite ability-array behavior and verified modifier data.
- Tests and diagnostic comparison harnesses.

Initial additive work must avoid `src/sim/world/techno_ai.rs` while that file is owned
by another active effort.

### Behavioral consumers

The state substrate affects damage, repair, healing, readiness/ammo, reload scheduling,
veterancy, kill credit, attribution, triggers, lifecycle requests, persistence,
determinism, replay, and reference expiration. A partial authority flip would therefore
create incompatible truths across several core systems.

## Chosen Approach

Embed a layered entity-state bundle inside `GameEntity`, expose it through a focused
view/mutation facade, and migrate it shadow-first.

```text
GameEntity
|- identity / position / mission
|- lifecycle state                 (separate authority)
|- entity_state
|  |- vitality
|  |- experience
|  |- readiness
|  |- combat_modifiers
|  `- relations
`- legacy fields                   (temporary migration mirrors)
```

The bundle is owned with the entity, shares entity lifetime automatically, and avoids
the duplicate identity and cleanup problems of a sidecar store. Focused accessors
prevent the bundle from becoming another flat set of fields that any subsystem can
mutate without preserving verified ordering.

Legacy fields remain gameplay-authoritative during migration. The exact bundle is a
diagnostic shadow until one coordinated cutover.

## Tiny-Detail Ledger

| Detail | Design disposition |
|---|---|
| Signed current/max health | Exact `i32` vitality state |
| Health separate from lifecycle | Enforced authority boundary |
| Native veterancy value | Exact native-bit representation |
| `1.0`/`2.0` promotion thresholds | Derived exact queries |
| Full Veteran/Elite ability arrays | Type/rules state, exact verified width |
| Signed readiness/ammo | Exact known state |
| Negative damage raising readiness | Preserved by caller operation |
| Readiness write before early exit | Ordered transaction effect |
| Unresolved reload field | Research gate; excluded |
| Per-unit armor/firepower multipliers | Separate exact native-bit fields |
| House/country/difficulty modifiers | Separate assembly inputs |
| Modifier application order | Combat authority responsibility |
| Attacker/owner/source House distinction | Explicit transaction identities |
| Owner/controller/last attacker distinction | Separate persistent relations |
| Mutable damage writeback | Explicit transaction output |
| Early-exit-specific writes | Ordered effects with no implicit rollback |
| Kill credit and XP before destruction | Explicit lifecycle integration |
| Synchronous trigger mutation | Explicit ordered request |
| Shadow excluded from snapshots/hash | Required before cutover |
| Future-affecting state persisted/hashed | Required at cutover |
| Direct-field access | Removed or documented exemption |
| Unknown native semantics | Never guessed |

## Design

### Components

#### EntityState

`EntityState` is embedded in `GameEntity` and groups exact state by behavioral family.
It does not own identity, storage, mission execution, lifecycle membership, scheduler
membership, or deletion.

#### VitalityState

Conceptual representation:

```text
current: i32
maximum: i32
```

The storage accepts signed exact results and performs no automatic saturation,
normalization, healing clamp, overkill clamp, or lifecycle transition. Damage and
repair authorities own the applicable formulas, comparisons, floors, and result codes.

#### ExperienceState

The state stores the native-format experience value through the existing exact-bit/x87
support. Veteran and elite rank are derived using the verified native comparisons at
`1.0` and `2.0`; rank is not stored as an independently mutable truth unless further
research proves a distinct native field.

XP calculation, allied-kill exclusion, caps, promotion effects, and kill-credit
ordering remain outside storage. Full Veteran/Elite ability arrays belong to unit-type
or rules state rather than individual entities and must preserve verified parsing and
replace-on-present behavior.

#### ReadinessState

The initial state contains only verified signed current and maximum values. Storage
does not impose a clamp. The calling authority preserves operation-specific behavior,
including cases where negative damage raises readiness above maximum and where only a
lower clamp is applied.

Damage-ordered readiness writes remain committed even when a later immunity or bunker
branch exits. Aircraft docking target, pad, phase, and rescan state stays in the docking
subsystem unless research proves it is shared entity authority.

No field is added for unresolved native reload/scheduling state merely to reserve a
place for it.

#### CombatModifierState

Verified per-instance armor and firepower multipliers use exact native-bit
representations. They remain distinct from House, country, difficulty, category, and
rules modifiers. The combat authority owns the verified application order, x87
operations, rounding, and conversion.

Fields whose writer, reset, persistence, or removal lifecycle remains unresolved do
not become authoritative.

#### EntityRelations

Persistent relations keep owning House, controlling House, and last-attacker entity
identities separate wherever verified. Stable IDs are used instead of raw Rust
references.

The attacking object, attacker owner, and source House are transient damage-transaction
inputs, not aliases for persistent victim state. Reference expiration, detach, and
final-reference handling remain with reference authority rather than being duplicated
inside this substrate.

### Interfaces and Contracts

Conceptual read interface:

```text
EntityStateView
- vitality()
- experience()
- readiness()
- combat_modifiers()
- relations()
```

Conceptual mutation interface:

```text
EntityStateMut
- write_vitality_exact(...)
- write_experience_exact(...)
- write_readiness_exact(...)
- write_combat_modifiers_exact(...)
- write_relation_exact(...)
```

These methods are exact state primitives, not gameplay formulas.

Interface invariants:

- A gameplay operation computes its result once from explicit inputs.
- Legacy and shadow updates do not independently rerun formulas or consume RNG.
- Writes are visible immediately to later verified operations in the same tick.
- The facade cannot implicitly trigger destruction, concealment, scheduler removal,
  experience award, trigger execution, or deletion.
- Unsupported or unresolved state has no public mutation API.
- Direct field access is temporary and must be removed or explicitly exempted before
  cutover.

### Data Flow

#### Shadow mutation

```text
Gameplay authority
    |
    | verified ordered operation
    v
Entity-state facade
    |- update legacy representation
    |- update exact shadow representation
    `- record classified comparison
             |
             v
Gameplay continues from legacy-authoritative result
```

Before cutover, exact-state diagnostics cannot affect gameplay, RNG, timing,
iteration order, snapshots, hashes, or replay results.

#### Damage boundary

```text
Damage request
    v
Read explicit entity / rules / House inputs
    v
Verified ordered receiver logic
    |- mutable damage writeback
    |- readiness mutation
    |- immunity or other early-exit result
    |- health mutation
    |- attribution mutation
    |- kill-credit / XP request
    |- synchronous trigger request
    `- lifecycle request
```

The substrate records exact state mutations but neither calculates nor reorders those
effects. Early exits retain exactly the mutations that occurred before the exit.

#### Lifecycle boundary

```text
Health/result transition
    v
Explicit lifecycle request
    v
Ordered lifecycle authority
    |- trigger consequences
    |- kill credit / XP at verified point
    |- mission/activity changes
    |- conceal / limbo
    |- scheduler removal
    `- uninitialization / deletion
```

Zero or negative health does not independently imply concealed, unscheduled, limbo,
uninitialized, deleted, or untargetable state.

### Shadow Comparison Contract

Each comparison receives enough context to identify the entity, tick, state family,
operation, legacy value, and exact value. It is classified as:

- `Equal`: both representations express the same state.
- `ExpectedRepresentationGap`: legacy storage cannot express a valid exact state.
- `SemanticDivergence`: both can express the state, but behavior differs.
- `Uncomparable`: required native semantics remain unresolved.

Expected representation gaps are surfaced rather than normalized away. Unexplained
semantic divergence blocks the authority flip.

The comparison record is diagnostic data and is never serialized simulation state.

### Error Handling

- Shadow mismatches are diagnostics, not simulation errors.
- Diagnostics cannot consume RNG, mutate state, or change control flow.
- Storage does not invent clamping, overflow, substitution, or recovery rules.
- Arithmetic semantics belong to the verified caller; storage accepts its exact result.
- Missing entity and House identities follow the verified reference-authority policy.
- Debug assertions may detect broken internal invariants, but release semantics cannot
  depend on assertions.
- Research-unknown operations are unavailable rather than implemented approximately.

### Testing Strategy

#### Representation tests

Verify signed boundaries, exact native bit patterns, promotion thresholds, identity
separation, and lossless state round trips.

#### Shadow writer tests

Each migrated writer proves one operation updates both representations without
duplicating formulas or RNG consumption.

#### Reader comparison tests

Before cutover, migrated readers must still return legacy-authoritative results while
recording exact-state comparisons.

#### Ordered integration tests

Cover at minimum:

- readiness mutation before later immunity/early exit;
- health and damage-result transitions;
- last-attacker and other attribution writes;
- kill credit and XP ordering;
- synchronous trigger effects; and
- explicit handoff to lifecycle authority.

#### Retail-derived parity checks

Parity claims require named gamemd traces, retail-derived captured outputs, or
exhaustive proof. Hand-computed values and Rust-vs-prior-Rust fixtures are regression
checks only.

#### Determinism tests

Prove enabling shadow diagnostics does not change simulation state, RNG, hashes,
iteration order, replay output, or gameplay results.

#### Persistence tests

At cutover, verify snapshot round trips and world-hash sensitivity for every
future-affecting authoritative field.

#### Access-inventory check

Automated search or equivalent tooling must reject unauthorized direct legacy reads
and writes before cutover.

## Research Gates

The initial authoritative bundle must not guess the following:

- complete semantics and consumer set of the unresolved Techno reload/scheduling field;
- full readiness/ammo writer, reset, save/load, and removal lifecycle;
- full per-unit firepower writer, reset, save/load, and removal lifecycle;
- House combat-field reapplication and persistence lifecycle;
- semantic identity of the uncertain damage receiver argument;
- unresolved native postlude/helper fields and effect helpers;
- Building damage-sound fallback behavior; and
- incomplete wrapper matrices and inheritance behavior.

Each gate must be resolved by research or explicitly excluded from the first authority
scope. `UNKNOWN` does not become a placeholder behavior.

## Staged Rollout

1. Close or formally bound the named research gates.
2. Introduce exact value representations and exact rule/type data.
3. Add the embedded shadow bundle without snapshot or hash participation.
4. Inventory and route writers through the facade one state family at a time.
5. Route readers through legacy-authoritative comparison accessors.
6. Run, classify, and resolve shadow comparisons.
7. Audit every direct access and unresolved exclusion.
8. Perform one coordinated authority flip with snapshot/hash/version ownership.
9. Remove legacy storage and temporary comparison plumbing.
10. Re-run damage, repair, veterancy, readiness, reference, lifecycle, replay,
    determinism, and persistence validation.

The authority phase is code/version state, not mutable per-match state. A saved game
cannot load into an ambiguous mixed-authority mode.

## Authority-Flip Gates

The coordinated cutover is blocked until:

- all reader and writer inventories are closed;
- required research gates are resolved or explicitly excluded;
- all semantic divergences are resolved;
- representation gaps have deliberate exact behavior;
- damage, repair, veterancy, kill-credit, readiness, reference, and lifecycle
  integration checks pass;
- every claimed parity behavior names executable gamemd/retail-derived evidence or an
  exhaustive proof;
- snapshot serialization and world-hash coverage are ready;
- one session owns the snapshot-version bump and golden rebaseline; and
- legacy direct access is prevented through visibility or module boundaries.

Compilation alone is not a completion criterion.

## Architectural Decisions

1. **Entity-owned bundle:** exact state follows entity ownership and lifetime.
2. **Facade-owned access:** systems cannot bypass ordered mutation contracts after
   migration.
3. **Legacy authority during shadowing:** the migration cannot change gameplay before
   evidence is ready.
4. **One coordinated authority flip:** the simulation never operates with several
   incompatible authoritative representations.
5. **Lifecycle remains separate:** health storage does not own reveal, conceal, limbo,
   scheduling, uninitialization, or deletion.
6. **Transactions distinguish identities:** attacker object, owner, source House,
   victim owner, controller, and last attacker are not interchangeable.
7. **Unknown fields stay absent:** native-looking placeholders are prohibited.
8. **Exact numeric representation:** native-bit/x87 facilities are reused rather than
   ordinary Rust floating-point gameplay arithmetic.
9. **Diagnostics are non-authoritative:** comparison machinery cannot affect future
   state.
10. **Persistence changes occur at cutover:** shadow data cannot leak into saves or
    hashes.

## Alternatives Considered

### Flat exact fields directly on GameEntity

This minimizes initial structure but expands an already broad entity type, encourages
direct access, weakens behavioral ownership, and makes ordered migration harder to
audit. Rejected.

### Sidecar EntityStateStore

This makes initial shadow comparison easy but duplicates entity identity and lifetime,
introduces stale-sidecar risks, and creates another cleanup/reference authority.
Rejected.

### Immediate replacement of legacy fields

This avoids temporary duplication but couples representation changes to combat,
veterancy, readiness, lifecycle, snapshots, hashing, and reference behavior before
their evidence and integrations are ready. Rejected.

### Independent authority flips per state family

This appears incremental but permits damage, lifecycle, persistence, and attribution
to observe incompatible truths. Preparation is incremental; authority change is
coordinated. Rejected.

## Handoff

No Rust implementation is authorized by this document alone. The next artifact should
be an implementation plan that breaks the rollout into reviewable tasks, begins with
research-gate closure and additive exact representations, avoids currently shared file
ownership, and assigns explicit verification and stop conditions to every stage.
