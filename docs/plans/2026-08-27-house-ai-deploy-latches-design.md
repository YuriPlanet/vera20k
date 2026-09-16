# Phase 3 successful AI ConstructionYard deploy latches

**Status:** proposed bounded design
**Phase/GSI:** Phase 3 / GSI-04.05
**Native owner:** `UnitClass__Deploy @ 0x007393C0`, straight-line stores `0x007398FF..0x00739919`
**Research authority:** [docs/research/PHASE3_UNIT_DEPLOY_HOUSE_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_UNIT_DEPLOY_HOUSE_FLAGS_GHIDRA_REPORT.md)

## 1. Bounded requirement

After an MCV has successfully become a Building, active retail enters the already-implemented AI ConstructionYard anchoring block only when the owner is not controlled by a human, the deployed target has `ConstructionYard=yes`, and the game mode is nonzero. After primary center, optional Recalc, BasePlan node-zero, and BasePlan-center writes, native writes three independent House bytes to literal one in this exact order:

1. `House+0x1EE Production`;
2. `House+0x1F2 AITriggersActive`;
3. `House+0x1F3 AutoBaseBuilding`.

The stores have no branch or call between them and precede `FUN_0050C920`. Repeated qualifying deployments repeat the idempotent stores. Failure, facing-only rotation, human control, mode zero, and a non-ConstructionYard target must not write any latch.

This design closes only the persistent state/default/save/hash representation and this qualifying deploy transaction. It does not claim closure of the three latches' independent writer/consumer lifecycles.

## 2. Player-experience and deterministic ledger

| Observable/deterministic effect | Required result |
|---|---|
| Successful ordinary AI ConYard deploy | All three latches become true after anchoring. |
| Human, campaign/mode-zero, non-ConYard, blocked, preflight-rejected, or facing-only deploy | All three retain their prior independent values. |
| Save/load after a qualifying deploy | All three values round-trip exactly, including noncanonical independent combinations constructed by other native writers. |
| Lockstep/native-shaped House fold | Production and AITriggersActive change the current House hash; AutoBaseBuilding is serialized but is not directly folded, matching the active House CRC census. |
| Failed or later-empty dispersal | Cannot roll back the already committed latches; dispersal remains a later slice. |

## 3. Native evidence and exclusions

- `UnitClass__Deploy 0x00739855..0x00739926` proves the outer gate, anchoring-before-latches order, literal values, store order, and later dispersal call.
- `HouseClass__Constructor 0x004F56F1/0x004F570A/0x004F5710` initializes all three bytes to zero.
- `HouseClass__Save/Load` raw-serializes all three within the `0x160B8` House block.
- raw House CRC `0x00502D60..0x0050303F` directly feeds Production at `0x00502E58` and AITriggersActive at `0x00502E74`; the exhaustive `+0x1F3` census proves no direct AutoBaseBuilding feed.
- `FUN_00505180` and `Computer_Paranoid` touch none of the bytes. `FUN_0050C920` is after the stores and touches none of them.
- Active independent mechanisms deliberately left open: Production's House-update and factory-tail consumers plus action 3/opcode 29; AITriggersActive's selector and actions 74/75; AutoBaseBuilding's House-update and Unit AI/Guard consumers plus action 30; ComputerTakeover; `AutocreateAllowed`; and deploy dispersal.
- Action 30 and team opcode 29 have zero hits in the enumerated installed retail corpus, but remain compiled/custom compatibility surfaces and are not claimed absent globally.

Any of those open items keeps GSI-04.05 open. Their absence does not invalidate this narrower transaction closure.

## 4. State ownership

Add a small serialized House-owned latch group in `src/sim/house_state.rs` with three independently addressable booleans:

- `production`;
- `ai_triggers_active`;
- `auto_base_building`.

The group derives/implements `Debug`, `Default`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Serialize`, and `Deserialize`; every default is false. `Debug` is required because `HouseState` derives it. Keeping a group prevents three unrelated top-level fields from drifting apart in snapshot/hash plumbing while retaining their independent semantics. It must not reuse `AiPlayerState::mcv_deployed`, which has different ownership, gates, and lifetime.

Provide a House method whose body assigns `production = true`, then `ai_triggers_active = true`, then `auto_base_building = true`. It performs no other mutation and has no fallible step. This method is the sole write introduced by this bounded slice.

## 5. Deploy integration

In `Simulation::deploy_mcv`, reuse the existing `recalc_context` qualification. After the already-committed BasePlan-center write and before returning success, call the latch-enabling House method inside that same qualifying block.

Do not:

- create a second, subtly different gate;
- move the stores before destructive-deploy preflight, unit removal, Building spawn, primary center, Recalc, node-zero anchoring, or BasePlan-center anchoring;
- set the latches merely because `deploy_mcv` returned true for facing rotation;
- set them for a deployable non-ConstructionYard such as `SMIN -> YAREFN`;
- set neighboring AutocreateAllowed;
- call or approximate dispersal.

An empty-plan countryless qualifying deploy still fails before source removal and therefore retains all latch values. A countryless nonempty plan succeeds without Recalc and sets all three after its center/node writes.

## 6. Snapshot and hash schema

Append the latch group to `HouseState` and bump the current-main `SNAPSHOT_VERSION` from 111 to 112 because bincode encodes the struct positionally. Update every current-version assertion/label that hard-codes 111; current-version tests should prefer `SNAPSHOT_VERSION` except the dedicated version-contract test.

Extend `Simulation::state_hash_with_schema` with `include_house_deploy_latches_v112`. The current hash enables it; historical probes disable it. When enabled, `hash_houses` directly folds only:

1. `house.ai_activation.production`;
2. `house.ai_activation.ai_triggers_active`.

It must not directly fold `auto_base_building`. This is an intentional native CRC asymmetry, not an omission. Add `state_hash_without_house_deploy_latches_v112` as a test-only provenance probe.

## 7. Validation

Focused tests must cover:

1. fresh `HouseState` defaults all three false;
2. all eight independent boolean combinations serialize and load exactly under snapshot v112;
3. Production-only and AITriggersActive-only changes alter the current hash but not the v111 historical probe;
4. AutoBaseBuilding-only changes do not directly alter either current or historical House hash;
5. successful nonhuman, nonzero-mode ConYard deploy sets all three and retains existing center/BasePlan/RNG assertions;
6. human, mode-zero, non-ConYard, placement failure, malformed-vector preflight failure, and facing-only cases retain prior latch combinations;
7. nonempty countryless qualifying deploy sets the latches, while empty-plan countryless preflight failure preserves them;
8. the latch-enabling method starts from a split independent combination, writes all three true in native order, and a second invocation leaves the same all-true state;
9. `git diff --check` is clean.

Run scoped `cargo test -p vera20k --lib <filter>` commands while working after checking that no other process owns Cargo. This integration PR still requires the repository's one final full `cargo test -p vera20k --lib` certification after the slice and critic review are stable.

## 8. Files expected to change

- `src/sim/house_state.rs`
- `src/sim/world/world_spawn.rs`
- `src/sim/deploy_tests.rs`
- `src/sim/snapshot.rs`
- `src/sim/world/world_hash.rs`

No Rules parser, House update, AITrigger selector, trigger action, team-script, production, mission, takeover, or dispersal file belongs in this bounded implementation.

## 9. Closure rule

This bounded transaction passes only when a fresh read-only critic verifies the native gate/order/state/save/hash contract and finds no unresolved behavior inside sections 1-8. Passing it records one closed active mechanism; it does not close Production, AITriggersActive, AutoBaseBuilding, `FUN_0050C920`, ComputerTakeover, or GSI-04.05.
