# Team AI Registry Ingestion Design

## Goal

Load the active YR `AIMD.INI` TeamType, ScriptType, TaskForce, and AITriggerType registries plus native-order map overrides into deterministic Simulation state, without implementing or approximating AI-trigger selection, Team creation, or live recruitment.

## Status and Scope Decision

Approved autonomously under the active Phase 3 goal after adversarial review. The goal explicitly authorizes safe autonomous progress, the active-YR investigation is complete, and the smallest robust prerequisite is discoverable without a user-only choice.

This design owns Stage A of [docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md). It does not close production reachability. The row remains open after implementation because a normal House still needs the exact AITrigger selector, empty Team construction, and incremental live recruitment.

Out of scope: Railgun, LaserDraw, Sonic Wave, destroyable cliffs, TS legacy, complete AITrigger condition semantics, runtime weight feedback, Team creation, TaskForce recruitment, and ScriptType opcode implementation.

## Architecture Context

The app loading path resolves `rulesmd.ini` through `AssetManager`, retains the unmerged map `IniFile` at `MapFile::ini`, constructs a GPU-free `Simulation`, then pre-interns rules type IDs before gameplay. It currently never opens `aimd.ini`.

`rules/` owns INI parsing and immutable game-data definitions and must not depend on `sim/`. `sim/` may depend on `rules/`. `TeamScriptVm` currently owns resolved ScriptType, TaskForce, and TeamType maps together with live Team state; it is serialized with Simulation, while its custom native Team CRC hashes only live Team/Script state. The VM is installed as `TeamScriptVm::default()` and all current definition registrations are test-only.

The active binary establishes a separate data pipeline. `Load_Game_Rules @ 0x0052CD70` opens `AIMD.INI`. `ScenarioClass::Full_Init @ 0x00686B20` processes fixed AIMD and map data per registry, in the order TeamTypes, ScriptTypes, TaskForces, AITriggerTypes, reusing existing identifiers in place and appending new ones. ScriptType and TaskForce readers reset their payloads on re-read; TeamType applies authored fields over its current/default state; AITriggerType replaces its comma record. Registry order is later AITrigger selection authority.

There is no relevant Team/AI system-model synthesis document. The primary source is [docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md), backed by the live addresses above and the retail `ini/aimd.ini` corpus. The research index identifies `src/sim/team_script_vm.rs`, `src/sim/world/mod.rs`, and `src/sim/ai.rs` as the current Rust corridor.

## Impact Analysis

Expected touchpoints:

- `src/rules/team_ai_ini.rs` — new pure parser/overlay owner for the distinct fixed AIMD and map sources.
- `src/rules/mod.rs` — exports the new immutable data module.
- `src/app/loading/init_helpers.rs` or `src/app/loading/init.rs` — opens `aimd.ini` through `AssetManager` and passes the untouched map INI as the later source.
- `src/sim/team_script_vm.rs` — receives resolved definitions, preserves native registry order, stores AITrigger records, and exposes count/order inspection needed by later stages.
- `src/sim/world/mod.rs` — a narrow install method may coordinate interning and VM replacement without exposing two mutable Simulation fields to app code.
- scoped tests in the new parser/VM/loading modules.

Risks and mitigations:

- **Order loss:** BTreeMap key order is not native registry order. Store explicit order vectors and use lookup maps only for identity resolution.
- **Snapshot migration:** bincode encodes structs positionally, so new serialized VM fields cannot safely default from a shorter record. The initial registry payload bumped `SNAPSHOT_VERSION` 98 → 99; retaining resolved ScriptType/TaskForce provenance bumped 99 → 100; retaining the typed AITrigger payload bumped 100 → 101; removing the falsely retained token-4 scalar bumped 101 → 102; retained TeamType zone fields bumped 102 → 103; category-distinct TaskForce member identities bumped 103 → 104; category-distinct AITrigger token-6 identities bumped 104 → 105. Older bytes are rejected cleanly, and v105 saves carry the corrected resolved registry.
- **Hash drift:** native Team CRC excludes static type registries. Keep `TeamScriptVm::hash_state` restricted to live Teams, matching the existing verified contract. Static definitions are analogous to RuleSet: deterministic match inputs, not per-frame CRC payload.
- **Interner drift:** install exactly once, after the existing rule-type pre-intern step and before gameplay. Intern each registry in native source order. Do not use HashMap iteration as an ID source.
- **Wrong source composition:** never merge AIMD into Rules layers. Parse fixed AIMD and map as separate passes.
- **Future-selector rework:** retain lossless raw TeamType authored fields and all 18 AITrigger tokens in addition to currently proven typed fields. Do not discard unknown token semantics.
- **Current test API compatibility:** `register_*` remains available for focused tests but must append order only on first identity and replace in place thereafter.

## Chosen Approach

Add a pure `rules::team_ai_ini` registry loader that produces ordered, unresolved string definitions from two distinct `IniFile` sources. Resolve and install that result into `TeamScriptVm` at the established scenario boundary after Rules IDs are interned.

This follows existing architecture: `rules/` parses data, Simulation owns deterministic resolved state, and app loading coordinates immutable asset inputs. It deliberately deviates from the current test-only “call three registration methods manually” pattern because a production registry has source ordering, overlay, diagnostics, and a fourth AITriggerType registry that individual registrations cannot safely express.

The parser uses ordered `Vec` storage plus uppercase identity-to-index maps. On a later-source duplicate, it re-reads/replaces the record at the existing index. On a new identity, it appends. It separately retains every fixed and map TeamType read transaction in that source's encounter order and with the current-field state at that read. Resolution therefore replays fixed references before map re-reads without deriving map order from the final merged identity vector; changing a TeamType attachment in the map does not erase the earlier placeholder or its registry position. The resolved VM mirrors the order explicitly while retaining keyed lookup for current Team operations.

Before each map pass mutates the live record, the parser also captures an immutable copy of that registry's fixed-AIMD definitions. The RuleSet-aware install boundary resolves this fixed view with a cloned interner, then resolves the final overlaid registry with the production interner. Fixed-origin diagnostics survive even when a same-identity map replacement repairs the final record or a partial TeamType overlay relabels inherited fields as scenario-origin; equivalent final diagnostics are deduplicated. This validation view is transient and does not change the snapshot schema.

### Why this approach is approved

Adversarial question: why not wait and implement the whole producer at once? Because the exact AITrigger condition table and several live recruitment predicates remain open native questions. Combining them would either block a separately proven loader or encourage approximations. The ordered registry is independently exact and is a mandatory dependency of every later exact approach.

Adversarial question: what could still make ordinary skirmish feel wrong? After Stage A, ordinary AI still creates no native Teams. This remains a milestone blocker and is kept visibly open; no completion claim follows this slice.

Adversarial question: what could cause expensive later rework? Throwing away load order, raw AITrigger tokens, or authored TeamType fields would require a registry redesign before the selector. This design stores all three now. Implementing runtime selection or recruitment in the parser would create the opposite problem—hidden coupling—so those remain separate stages.

## Player-Experience Detail Ledger

- `MILESTONE-BLOCKING` — ordinary nonhuman Houses need all four fixed AIMD registries; current production state has none. Stage A supplies data only and leaves the producer row open. [doc: [PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md) §§1, 8]
- `COMPOUNDING` — registry source order becomes weighted-selection order and exact-tie authority later. Preserve fixed order, existing-ID position, and map append order. [GHIDRA `0x00686B20`, `0x006F0AB0`]
- `COMPOUNDING` — `AIMD.INI` is a distinct fixed root. Folding it into Rules layers changes both precedence and per-registry re-read behavior. [GHIDRA `0x0052CD70`, `0x0068797A..0x006879E3`]
- `COMPOUNDING` — map duplicates update the existing identity without moving it; new map identities append. [GHIDRA registry loaders `0x006F19B0`, `0x00691970`, `0x006E8220`, `0x0041F2E0`]
- `MILESTONE-BLOCKING` — ScriptType reads keys `0..49` and compacts gaps. A sparse map script cannot preserve numeric holes. [GHIDRA `0x006918A0`]
- `MILESTONE-BLOCKING` — TaskForce reads at most six rows, stores signed count before exact resolved type, and does not count unresolved types. [GHIDRA `0x006E8420`]
- `COMPOUNDING` — TeamType defaults and later map reads use current-field semantics; resetting a duplicate TeamType to constructor defaults would erase fixed AIMD values that the map omitted. [GHIDRA `0x006F06E0`, `0x006F1090`]
- `COMPOUNDING` — TeamType Script/TaskForce lookup finds or allocates every valid requested identity immediately. The fixed and map TeamType lists are each replayed in their own encounter order; this is distinct from the final identity vector when a map lists a new TeamType before overriding a fixed identity. Later ScriptTypes/TaskForces passes fill those placeholders in place, so first-reference order owns the registry prefix; a still-unlisted valid identity remains an empty placeholder. Case-insensitive `none`/`<none>` returns no identity, after which the outer TeamType reader attaches registry entry zero without refusal when available and fails only while the registry is empty. [GHIDRA `0x006F19B0`, `0x006F1090`, `0x006F14A3 -> 0x00691C00`, `0x006F14DC -> 0x006E85F0`, `0x007C8D20`]
- `COMPOUNDING` — AITriggerType is keyed by record ID, not listed by numeric values; all 165 stock records have 18 tokens. Token 12 and semantic labels for 11–14 remain unknown, so all tokens must survive losslessly. `[AITriggerTypesEnable]` false disables only in game mode zero; every listed key enables in skirmish/MP. [GHIDRA `0x0041F2E0`, raw body `0x0041F580`; ini: `aimd.ini [AITriggerTypes]`]
- `MILESTONE-BLOCKING` — AITrigger token 4 is required but discarded; native initializes `+0xB0` to zero, then keeps the larger primary/secondary TaskForce result from the ordered `0x006E8780` member-TechLevel fold. Member counts are irrelevant; in nonzero game modes a `TechLevel=-1` slot writes `11` even after a higher accumulator, so slot order is load-bearing. [GHIDRA `0x0041F712..0x0041F728`, `0x0041FA5C..0x0041FADD`, `0x006E8780`]
- `COMPOUNDING` — the three difficulty weights are native parsed scalars, but Stage A does not consume them for RNG. Preserve their source token text and proven typed values without drawing RNG. [GHIDRA `0x0041F580`, `0x006F0AB0`]
- `MILESTONE-BLOCKING` — 132 TaskForces, 88 ScriptTypes, 163 TeamTypes, 165 AITriggerTypes, 12 base-defense TeamTypes, and the documented priority distribution are the retail corpus acceptance oracle. [ini: `aimd.ini`; doc §7]
- `RESOLVED` — all 66 retail TaskForce member identities resolve through Infantry/Unit/Aircraft registries and explicitly author `TechLevel`. The Stage-A critic exactification also closes custom data: Rust preserves category-distinct type registries, resolves TaskForce members in native Infantry/Unit/Aircraft order without BuildingType admission, retains the selected family plus ID in each compact entry and snapshot, and keeps the native missing-key `TechLevel=255` constructor value distinct from explicit `-1`. Both mode-zero and nonzero threshold folds are covered. [GHIDRA `0x004C4EF0`, `0x00711082`, `0x00714570..0x00714584`, `0x006E8780`; retail corpus census]
- `RESOLVED` — AITrigger token 6 resolves in native Infantry/Unit/Aircraft/Building order and retains the selected family plus ID through deterministic registry serialization and snapshots; an ambiguous name-only lookup is insufficient for custom rules with duplicate cross-family identities. [GHIDRA `0x0041F77D..0x0041F7E4`]
- `EXACTIFICATION-RESIDUAL` — malformed/custom-map records outside the proven stock shapes may have native failure details not yet closed. Deterministically reject with diagnostics; do not synthesize data. Trigger: malformed custom AI data. Player effect: that authored team/trigger is absent. Frequency: zero in fixed retail corpus. Downstream risk: bounded if lossless diagnostics preserve the source. [UNKNOWN-RISK reduced by stock census]
- `MILESTONE-BLOCKING` — Stage A must not create a Team, consume scenario RNG, issue commands, or alter House timers. Any such side effect would move unverified Stage B/C behavior into load. [doc §§5–6, 9]
- `COMPOUNDING` — static registry state must survive save/load so future live Team and selector state keeps valid identities, while the verified Team CRC remains live-state-only. [doc §10; `TeamClass::ComputeCRC @ 0x006EC5A0`]
- `MILESTONE-BLOCKING` — active scenario is ordinary standalone YR skirmish; TS and campaign forced-team creators are excluded. [GHIDRA `0x004F8440`; doc §§1, 10]

## Design

### Components

#### 1. `rules::team_ai_ini`

Owns immutable, unresolved definitions and native overlay policy:

- `TeamAiIniRegistry`
- ordered `ScriptTypeIni`, `TaskForceIni`, `TeamTypeIni`, and `AiTriggerTypeIni` vectors
- uppercase identity indexes used only to find an existing vector slot
- deterministic `TeamAiLoadDiagnostic` values for malformed or unresolved records

The public constructor accepts `fixed: &IniFile`, `map: &IniFile`, and the scenario's `game_mode_nonzero` fact. Internally it executes the native per-registry sequence rather than merging the two files. The mode fact is consumed only by `[AITriggerTypesEnable]`: false disables a listed trigger only in game mode zero.

ScriptType stores compact signed action pairs. TaskForce stores at most six signed-count/type-name rows and `Group`. TeamType stores proven typed corridor fields plus an ordered authored-field payload so unmapped fields remain lossless across the fixed/map overlay. AITriggerType stores the exact key, all 18 trimmed tokens, source, and proven parsed references/scalars; a non-18-token record is diagnosed and omitted rather than padded.

#### 2. `TeamScriptVm` resolved registry

Extend the VM with explicit ordered identities for ScriptType, TaskForce, TeamType, and AITriggerType plus resolved definitions keyed by `InternedId`. Registration/install rules are:

- first identity appends to the relevant order vector;
- duplicate identity replaces the keyed definition without moving the order entry;
- a full production install starts from empty definition registries and leaves live Teams empty;
- Script/TaskForce/TeamType identities and referenced TechnoTypes are interned in deterministic registry/record order;
- TaskForce entries whose TechnoType is absent from RuleSet are omitted and diagnosed;
- TeamType Script/TaskForce references find or allocate placeholders during the TeamType pass; later ScriptTypes/TaskForces records fill those same keyed definitions without changing first-reference order.

The current live-Team BTreeMap and creation-order ID allocation remain unchanged in Stage A.

#### 3. Scenario load integration

Add a helper paralleling the retail Rules root loader:

```text
AssetManager.get_with_source("aimd.ini")
    -> IniFile::from_bytes
    -> TeamAiIniRegistry::from_sources(fixed, &map_data.ini)
```

Build this immutable registry during map load. After `Simulation` construction and the existing `intern_rule_type_ids`/`resolve_type_handles` step, install it into the VM before the scenario is handed to runtime. Presentation code never reads or mutates it.

Missing/unparseable fixed AIMD fails the normal verified app map load with context, because silently starting a stock skirmish with no AI registry is a common-path gameplay failure. Test-only `Simulation::new()` remains empty and does not need assets.

### Interfaces / Contracts

- `TeamAiIniRegistry::from_sources(fixed, map) -> TeamAiIniLoad`
- `Simulation::install_team_ai_registry(load, rules) -> Vec<TeamAiLoadDiagnostic>` or an equivalent narrow coordinator
- read-only VM methods for registry counts/order and definition lookup, scoped `pub(crate)` unless tests require public linkage
- existing `register_*` methods continue for focused tests but preserve first-registration order

No interface in Stage A may create Teams, tick House AI, inspect live entities, issue commands, or touch scenario RNG.

### Data Flow

```text
AssetManager AIMD bytes ──> fixed IniFile ──┐
                                            ├─> ordered unresolved TeamAiIniRegistry
MapFile::ini ───────────────────────────────┘
                                                     │
RuleSet + Simulation interner ───────────────────────┤ resolve once
                                                     v
                                 TeamScriptVm ordered definitions
                                                     │
                                      snapshot/restore with Simulation
```

The future House selector will read the resolved AITrigger order; it will not parse INI or reconstruct source order.

### Error Handling

- fixed `aimd.ini` absent, empty in any of the four required registries, or containing a refused definition: fail the normal app scenario load with a descriptive error before installing the registry;
- a missing or malformed scenario definition: omit the refused map addition and retain a source-tagged diagnostic without invalidating the clean fixed registry;
- malformed ScriptType action: omit that action exactly where native parse proof supports omission and diagnose it;
- unresolved fixed-AIMD TaskForce TechnoType or AITrigger reference: retain a source-tagged install diagnostic and abort production installation rather than admitting a partial fixed registry; a valid unlisted TeamType Script/TaskForce identity is instead a native empty placeholder and is not a refusal;
- the same unresolved scenario reference: omit the unresolved TaskForce member/AITrigger reference as appropriate, retain a source-tagged diagnostic, and keep the clean fixed registry plus valid map overlays installed; a `none`/`<none>` TeamType attachment fails only while its target registry is empty, while every valid unlisted identity remains an undiagnosed placeholder;
- malformed AITrigger token count: omit and diagnose;
- duplicate map identity: update in place; never warn as a duplicate because this is authored override behavior.

Diagnostics are deterministic data returned/logged at load, not sim-tick side effects.

### Testing Strategy

Focused `--lib` tests only during this slice:

1. parser unit tests for ScriptType compaction/50 cap, TaskForce six cap and unresolved omission, TeamType current-field override, AITrigger 18-token enforcement, duplicate in-place update, and new-ID append;
2. VM install tests for first-reference placeholder ordering, a map TeamType order that conflicts with final merged identity order and the later Script/TaskForce lists, later in-place fill, retained unfilled placeholders, no live Team creation, and serde round trip;
3. stock-corpus oracle test asserting `132/88/163/165`, 12 base-defense TeamTypes, 163 `Autocreate=yes`, exact priority distribution, and every resolved AITrigger threshold;
4. loading-boundary tests for the required active-YR AIMD sections plus Simulation installation without direct `register_*` calls;
5. re-run the existing focused `team_script_vm`, `gsi_04_05_`, snapshot, and base-defense response tests to prove earlier fixes remain green.

Before every Cargo command, check `cargo`/`rustc`; every command carries `--lib`. Do not run the full lib suite for this prerequisite slice.

## Determinism and Persistence

All native registry order is represented by vectors. Identity lookup maps must be BTreeMap or otherwise excluded from iteration authority. The parser consumes source order only from `IniSection::keys/get_values`, never HashMap entry order. Installation happens before the first gameplay tick and consumes no RNG.

Resolved definitions and order vectors serialize with `TeamScriptVm`. Because bincode cannot safely default a changed positional struct, the initial registry payload bumped `SNAPSHOT_VERSION` 98 → 99, resolved ScriptType/TaskForce provenance bumped 99 → 100, the typed AITrigger payload bumped 100 → 101, removal of the non-native token-4 scalar bumped 101 → 102, retained TeamType zone fields bumped 102 → 103, category-distinct TaskForce member identities bumped 103 → 104, and category-distinct AITrigger token-6 identities bumped 104 → 105; older bytes are rejected at the envelope boundary. The custom Team CRC remains unchanged because the verified native CRC includes live Team/Script instance state, not the source registry. Tests must prove save/restore retains counts, order, IDs, and definitions.

## Architectural Decisions

- Keep AIMD separate from RuleSet layering because native file authority and per-registry override semantics differ.
- Put parsing in `rules/`, not the already oversized runtime VM module.
- Keep resolved registries with `TeamScriptVm` for now because existing live Team state already references those identities and serialization owns them there.
- Preserve raw unknown payload alongside proven typed fields to prevent later selector/recruitment research from forcing a loader rewrite.
- Do not hash static definitions in the per-frame Team CRC; validate input equality at scenario compatibility boundaries as RuleSet already does.
- Do not promote the existing generic attack-wave AI into this registry path.

No intentional parity drift is introduced. Malformed custom AI data remains a named exactification residual with deterministic refusal; fixed retail corpus is fully in scope.

## Alternatives Considered

### Parse directly inside `TeamScriptVm`

This would reduce the number of types, but it makes the simulation runtime own Asset/INI concerns, grows an already oversized module, and couples later House selection to loading. It also makes headless/data tests harder. Rejected as an architectural inconsistency.

### Merge AIMD into the composed Rules INI / RuleSet

This reuses existing rules layering, but it is behaviorally wrong: native uses a distinct fixed root and executes fixed/map passes per AI registry. A single merge loses re-read semantics and registry order. Rejected as parity drift.

### Implement loader, selector, and recruitment as one vertical feature

This would produce player-visible Teams sooner, but the selector condition table and several recruitment predicates are still open evidence. It would either stall a closed loader or force approximations into common AI behavior. Rejected for this slice; Stages B–D remain mandatory follow-up under the Phase 3 row.
