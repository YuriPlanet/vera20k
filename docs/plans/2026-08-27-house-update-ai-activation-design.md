# House-update AI activation design

**Date:** 2026-08-27
**Phase:** 3 / GSI-04.05 bounded mechanism
**Status:** Approved after read-only design review
**Native evidence:** [docs/research/PHASE3_HOUSE_UPDATE_AI_ACTIVATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSE_UPDATE_AI_ACTIVATION_GHIDRA_REPORT.md)

## Goal

Implement the active-retail `HouseClass__Update @ 0x004F8440` activation transition at
`0x004F8564..0x004F85B7`, including its signed `[IQ] Production` input, fourth persistent House
latch, ordered House-array execution, snapshot representation, and direct House CRC shape.

This is one bounded House mechanism, not closure of all GSI-04.05 ownership. Trigger action 13,
factory-production policy, ordinary AutoBase consumers/action 30, AITriggersActive selection,
computer takeover, deploy dispersal, and the broader network-modal scheduler remain distinct
mechanisms and keep the row open.

## Verified retail contract

For each non-null House reached in forward live House-array order:

```text
controlled = CurrentPlayer || (GameMode == 0 && PlayerControl)
if !controlled && (AutoBaseBuilding != 0 || CurrentIQ >= Rules.IQ.Production) {
    AutoBaseBuilding = 1
    Production = 1
    AutocreateAllowed = 1
}
```

The comparison is signed and inclusive. Any nonzero AutoBase value bypasses the IQ read. The
three stores are adjacent, ordered, literal-one writes and have no intervening call, branch, RNG,
timer, or side effect. Controlled, defeated, passive, low-power, and difficulty state add no gate.
The transition never writes AITriggersActive.

`[IQ] Production` is a signed dword with constructor default `5`; a present INI value replaces it
without clamp. Stock retail `rules.ini` and `rulesmd.ini` both resolve to `5`.

`AutocreateAllowed` is constructor-cleared, raw-save/load persistent, and directly included in
the House CRC between Production and AITriggersActive. Its exhaustive direct-access census finds
no ordinary gameplay reader. TeamType `Autocreate=` is a separate mechanism and must not be wired
to this byte.

Offline mode-0/5 Menu, Abort Confirm, and Options own blocking service-pump loops that never call
`Main_Tick`; they therefore freeze this transition and the frame. An eligible network modal can
run the transition only as part of the complete PerTick/House/late-tail path. This design does not
change app admission or create a selective modal pass.

## Architecture fit

The existing ownership is correct:

- `GeneralRules` owns signed `[IQ]` thresholds.
- `HouseState` owns `CurrentIQ`, the exact control predicate, and persistent House latches.
- `ScenarioSession::house_order` owns native House registration order.
- `Simulation::run_late_region` is the current House-tail seam after factory/production work and
  before defeat and strategic AI command generation.
- `snapshot.rs` and `world_hash.rs` own versioned persistence and canonical deterministic folding.

The transition therefore belongs on `HouseState`, invoked by one Simulation House-order pass. It
must not be routed through `AiPlayerState`: Neutral/Special and scenario Houses are still native
House-array members.

## Data changes

### Rules

Add `GeneralRules::iq_production: i32` beside the existing `[IQ]` fields.

- `GeneralRules::default`: `5`.
- `GeneralRules::from_ini`: `[IQ] Production` through `get_i32`, falling back to the constructor
  value, with no clamp or normalization.
- The current parser returns `Self::default()` before binding `[IQ]` when `[General]` is absent,
  although native `ReadIQ` is independent. Resolve this for the new field without broadening the
  slice: compute the optional `[IQ] Production` override before the `[General]` early return and
  return `Self { iq_production, ..Self::default() }` on that path. The ordinary `[General]` path
  reuses the same parsed value. Existing IQ-only behavior for the older IQ fields is a separately
  recorded rules-ingress residual, not a reason to make the new native input wrong.
- Parser tests must cover missing section/key, stock value `5`, negative value, value above
  `MaxIQLevels`, and an `[IQ]`-only file with no `[General]`; custom signed values must survive
  verbatim.

### House latches

Extend `HouseAiActivationLatches` to four independent booleans in native conceptual byte order:

```rust
production
autocreate_allowed
ai_triggers_active
auto_base_building
```

All default false. Snapshot schema 113 rejects prior positional records, so reordering the three
existing fields into native order does not create an accepted mixed layout. Update every explicit
fixture literal so the compiler proves the fourth state was considered.

`enable_ai_deploy_latches` remains the deploy transaction and still writes exactly Production,
AITriggersActive, AutoBaseBuilding in that order. It must not write or clear AutocreateAllowed.

## Transition API and scheduling

Add a House-owned helper, named to local convention, with inputs
`(game_mode_nonzero: bool, iq_production: i32)`. It performs the verified control test, AutoBase
bypass, signed inclusive IQ comparison, then assigns:

1. `auto_base_building = true`;
2. `production = true`;
3. `autocreate_allowed = true`.

It leaves `ai_triggers_active` unchanged. Repeated eligible calls execute the same assignments and
remain idempotent. No return value should imply a native “changed” edge that does not exist.

Add one private Simulation pass that walks `ScenarioSession::house_order` by increasing index and
reloads the current vector length each iteration. Missing/null-equivalent map entries are skipped.
For each present House it calls the helper with `self.session.game_mode_nonzero` and
`rules.general.iq_production`.

Call the pass in `run_late_region`:

1. after the existing early House-like vision reconciliation;
2. after the factory/production sweep that precedes `run_late_region`;
3. before `check_defeat`;
4. before `ai::tick_ai`.

Rules-less fixtures perform no activation, matching the absence of a live native Rules owner in
that artificial path. Both `TickLane::Ordinary` and any already-admitted future
`TickLane::NetworkModal` reach the same House tail; app-level offline modal admission remains
unchanged and continues to call neither lane.

## Persistence and hash contract

### Snapshot

Bump `SNAPSHOT_VERSION` from the current-main 112 to 113 and document the addition of
`AutocreateAllowed`. Replace the eight-combination latch test with all 16 combinations. Current
snapshot assertions must refer to the current constant except for the explicit 113 contract test.

### Hash

Add a v113 schema switch after the v112 deploy-latch switch. Native places CurrentIQ after the
three direct activation booleans, while committed Rust v112 placed CurrentIQ before Production and
AITriggersActive. Correct the native-relative position only for current v113; preserve the exact
historical v112 and pre-v112 streams.

After the existing `tech_level` fold, the current v113 direct House fold must be:

```text
Production
AutocreateAllowed
AITriggersActive
CurrentIQ
```

AutoBaseBuilding remains directly excluded. Implement this by separating the existing v112
Production and AITriggers folds around the new conditional Autocreate fold and conditionally
placing CurrentIQ:

- when v113 is disabled, fold CurrentIQ at the old position before any v112 latch fields;
- when v112 is enabled, fold Production;
- when v113 is enabled, fold AutocreateAllowed;
- when v112 is enabled, fold AITriggersActive;
- when v113 is enabled, fold CurrentIQ at the corrected native position.

Thus the v112 historical probe remains `CurrentIQ -> Production -> AITriggersActive`; pre-v112
remains `CurrentIQ` only; current v113 becomes
`Production -> AutocreateAllowed -> AITriggersActive -> CurrentIQ`.

Extract the four-field fold into a small private helper if needed so an ordering-sensitive unit
test can compare its output to a manually constructed `DefaultHasher` stream with deliberately
distinct values. Differential whole-state hashes alone cannot prove sequence order.

Differential tests must show:

- current hash changes for Production, AutocreateAllowed, or AITriggersActive;
- current hash does not change for AutoBaseBuilding alone;
- the pre-v113 probe ignores AutocreateAllowed;
- the v113 helper/current fold equals the manual native sequence above;
- the v112 historical helper/probe equals the manual committed sequence above;
- the existing pre-v112 probe remains stable and omits all latch additions.

## Expected files

- `src/rules/ruleset.rs`
- `src/sim/house_state.rs`
- `src/sim/world/mod.rs`
- `src/sim/deploy_tests.rs`
- `src/sim/snapshot.rs`
- `src/sim/world/world_hash.rs`
- optionally one focused test module under `src/sim/world/` if keeping the House-tail scenarios
  out of `world_tests.rs` materially improves clarity

No app, UI, TeamType, trigger-runtime, AI-player, or map-data file changes are authorized by this
design.

## Validation ledger

Focused tests should share the `house_ai_activation` name fragment where practical so one scoped
Cargo filter exercises the mechanism.

### House helper

- fresh state: four false latches and zero CurrentIQ;
- below/equal/above signed threshold;
- negative threshold with zero IQ;
- AutoBase bypass far below threshold;
- campaign CurrentPlayer and PlayerControl exclusions;
- nonzero-mode PlayerControl ignored, CurrentPlayer still excludes;
- repeated eligible visit idempotence without RNG/timer/counter mutation;
- eligible Houses already marked defeated or passive, across at least two difficulty values, still
  activate; these common neighboring gates must not leak in from `tick_ai`;
- split state after AutoBase clear below threshold remains split;
- split state at threshold restores AutoBase/Production/Autocreate;
- deploy-produced `{Production, AITriggers, AutoBase}=true, Autocreate=false` gains only the missing
  Autocreate semantic state on the next eligible House visit;
- AITriggersActive is preserved in every path.

### House-tail integration

- forward `house_order` visits human, computer, and Neutral/Special fixtures once and activates
  only eligible members;
- a House eligible on the same frame as later defeat is activated before defeat processing;
- the full-frame call reaches activation after the production sweep and before AI command
  generation;
- `advance_master_frame(..., TickLane::NetworkModal, ...)` with an empty command slice runs
  activation as part of the admitted complete frame; this pins House activation without blessing
  the lane's known generic due-command mismatch;
- rules-less fixture is unchanged;
- existing offline modal decision tests continue to prove zero admitted frame/activation while a
  mode-0/5 modal is open; no app change is needed.

### Persistence/hash and regressions

- all 16 latch combinations round-trip schema 113;
- current/direct, manual-order, and historical hash tests above;
- existing qualifying deploy, facing-only deploy, controlled deploy, malformed/blocked deploy,
  and BasePlan tests remain green and prove deploy never writes AutocreateAllowed;
- existing BasePlan/BuildConst/naval placement behavior remains unchanged;
- `git diff --check` is clean.

Run only focused `cargo test -p vera20k --lib <filter>` commands for this mechanism. The parent
Phase-3 run owns the single final full `cargo test -p vera20k --lib` certification.

## Evidence-backed exclusions and open ownership

- Trigger action 13 is not part of this builder slice. It is absent from the 310 effective shipped
  map payloads surveyed, but remains required for compiled/custom parity and keeps GSI-04.05 open.
- No House AutocreateAllowed consumer is added: none exists in the active executable.
- TeamType `Autocreate=` remains independent.
- Offline modal admission is preserved; no House-only service lane is invented.
- Broader NetworkModal phase partitioning is not necessary to implement or validate the ordinary
  House transition and remains a scheduler residual. In particular, native admitted network
  modals still dispatch generic queued commands after PerTick, while current Rust suppresses the
  entire late command slice. Only the Rust-specific offline Options `SetGameSpeed` ingress is
  correctly Ordinary-only. This builder must neither change nor assert the generic suppression.
- Production consumers, AutoBase consumers/action 30, AITriggersActive, takeover, and deploy
  dispersal retain their existing owners and remain row residuals until separately closed.

## Completion condition for this mechanism

The mechanism is ready for a builder only after a read-only design critic approves this document
against the native report and current source. After implementation, a fresh critic who did not
build it must receive the requirement, native evidence, commit diff, and literal focused outputs.
Any finding reopens the mechanism; repair the largest finding first and submit the repaired result
to a new critic who also rechecks prior fixes.
