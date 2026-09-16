# GSI-04.05 Building Attack-Frame Writer Design

## Goal

Connect the already serialized House last-Building-attack frame to the exact
`BuildingClass__ReceiveDamage` prelude without changing receiver order, adding
false gates, or claiming the adjacent responder-selection mechanism complete.

## Architecture Context

`EntityDamageEvent` is the one ordered Rust record for area and concrete direct
receiver calls. It retains `attacker_id` separately from `source_house`, then
`commit_damage_events_with_isolation` processes each admitted record in native
order. The commit currently calls pure `resolve_receive_damage` first and only
then mutates entity/House consequences. That function takes Houses immutably and
owns generic Techno receiver calculation, so it is too late and the wrong
authority for this Building-specific prelude.

`HouseState.strategy_emergency.last_building_attack_frame` is already the
snapshot/hash-covered authority, with `note_building_attack` as its setter.
`ObjectType` already retains `DamageSelf`, `UndeploysInto`, and the merged art
foundation. Its `base_reservation_writer_eligible` method independently contains
the same verified `UndeploysInto && 1x1` type predicate as one of its gates.

The new research report
[docs/research/PHASE3_HOUSE_LAST_BUILDING_ATTACK_FRAME_0044229C_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSE_LAST_BUILDING_ATTACK_FRAME_0044229C_GHIDRA_REPORT.md)
is primary evidence. It verifies active YR receiver dispatch, exact entry order,
all direct `House+0x54D8` references, and retail-data activation. No relevant
House/combat `*_SYSTEM_MODEL_SYNTHESIS.md` exists; lifecycle synthesis documents
are orientation only and do not supersede this receiver-specific evidence.

## Impact Analysis

- `src/rules/object_type.rs`: expose the already represented, body-proven
  `is_1x1_with_undeploy` predicate and reuse it in base-reservation eligibility.
  This is a no-behavior-change refactor for the prior reservation mechanism.
- `src/sim/combat/mod.rs`: run one Building wrapper prelude after transaction
  isolation admission and immediately before `resolve_receive_damage`.
- `src/sim/combat/combat_tests.rs` and existing ObjectType tests: prove the
  admission matrix, same-record ordering, and signed frame conversion.
- Save format and world-hash layout do not change; the target field landed in
  snapshot version 96 in the preceding slice.
- The timestamp prelude consumes no RNG. Its position must not move the
  still-missing `FUN_00708080` responder-selection RNG into this slice.
- Railgun, LaserDraw, Sonic Wave, destroyable cliffs, and TS legacy are untouched.

## Chosen Approach

Add a private, explicitly named Building receiver prelude in `combat/mod.rs`.
It returns either `Continue` or `ReturnZero` and may write the victim House
timestamp. For non-Buildings it is a no-op. For a self Building hit with
`DamageSelf=no`, `ReturnZero` causes the ordered event to stop before generic
receiver work, matching the native wrapper rather than merely suppressing the
timestamp. Otherwise a non-null attacker ID and a type that is not a 1x1
undeployer write `current_tick as u32 as i32` through the House setter.

The prelude uses the event's nonzero `attacker_id` as the preserved native
“attacker object argument was non-null” fact. It must not require
`entities.get(attacker_id)` for non-self records: a prior record can uninitialize
the represented source while the ordered event still retains the non-null
source argument. `source_house` alone never qualifies.

This approach preserves the receiver commit as the sole mutable order owner,
keeps House state in House authority, and shares the exact type predicate rather
than duplicating a misleading cloak-like condition.

## Player-Experience Detail Ledger

- **MILESTONE-BLOCKING — same-record timing:** write before Building immunity,
  already-dead handling, and generic Techno receiver; an immune hit still resets
  later Strategy/replacement deadlines. [GHIDRA `0x00442290..0x00442425`]
- **MILESTONE-BLOCKING — self entry:** `attacker == victim` with
  `DamageSelf=no` returns result zero before all receiver work. The commit must
  stop the record, not only skip bookkeeping. [GHIDRA `0x00442243..0x00442262`]
- **COMPOUNDING — attacker object versus source House:** object-null suppresses
  the write even if source House exists; a non-null object writes independently
  of source House. Conflating them changes sourceless/special damage behavior.
  [GHIDRA `0x0044227E..0x00442280`]
- **COMPOUNDING — raw signed frame:** native copies one dword; downstream
  readers use wrapping addition and signed comparisons. Rust must truncate
  `u64` through `u32` to `i32`, not saturate. [GHIDRA `0x00442296..0x0044229C`,
  `0x004FD80A..0x004FD840`]
- **COMPOUNDING — no false outcome gates:** alliance, requested damage sign,
  immunity, Health zero, attacker limbo, and eventual result do not suppress
  the store. Adding any of them changes AI abandonment/replacement timing.
  [doc: [PHASE3_HOUSE_LAST_BUILDING_ATTACK_FRAME_0044229C_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSE_LAST_BUILDING_ATTACK_FRAME_0044229C_GHIDRA_REPORT.md)
  §§1,6]
- **COMPOUNDING — exact type predicate:** only a type with both non-null
  `UndeploysInto` and exactly 1x1 foundation skips. It is not cloak state.
  Retail's complete set is 4x4/4x4/4x4/2x2, so ordinary retail undeployers still
  write; mod data remains exact. [GHIDRA `0x00457620`, `0x00465D40`; ini/art
  ledger in research §3]
- **COMPOUNDING, STILL OPEN AFTER THIS SLICE — adjacent response block:** every
  qualifying store also writes attacker-owner House index at `+0x54DC` and
  invokes responder selection, which can consume Scenario RNG and assign
  defenders. Trigger: qualifying Building attack; player effect: AI response;
  ordinary frequency: potentially common; downstream risk: mission/RNG order.
  It requires its own bounded research/builder/reviewer mechanism and keeps
  GSI-04.05 open. [GHIDRA `0x004422A2..0x004422BC`, `0x00708080`]
- **UNKNOWN-RISK, NON-ADDITIVE — victim limbo caller reachability:** the wrapper
  has no local victim-limbo gate, while ordinary area collection uses active
  objects. This design deliberately adds no limbo test, preserving the local
  contract without inventing a caller exclusion. [research OQ-15]

## Design

### Components

1. `ObjectType::is_1x1_with_undeploy()` returns
   `undeploys_into.is_some() && foundation_dimensions(...) == (1, 1)`.
2. A private `BuildingReceivePrelude` enum makes the early-return contract
   visible at the commit site.
3. `apply_building_receive_prelude(event, entities, rules, interner, houses,
   current_tick)`:
   - resolves the target; non-Structure or missing target continues;
   - resolves target type; missing unsupported rules data continues without a
     House write;
   - returns `ReturnZero` for self with `DamageSelf=no`;
   - writes only for nonzero attacker ID and a type that is not a 1x1
     undeployer;
   - looks up the victim owner House and calls `note_building_attack`;
   - returns `Continue`.
4. `commit_damage_events_with_isolation` calls the prelude after its
   transaction-level isolation `continue` and before `resolve_receive_damage`.

### Interfaces / Contracts

The helper is private to combat. It does not expose House state broadly, does
not mutate entities, does not consume RNG, and does not return an approximation
of native receiver results. `ReturnZero` means only the verified Building
self/DamageSelf entry return; other receiver outcomes stay with the existing
resolver.

The ObjectType predicate is public because two independently verified native
mechanisms use it. `base_reservation_writer_eligible` is rewritten to call it,
keeping one semantic definition.

### Data Flow

```text
ordered EntityDamageEvent
  -> transaction isolation admission
  -> Building receiver prelude
       -> optional House timestamp write
       -> optional native result-zero event stop
  -> existing resolve_receive_damage
  -> existing HP/retaliation/death commit
```

### Error Handling

Missing target, type, or House data is unsupported/incomplete Rust state. The
helper remains panic-free: missing target/type produces no prelude write and
lets existing receiver handling decide; missing House drops only the impossible
native owner write. No fallback to source House, attacker live lookup, target
category heuristics, or default foundation is introduced.

### Testing Strategy

Focused `--lib` tests will prove:

- null attacker plus non-null source House does not write;
- ordinary hostile and allied attackers write;
- negative and zero requested damage write;
- receiver immunity and already-zero Health cannot prevent the prelude write;
- self `DamageSelf=no` returns before both write and HP change, while `yes`
  proceeds;
- a modded 1x1 undeployer skips while a stock-shaped 2x2/4x4 undeployer writes;
- repeated qualifying records in one frame remain admitted;
- `current_tick` truncates through raw 32-bit storage to the expected signed
  value;
- the reused ObjectType predicate preserves prior base-reservation eligibility.

The focused command must be a scoped `cargo test -p vera20k --lib <filter>`
after checking for other Cargo/rustc owners. No full-suite run belongs to this
intermediate GSI slice.

## Architectural Decisions

- Follow the existing ordered combat commit rather than introducing a second
  House/combat queue or deferred command.
- Reuse House serialization/hash authority; no duplicate timestamp lives on a
  Building or AI player.
- Represent the native vtable result as a Rust type predicate, not C++ virtual
  plumbing.
- Make the self early return explicit so later reviewers cannot accidentally
  move it after generic damage.
- Keep `House+0x54DC` and responder selection separate and visibly open; this
  slice is coherent but does not close GSI-04.05.

No new technical debt is accepted. The only residual is an already verified
adjacent native mechanism that needs a separate scope because it has its own
RNG, mission, and target-selection authority.

## Alternatives Considered

### Put the write inside `resolve_receive_damage`

Rejected. That function owns pure generic receiver calculation, borrows Houses
immutably, and can return early after lookups/gates. Making it mutate House
state would hide Building wrapper order and couple calculation to commit.

### Write when damage events are emitted

Rejected. Emission precedes transaction isolation and final ordered receiver
dispatch. Some records never enter the receiver, and earlier records can change
later lifecycle state. Emission-time writes would be observably early.

### Update the frame lazily from House Strategy

Rejected. It loses the actual attack frame, cannot represent multiple attacks
between Strategy ticks, and breaks the independently active replacement-timer
reader at `0x0050CBCB`.

## Adversarial Approval

- **Why approve?** The design places the write at the only existing mutable
  receiver-order authority and uses already authoritative House/type state.
- **What could still make ordinary skirmish feel wrong?** Missing responder
  selection can leave AI defenders idle and alter Scenario RNG; it is explicitly
  the next open mechanism, so neither this commit nor the row will be called
  complete.
- **What could create expensive later rework?** Treating `source_house` or live
  attacker lookup as object presence would force event-model repair later. The
  design uses the already retained attacker-ID fact and snapshots no new data.
- **What is the largest local regression risk?** Accidentally applying the
  Building self return to all entity categories. The helper gates Structure
  first and focused tests include category separation.

**Decision:** self-approved under the autonomous Phase 3 goal for this one
writer slice. Implementation may proceed; GSI-04.05 remains open afterward.
