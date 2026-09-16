# Phase 3 AITrigger Selector Design

## Goal

Implement the active Yuri's Revenge House AITrigger selector, its empty-Team output boundary, and adaptive trigger feedback exactly enough to make later live recruitment consume the same deterministic decisions as retail, without touching Railgun, LaserDraw, Sonic Wave, destroyable cliffs, or TS-legacy behavior.

The Phase 3 Team-production row remains open after this slice. Stage-C live recruitment is a separate active mechanism with its own builder/reviewer round; selector-created Teams must remain inert and pending until that mechanism is installed.

## Architecture Context

The fixed `AIMD.INI` plus map overlay pipeline already resolves ScriptType, TaskForce, TeamType, and AITriggerType records into `TeamScriptVm` in native registry order. `TeamAiTriggerDefinition` retains the 18 source tokens and the proven static fields, while `TeamTypeIniMetadata` retains `Max`, `Autocreate`, recruitability, and authored fields. `TeamScriptVm` also owns live Team state and is therefore the only current Rust owner which can preserve native Team order, TeamType counts, cancellation state, and destruction feedback without duplicating authority.

The Simulation master frame already runs the copied Team pass before the Logic/object walk and runs House/generic-AI work in the late region after object and production consequences. Native does the same load-bearing ordering: the global Team pass precedes `HouseClass::Update`, which invokes selector `0x006F0AB0`; a Team constructed in that House tail cannot recruit or execute a script until the next frame. The selector belongs in the late House rung, but it must remain separate from `sim::ai::tick_ai`, whose current Rust-only production and attack-wave state is neither the native AITrigger registry nor the native Team producer.

Existing state owners relevant to selector facts are:

- `rules::ruleset::GeneralRules` for `[General]` values;
- `ScenarioSession` and scenario loading for mode, House order, and scenario flags;
- `HouseState` for country, side, difficulty, human/control/passive gates, TechLevel, enemy House, power, and economy;
- `TeamScriptVm` for ordered triggers, ordered live Teams, TeamType metadata, construction, and destruction;
- `EntityStore` plus production state for authoritative owned-Building order and primary-factory identity;
- the zone map for the already verified non-bridge base-zone relation;
- the superweapon subsystem for per-House instance state, although its current `BTreeMap` key order is not the native first-instance order required by conditions 5 and 6;
- the scenario RNG, which is the only permitted source for both selector draws.

The design follows the existing rules-to-Simulation data flow and subsystem ownership. It adds one Rust-native selector facts adapter instead of reproducing native pointer vectors, vtables, or the House inheritance tree.

## Impact Analysis

Expected implementation surfaces:

- `src/rules/ruleset.rs` and focused rules tests: parse selector scalars and optional three-value vectors from `[General]` in Hard/Normal/Easy storage order;
- scenario/map ingestion and `src/sim/scenario_session.rs`: retain `[Basic] IgnoreGlobalAITriggers` and per-House `RatioAITriggerTeam`;
- House state: own ratio, active latch, selector timer start/duration, and maintained base-defense count;
- `src/sim/team_ai_selector.rs` (new): pure ordered eligibility, cap, weighted selection, and adaptive math helpers;
- `src/sim/team_script_vm.rs`: dynamic trigger state, exact empty Team construction, cancellation bytes, synchronous centralized destruction, and native CRC projection;
- `src/sim/world/mod.rs`: late-House orchestration and the immutable selector-facts adapter;
- trigger action dispatch: actions 74, 75, and 76 write the proven House fields;
- the successful MCV-deploy path: set the selector-active latch only after deployment succeeds;
- superweapon and production state: retain the narrow order/primary facts that the selector reads;
- snapshot envelope/version and deterministic state hashing;
- focused module tests and existing master-frame-order tests.

Blast-radius risks are deterministic rather than computational. A misplaced draw changes every later random outcome; a wrong House/trigger/Building/superweapon order changes selected Teams; ordinary `f64` or saturating arithmetic changes adaptive weights; incomplete destruction routing misses feedback; and adding serialized fields without a snapshot-version bump misreads older positional bincode data. The implementation must keep generic Rust AI behavior operational while installing the native Team producer beside it, because replacing generic AI is not part of this mechanism.

No visual, audio, asset, Railgun, LaserDraw, Sonic Wave, cliff, or TS-legacy module is in scope.

## Chosen Approach

Use a pure `team_ai_selector` mechanism with explicit immutable facts assembled by Simulation, ordered mutable AITrigger/Team authority retained in `TeamScriptVm`, and a late-House coordinator which consumes the scenario RNG in native order.

This is preferred over putting the selector directly in `world/mod.rs` because it keeps eligibility and weighted arithmetic independently testable and prevents a large world-owner function from silently choosing alternate order authorities. It is preferred over extending `sim::ai::tick_ai` because the latter owns a different Rust AI model, scheduler contract, command path, and RNG consumption.

The coherent slice includes all state and lifecycle consequences of one selector invocation: cadence, gates, counts/caps, eligibility, weighted output, cancellation, `Autocreate`, separate `Max` rechecks, empty construction, and adaptive feedback on every Team destruction. It does not approximate recruitment. Selector-created empty Teams carry an explicit pending-recruitment state and cannot execute ScriptType actions. Stage C will replace that narrow inert gate with the verified per-slot recruitment pipeline.

## Player-Experience Detail Ledger

- `MILESTONE-BLOCKING` — ordinary nonhuman Houses call the selector from the House update path; stock `AIMD.INI` supplies 165 triggers and retail `TeamDelays` repeatedly arms it. Activation is proven for ordinary YR skirmish, not inferred from a dormant binary helper. [doc: [PHASE3_AITRIGGER_SELECTOR_ELIGIBILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_AITRIGGER_SELECTOR_ELIGIBILITY_GHIDRA_REPORT.md) §§2,4]
- `COMPOUNDING` — the ratio draw `RandomRanged(1,100)` occurs before the ratio and active-latch tests, even at stock ratio 100. Omitting or moving it shifts the global scenario RNG forever. [GHIDRA `0x006F0AB0`; doc §4.1]
- `COMPOUNDING` — the initial timer is `TeamDelays[difficulty] + HouseOrderIndex*175`; every repeat uses only `TeamDelays[difficulty]`, and expiry resets even when selection returns nothing. Retail values are `2000,2500,3500` in native Hard/Normal/Easy storage order. [GHIDRA `0x004F70D0`, `0x005010FF`, `0x004F8A00`; doc §4.3]
- `MILESTONE-BLOCKING` — nonzero game mode skips only `IsHuman`; zero mode skips `IsHuman || PlayerControl`; every mode skips `MultiplayPassive`. The separate active latch defaults false and is written by actions 74/75, computer takeover, and successful MCV deploy. [GHIDRA `0x004F8440`, `0x004F570A`, `0x006DF2FA`, `0x006DF339`, `0x0050A7F6`, `0x0073990C`; doc §4]
- `MILESTONE-BLOCKING` — cap prepass counts all owned live Teams and owned base-defense Teams. Its strict comparisons, signed-priority earliest-tie eviction, and synchronous destruction ordering must remain literal. The eviction branch is excluded only for ordinary selector-only stock play; it stays active for maps/custom data and other Team creators. [GHIDRA `0x006F0AB0`; doc §5]
- `MILESTONE-BLOCKING` — minimum-defense behavior depends on target nullness, `UseMinDefenseRule`, the maintained House defense count, mixed primary/secondary defense identity, and strict suppression. Deriving this from aggregate Team count changes early-game defense. [GHIDRA `0x0041E720`; doc §6.1]
- `MILESTONE-BLOCKING` — source/global-ignore, enabled, token 11/session, difficulty, owner mode, country, side, and TaskForce-derived TechLevel gates are all active before a trigger can enter the weighted distribution. Zero/nonzero mode use different difficulty-byte mapping. [GHIDRA `0x0041E720`; doc §§6.2-6.3]
- `MILESTONE-BLOCKING` — conditions `-1..7`, signed payload/comparator semantics, exact Aircraft/Building/Infantry/Unit family counts, available-wallet formula, first-Civilian House, first-matching superweapon, zone relation, factory availability, and primary-then-secondary `Max` checks form one eligibility contract. A partial condition set is not admissible. [GHIDRA `0x0041E720`, `0x0049FAE0`, `0x004F6990`, `0x0041EC90`, `0x0041FEE0`, `0x00509610`, `0x005095D0`; doc §6]
- `COMPOUNDING` — family counts are native lifecycle-maintained per-type tables, not raw current-screen scans. Rust must expose an authoritative category-and-type count whose add/remove lifecycle matches live ownership, rather than reconstructing an unordered approximation inside the selector. [GHIDRA `0x00502A80`, `0x0050291D`, `0x005029A7`, `0x005027C1`, `0x00502A41`; doc §6.5]
- `MILESTONE-BLOCKING` — conditions 5/6 stop at the first same-kind superweapon even if it is inactive or unready. The current type-keyed map cannot be treated as instance order; an explicit per-House order is required. Recharge zero preserves native IEEE failure behavior. [GHIDRA `0x0041E720`, `0x006CC260`; doc §6.7]
- `MILESTONE-BLOCKING` — factory search scans the acting House's owned Building order, returns an eligible primary immediately, otherwise retains the last eligible fallback, and applies limbo, family, power, mission, owner-mask, pad, and naval gates. `active_producer_by_owner` may supply primary identity only when its production-category mapping is proven to match the queried native factory family; it cannot replace the ordered Building scan. [GHIDRA `0x005F7900`; doc §6.10]
- `COMPOUNDING` — eligible current weights use x87 `ftol` to signed `i32`; exact truncated `5000` clears the prior tier; totals/cumulative values wrap in `i32`; winner comparison is unsigned; the second RNG draw occurs only for nonempty candidates with nonzero total. [GHIDRA `0x006F0AB0`, `0x0065C7E0`; doc §7]
- `MILESTONE-BLOCKING` — output is primary then optional secondary, cancellation scans live owner Teams without retry, and uncancelled output sets `Autocreate`. Constructor bytes `Team+0x7B=0` and `Team+0x7F=0` make a new unformed Team cancel another output of the same TeamType until later formation state changes. [GHIDRA `0x006F0AB0`, `0x006E8A90`; doc §7.4 plus constructor cold check]
- `MILESTONE-BLOCKING` — output construction rechecks `Max` independently per entry, may create primary and identical secondary when `Max` permits, and may overshoot total cap by one. It creates no TaskForce members synchronously and cannot enter the already-completed Team pass that frame. [GHIDRA `0x006F09C0`, `0x006E8A90`; docs §6.11 and [PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md) §5.3]
- `COMPOUNDING` — every Team destruction updates every ordered trigger sharing that Team's primary TeamType before count/member/global removal. Success uses the action-49 Team byte; secondary-only matches receive no feedback. Current/min/max use native double semantics, counters are signed and wrapping where native increments wrap, and clamp/order follow the two verified formulas. [GHIDRA `0x0041FD60`, `0x0041FE20`, `0x006E8DE0`; doc §8]
- `COMPOUNDING` — snapshot includes current/min/max and success/attempt counters. Native CRC includes current/min/max and the documented static projection but excludes the two counters directly and excludes the Team success byte. Rust hashing must preserve that distinction. [GHIDRA `0x0041E540`, `0x0041E5C0`, `0x0041E5E0`; doc §9]
- `MILESTONE-BLOCKING` — selector vectors have no numeric constructor fallback: `TeamDelays`, minimum/maximum defense Teams, and total cap construct empty, and `DifficultyClass::ReadINI_IntVector @ 0x00475D70` leaves them empty when the key is missing. Retail data supplies all four. Rust must retain absence instead of silently substituting retail values into custom data. Scalar constructor defaults are success delta `1.0`, failure delta `-1.0`, track coefficient `1.0`, minor-super percent `0.8`, and `UseMinDefenseRule=true`; retail overrides are `20`, `-50`, `1`, `.7`, and `yes`. [GHIDRA `0x00665650`, `0x0066D530`, `0x00475D70`; ini values summarized in selector report §§4-8]
- `EXACTIFICATION-RESIDUAL` — selector cap eviction is unreachable under ordinary selector-only retail limits because defense count remains below half of total at cap, but it is implemented now because its synchronous destruction path is a shared lifecycle invariant and custom maps can reach it. Trigger: custom limits or non-selector Team creators; expected stock frequency: none; downstream risk if omitted: divergent Team/weight/count state. [doc §5.3]
- `EXACTIFICATION-RESIDUAL` — fixed retail has no conditions `-1`, `2`, or `3`, no comparator `5`, no negative/overflow weight totals, and no gameplay reader for token 14. The active branches/static CRC field remain implemented where specified; pathological nonterminating RNG spans are not introduced as a host hang oracle. [doc §10]
- `UNKNOWN-RISK` resolved — the mechanism is YR-active in a normal stock skirmish; no selector premise relies on a TS-only branch. [doc §§2,4,10]

## Design

### Components

#### Selector rules

Add a `TeamAiSelectorRules` value owned by `GeneralRules` containing:

- optional `[i32; 3]` `team_delays`, `minimum_defensive_teams`, `maximum_defensive_teams`, and `total_team_cap` in Hard/Normal/Easy order;
- `use_min_defense_rule`;
- native-bit `f64` values for success delta, failure delta, and track-record coefficient;
- native-bit `f32` or an exact promoted representation for `AIMinorSuperReadyPercent`, preserving the native float store before eligibility arithmetic.

Parsing must distinguish a missing or incomplete difficulty vector from a complete vector. Existing `read_3int(default)` cannot express the native empty-vector state by itself, so the selector parser uses a narrow optional-three-int reader or explicit key presence plus exact three-token validation. A House with absent/out-of-range required selector vector data is not armed; the loader records a deterministic diagnostic. This avoids indexing nonexistent native data and does not invent a custom-data fallback. Stock retail inputs must prove all values and exact order.

#### House selector state

Each House owns:

- signed ratio, default `100`;
- active latch, default `false`;
- selector timer start and duration as wrapping signed frame values;
- whether the first House-order stagger has been seeded;
- maintained base-defense Team count.

The scenario loader applies per-House `RatioAITriggerTeam`. Scenario state owns `IgnoreGlobalAITriggers`. Action dispatch writes 74/75/76 directly to the target House state, and successful MCV deploy writes the active latch after the entity transition succeeds. House creation/difficulty application arms the timer only when the rules vector exists, using deterministic House registry order rather than an unordered owner key.

#### Dynamic AITrigger state

Split immutable definition data from mutable runtime data without losing registry order. Each installed trigger stores current, minimum, and maximum native-double bits plus signed successes and attempts. Initial current/min/max values come from the verified three source weight tokens. The selector walks `ai_trigger_order`; no map key or weight sort becomes an alternative authority.

`team_ai_selector` owns pure functions for:

- ratio/latch/admission and timer-expiry outcomes;
- cap prepass and eviction-candidate selection;
- complete eligibility using explicit facts;
- x87 truncation, priority tier, wrapping total/cumulative, and unsigned winner comparison;
- primary/secondary output and cancellation;
- adaptive success/failure calculations and clamps;
- native CRC projection in registry order.

It does not read INI, intern strings, query global Simulation state, or issue generic AI commands.

#### Selector facts adapter

The late-House coordinator assembles one immutable `TeamAiSelectorFacts` view for the acting House:

- acting, target, and first-Civilian House facts;
- scenario mode, scenario difficulty, ignore-global flag, and current frame;
- category-and-type authoritative owned counts;
- live Team order with owner, TeamType, signed priority, defense bit, and literal cancellation bytes;
- TeamType Max/Autocreate/zone/TaskForce metadata;
- exact non-bridge base-zone relation results for the TeamType movement rows;
- acting-House owned Buildings in authoritative order with family, power, mission, owner mask, naval, and primary identity;
- acting-House superweapon instances in explicit native-preserving order;
- available-wallet input using the already authoritative economy storage and balance values.

The adapter may call narrow subsystem queries, but it must not widen exact selector predicates into generic `can_reach`, generic “has factory,” aggregate credits, or type-keyed superweapon existence.

#### Empty construction and destruction

Add a selector-only `construct_empty_team_from_type` seam in `TeamScriptVm`. For each output it rechecks live owner+TeamType count against signed `Max`, resolves the TeamType's Script/TaskForce, inserts a memberless Team in live order, initializes literal constructor state including `+0x7B=false`, `+0x7D=true`, `+0x7F=false`, success false, increments the TeamType/live and House-defense authorities, and marks the Team pending recruitment. Pending Teams participate in later counts and cancellation but do not execute ScriptType actions.

Keep the existing immediate-admission seam for already-supported scenario/tests; the selector must never call it. Stage C will unify the member admission path when exact recruitment is implemented. The pending flag is a bounded integration seam, not a claim that native stores an extra flag.

All Team removals route through one `destroy_team` transaction. It first fans success/failure into every trigger whose primary TeamType matches, in trigger registry order, then performs defense/type/member/global removal. Selector eviction calls this same transaction synchronously before eligibility. No direct `teams.remove` remains on a gameplay removal path.

### Interfaces / Contracts

- `select_for_house(facts, rules, rng) -> SelectorOutcome` consumes the ratio draw first and the weighted draw only under the verified conditions. Its outcome carries zero, one, or two ordered TeamType IDs plus an optional eviction Team ID and the timer-reset instruction.
- `TeamScriptVm` provides ordered, immutable trigger/Team projections to the selector and applies the returned mutation transaction in the same House update call. Applying an outcome revalidates each output's `Max` immediately before construction.
- The facts adapter returns exact booleans/counts already resolved by their owning subsystem. The pure selector never reaches back into Simulation and never guesses missing facts.
- Optional selector vectors are a load/runtime admission fact. Missing custom-data vectors disable selector arming with a diagnostic; they do not become zeros or retail defaults.
- The existing generic AI state remains separate. This slice neither consumes its RNG choices nor uses its attack-wave selection as Team output.
- Snapshot format changes bump `SNAPSHOT_VERSION`; older bytes fail cleanly at the envelope. New state is fully serialized.

### Data Flow

1. Rules loading parses the optional vector/scalar selector values. Fixed AIMD/map loading installs ordered trigger definitions and initializes dynamic state.
2. Scenario loading installs ignore-global, House ratio, House order, active-latch defaults, and initial timer state.
3. The existing Team pass runs. Pending selector-created Teams remain inert until Stage C.
4. Logic/object/production consequences update authoritative House counts, economy, power, Building state, zones, and superweapon state.
5. In the late House rung, admitted nonhuman/nonpassive Houses whose timers expire are visited in House order.
6. For each expired House, the coordinator consumes the mandatory ratio draw, applies ratio/latch gates, performs cap/eviction and synchronous feedback, assembles exact eligibility facts, walks triggers in registry order, and conditionally consumes the weighted draw.
7. Cancellation either clears the full output or applies `Autocreate`. Each surviving TeamType gets an immediate separate `Max` recheck and empty construction in primary/secondary order.
8. The timer resets to unstaggered `TeamDelays[difficulty]` even on no output. The late frame commit follows.
9. Any later Team destruction uses the same feedback-first transaction. Snapshot/hash observe the resulting state at their existing boundaries.

### Error Handling

- Missing required retail selector keys fail the retail corpus acceptance test. Missing custom-map/rules vectors remain explicit absence, emit one stable diagnostic, and leave the affected House timer unarmed.
- An unresolved TeamType, ScriptType, or TaskForce at construction returns the existing deterministic refusal shape; fixed-AIMD unresolved references remain installation refusals under the Stage-A contract.
- A missing exact fact required by an active eligibility branch rejects that trigger and records a test/debug diagnostic; it never falls back to a broader approximation.
- Malformed custom negative totals retain the verified signed/wrapping selector arithmetic while host-side tests bound the call. The design does not intentionally emulate the native RNG helper's nontermination for spans at least `0x80000000`.
- All ordinary lifecycle paths use checked identity lookup but native signed comparisons/wrapping increments where behavior requires them.

### Testing Strategy

Focused `--lib` tests are organized by owner:

1. Rules tests prove scalar constructor defaults, retail overrides, Hard/Normal/Easy order, complete optional vectors, and missing/partial-vector absence.
2. Pure selector table tests cover every condition/null-target row, every comparator, family identity, owner/side/mode/difficulty gates, minimum-defense and suppression, primary/secondary zone/factory/Max order, first superweapon/Civilian behavior, wallet rounding, exact 5000 tier, zero-total/no-second-draw, wrapping signed totals, unsigned cumulative comparison, cancellation/no retry, and output order.
3. RNG-spy tests assert the mandatory first draw and conditional second draw, plus registry and House iteration order.
4. Cap tests cover strict equality, signed-priority earliest tie eviction, synchronous adaptive feedback, stock eviction exclusion, and two-output cap overshoot.
5. Construction tests prove separate `Max` rechecks, same-TeamType double creation at Max 2, no synchronous members, next-frame inertness, constructor bytes, counts, and no ScriptType execution while pending.
6. Adaptive tests use exact native-bit inputs for A<=0, positive/negative adjustment clamps, min/max clamps, success/failure counters, fan-out to every primary match, no secondary-only feedback, and feedback-before-removal visibility.
7. Integration tests prove successful MCV deploy and actions 74/75/76 write only the target House fields; first stagger and repeat reset; Team-before-House master-frame order; and generic AI remains operational but separate.
8. Snapshot/hash tests prove round-trip of House timer/latch/ratio, dynamic weights/counters, Team cancellation/pending state, and new order authorities. Hash tests prove current/min/max/static trigger projection participates while counters and Team success do not participate directly.
9. Retail corpus tests assert `165` ordered triggers, verified weight census/bounds, stock selector Rules values, all required vectors present, and evidence-backed absent condition/comparator cases.

Work-in-progress validation uses scoped `cargo test -p vera20k --lib <filter>` only after confirming no other Cargo/rustc owner. The phase-wide full `cargo test -p vera20k --lib` remains reserved for the end of Phase 3, not this slice.

## Architectural Decisions

- **Keep trigger and Team mutation authority together.** `TeamScriptVm` already owns both ordered registries and live Teams, so destruction feedback and cancellation cannot drift between stores.
- **Use a pure selector with an explicit facts boundary.** This follows existing subsystem-function patterns while preventing `world/mod.rs` from becoming the semantic owner of every predicate.
- **Preserve narrow native order authorities.** House, trigger, Team, owned Building, first Civilian, and superweapon instance order are independent facts. A `BTreeMap` key order or ad hoc scan is not substituted.
- **Preserve native numeric domains.** x87 conversion helpers, native float/double bit wrappers, signed wrapping arithmetic, and unsigned comparison are explicit contracts rather than incidental implementation details.
- **Deviate with a temporary pending-recruitment seam.** Native has no Rust-style pending flag, but the exact constructor produces an empty Team and Stage C owns subsequent recruitment/latch transitions. The flag prevents an impossible empty Team from executing a script and is removed or made derived when Stage C closes.
- **Do not replace generic AI yet.** The selector produces native Team state; generic AI currently keeps the incomplete game playable. Removing/reconciling it is a later ownership decision after Team recruitment/action coverage exists.
- **No Phase-3-row closure claim.** This design closes only selector/feedback ownership. Live recruitment, admission, dissolution, and the remaining row mechanisms remain open and separately reviewed.

No unbounded technical debt is accepted. The only intentional bridge is pending recruitment, whose trigger, effect, and removal owner are named.

## Alternatives Considered

### Put the selector directly in `world/mod.rs`

This would make fact access easy, but it couples rules parsing, House cadence, Team mutation, entity scans, superweapon order, RNG, and adaptive arithmetic into the master scheduler. Unit tests would require a whole Simulation, and borrow-driven staging could accidentally change same-call destruction/output order. Rejected for high determinism and maintenance risk.

### Extend `sim::ai::tick_ai`

This reuses the current late-AI call but conflates two mechanisms. Generic AI owns Rust production/attack-wave choices and command application; native selector owns ordered AITriggers and constructs empty Teams directly after the Team pass. Combining them changes RNG, scheduler, lifecycle, and future replacement boundaries. Rejected as architectural and parity drift.

### Implement selector and full live recruitment as one change

This would complete more of the player loop, but recruitment has its own per-slot cadence, category arrays, nearest/tie ordering, class-specific admission helpers, push-front membership, and dissolution lifecycle. Combining it with selector makes reviewer evidence too broad and violates the one-mechanism builder/reviewer cadence in the Phase 3 goal. Rejected for this slice; Stage C remains the exact next Team-production mechanism after selector review closes.

## Adversarial Approval

**Why should this design be approved?** It maps every selector-visible branch to a proven native owner, preserves all compounding order/RNG/numeric/lifecycle state, resolves the last default-value ambiguity directly from `RulesClass` construction and `DifficultyClass::ReadINI_IntVector`, and keeps the Rust-native ownership boundary testable. It introduces no excluded-system work and makes no claim that the larger row is complete.

**What can still make an ordinary skirmish feel wrong after this slice?** Selector-created Teams will not recruit or act until Stage C is implemented, so this slice is not by itself a player-loop milestone. The existing generic AI continues to supply interim opponents. This is recorded as an open milestone-blocking mechanism, not hidden as residual parity.

**What could cause expensive later rework?** Treating key-sorted maps as native Building/superweapon order, scanning objects instead of maintaining per-type lifecycle counts, sharing generic-AI RNG/state, or losing exact trigger numeric state would force redesign. The explicit facts adapter and order authorities prevent those choices. The pending-recruitment flag is deliberately narrow and has a named Stage-C removal path.

**Decision:** approved autonomously under the active Phase 3 goal. Proceed to proportional implementation of this selector slice, then give a fresh reviewer the requirement, native report, diff, and literal focused-test output. Any unverified or approximate selector behavior keeps this mechanism open.
