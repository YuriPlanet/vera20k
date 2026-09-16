# MapGen Same-Process Lifecycle and Bridge Caller Reconciliation

**Date:** 2026-07-13  
**Target:** Yuri's Revenge `gamemd.exe`  
**SHA-256:** `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`  
**Ghidra program:** `gamemd.exe`, image base `0x00400000`, x86 little-endian 32-bit  
**Investigation mode:** read-only Ghidra plus local source/INI inspection  
**Overall status:** **PARTIAL — static reconciliation complete; same-process C0/C1/C2 runtime proof blocked by the no-launch constraint**  
**Active in standard YR:** yes for CRT initialization, bridge repair, and bridge destruction; conditional for random-map generation (`.SED`); one unreferenced RMG-shaped function remains statically unreachable/UNCHECKED

## 1. Scope and verdict

This report closes Track C's static work. It revalidates the writer/reset census for `g_MapGenRng @ 0x00ABE890`, classifies every direct caller of the MapGen ranged helper `0x00598030`, and audits the RNG receivers in representative bridge-destruction bodies. It does not launch `gamemd.exe`, modify Ghidra, change Rust, or claim same-process retention from static evidence alone.

The corrected bridge verdict is:

> **Active match bridge repair consumes MapGen RNG. Active match bridge destruction consumes Scenario RNG, not MapGen RNG. The other statically reachable direct callers of `0x00598030` belong to conditional random-map construction.**

Therefore the older phrase “bridge repair/destruction actively consumes MapGen” is wrong. The precise replacement is “bridge repair plus conditional RMG map construction consumes MapGen; audited destruction walkers and `CellClass::BlowUpBridge` consume Scenario RNG.” A damaged or destroyed bridge can later cause a MapGen draw when an engineer/CABHUT repair restores a random healthy overlay variant, but that later repair draw is not a destruction draw.

The lifecycle verdict is narrower:

- ordinary PE startup statically reaches raw CRT thunk `0x0058B770`, which executes `Random__Seed(0)` on `g_MapGenRng`;
- RMG entry `0x00598960` later replaces the complete `0x3F4`-byte object with a 253-dword copy of a seeded stack object;
- `Random__Next` calls mutate the object during RMG and bridge repair;
- no separate match-teardown or fixed-map-reentry reset/replacement was found in the static xref/caller census;
- **retention across two matches in one process remains UNCHECKED until runtime cases C0/C1/C2 capture the full object.**

This is not parity certification. Static absence of an identified write cannot exclude a missed indirect/bulk write, and the approved task explicitly forbade the runtime launch needed to decide that question.

## 2. Evidence method and confidence rules

The research index was used only to locate prior claims. Every load-bearing binary claim below was cold-rechecked against the active Ghidra program with read-only `get_function_callers`, `get_xrefs_to`, `get_bulk_xrefs`, `get_function_by_address`, `decompile_function`, `disassemble_function`, and `read_memory` calls. Local labels are treated as navigation hints; role and liveness come from body, receiver, callsite, and caller-chain evidence.

Confidence labels used here:

- **High:** exact instructions plus direct caller/callee chain establish the mechanism and active/conditional entry.
- **Medium:** body and static xrefs establish the best current classification, but an owner boundary or indirect reachability remains unresolved.
- **Blocked runtime:** the mechanism is statically bounded, but process-lifetime behavior requires observation in one owned process.

## 3. `Random__Seed` object boundary and fresh MapGen construction

### 3.1 Exact object extent

Fresh `disassemble_function(0x0065C6D0)` establishes:

- `+0x04 = 0` at `0x0065C6DA`;
- `+0x08 = 0x67` at `0x0065C6DD`;
- the state destination begins at `this+0x0C` at `0x0065C6E5`;
- the loop counter is `0xFA` (250) at `0x0065C6EE`, with one dword stored per iteration at `0x0065C755`;
- byte `+0x00` is cleared only after the table fill at `0x0065C770`;
- padding bytes `+0x01..+0x03` are not written.

The last state dword is therefore at `+0x0C + 249*4 = +0x3F0`, occupying bytes through `+0x3F3`. The complete logical/copy object is `0x3F4` bytes (`253` dwords), with layout:

| Range | Size | Meaning |
|---|---:|---|
| `+0x000` | 1 | disabled/locked byte, cleared by `Seed` |
| `+0x001..+0x003` | 3 | untouched padding |
| `+0x004..+0x007` | 4 | first index, seeded to 0 |
| `+0x008..+0x00B` | 4 | second index, seeded to 103 |
| `+0x00C..+0x3F3` | 1000 | 250 generated state dwords |

The older `[0x0C..0x3F7]` range in `RMG_RNG_SEED_MAPGENRNG_GHIDRA_REPORT.md` is four bytes too long. Its correct state range is `[0x0C..0x3F3]`.

### 3.2 Direct seed caller census

Fresh direct callers of `Random__Seed @ 0x0065C6D0` are:

| Caller | Receiver/use | Active in YR | Evidence |
|---|---|---|---|
| `0x00598960` | stack temporary later copied to MapGen | Conditional RMG | `get_function_callers(0x0065C6D0)`; decompile/disassembly at `0x00598960` |
| `0x00683560` | receiver object seed/reset | Yes, Scenario lifecycle | caller census and decompile |
| `0x0052FC20` | two stack temporaries copied to Scenario/Main | Yes, random-number initialization | caller census and decompile |
| `0x006832C0` | Scenario constructor receiver | Yes | caller census and decompile |

Raw CRT thunk `0x0058B770` is not in this function-level caller list because Ghidra has no function boundary there. Direct disassembly nevertheless proves the call:

```text
0058B770  PUSH 0
0058B772  MOV  ECX,0x00ABE890
0058B777  CALL 0x0065C6D0
0058B77C  RET
```

`get_function_by_address(0x0058B770)` returns no function. `get_xrefs_to(0x0058B770)` returns only the initializer-table data reference at `0x00813B68`, and `read_memory(0x00813B68,4)` is `70 B7 58 00`. The CRT iterator `0x007CBED3` walks an ascending dword pointer range and calls non-null entries; startup owner `0x007CBDAF` supplies `[0x00812000,0x00815DA4)`, which contains `0x00813B68`. The PE entry at `0x007CD80F` calls `0x007CBDAF` before `WinMain`, and `get_xrefs_to(0x007CBDAF)` finds that PE-entry call. This is High-confidence evidence that the thunk is active once on the ordinary PE-entry startup route. It is not a runtime count proof for exotic reentry, but no match-loop caller exists.

Because the MapGen global is initially zero-filled, the untouched padding remains zero on fresh startup. The logical RNG is nevertheless **not** an all-zero RNG: `Seed(0)` generates its 250-dword table and initializes indices `0/103`.

## 4. RMG replacement writer and process-lifetime census

### 4.1 RMG writer `0x00598960`

Fresh decompile/disassembly establishes the exact replacement sequence:

- read seed from `MapSeed+0x74`;
- call `Random__Seed` on a stack temporary at `0x00598985`;
- load `EDI=0x00ABE890` at `0x00598996`;
- set `ECX=0xFD` and execute `REP MOVSD` at `0x0059899B`.

All `0x3F4` native bytes are replaced. Because `Random__Seed` clears byte 0 but does not initialize bytes `+1..+3`, the RMG copy transfers stack-derived padding into the global. Native/oracle evidence must capture and compare those bytes rather than silently normalize them. Whether Rust must store, serialize, hash, or otherwise preserve the raw padding is **UNKNOWN** until an active-path consumption/xref or controlled perturbation proves that the bytes affect native behavior.

`get_function_callers(0x00598960)`/xref inspection finds one scenario-load call (`ScenarioClass__Read_Scenario @ 0x00684620`, callsite `0x00684989`) and three calls inside random-map setup owner `0x00596300` (`0x0059664C`, `0x00596A49`, `0x00596A66`). Fresh decompile of `0x00684620` separates ordinary INI loading from the `.SED` branch and calls the RMG writer only after the random-map branch succeeds. Fresh decompile of `0x00596300` shows preview/generate/accept paths can invoke the writer before a final match is accepted. Thus “RMG writer only at match launch” is too narrow: an RMG setup preview can replace process-global MapGen earlier.

### 4.2 Global xref census

Fresh `get_xrefs_to(0x00ABE890, limit=500)` returns 141 xrefs. Read-only instruction classification gives:

- 137 receiver loads that reach `Random__Next @ 0x0065C780` (including four sites where the call is farther than the immediately adjacent instructions);
- two xrefs for the one RMG replacement operation (`EDI` destination load at `0x00598996`, classified data, plus `REP MOVSD @ 0x0059899B`, classified write);
- one receiver load for the CRT `Seed(0)` thunk at `0x0058B772`;
- one apparent end-sentinel use at raw `0x005AC192`.

The last item is a negative fact, not a MapGen teardown. Disassembly from `0x005AC192` loads `ESI=0x00ABE890`, then accesses `[ESI-0x0C]` and decrements by `0x0C` in a loop that clears 0x6D adjacent 12-byte objects below MapGen. Its first write is below the MapGen base (`0x00ABE884`); it does not write `0x00ABE890..0x00ABEC83`.

`get_bulk_xrefs` over dword addresses in the full `[0x00ABE890,0x00ABEC84)` range yields no other static replacement writer. Ghidra naturally does not create a distinct global xref for every indirect indexed state-table mutation inside `Random__Next`; those are already classified as ordinary stream advances through the receiver loads and `Random__Next` body.

Static census conclusion: the identified whole-object/reset mechanisms are CRT `Seed(0)` and RMG's `0xFD`-dword copy. Ordinary calls to `Random__Next` mutate the existing object. No identified fixed-map load, match teardown, or shell reentry path constructs a third reset. This is strong static evidence but not an exhaustive process-lifetime proof.

## 5. Exact `0x00598030` mechanism and complete direct-caller census

Fresh `disassemble_function(0x00598030)` proves this calling contract:

- input minimum in `ECX`, maximum in `EDX`;
- compute signed width `max-min+1`, but use unsigned raw RNG conversion;
- hardcode `ECX=0x00ABE890` at `0x0059805E` and call `Random__Next @ 0x0065C780` at `0x00598063`;
- scale by `2^-32`, multiply by the inclusive width, add the minimum, and convert with `0x007C5F00`;
- retry only while the converted result is unsigned-above maximum (`CMP`/`JA` at `0x00598087..0x00598089`).

The caller does not supply an RNG object. Every successful helper invocation consumes at least one MapGen raw draw and may consume more only on the rejection path.

Fresh `get_function_callers(0x00598030)` returns exactly ten functions. `get_bulk_xrefs(0x00598030)` returns eleven direct CALL instructions because `0x005A1350` contains two sites:

`0x005A173F`, `0x005A175E`, `0x005A3F92`, `0x005A50C0`, `0x005A972D`, `0x00579630`, `0x0057AD04`, `0x0057F8CF`, `0x0057FDEA`, `0x00580306`, `0x00580831`.

### 5.1 All ten direct callers

| # | Caller | Exact draw gate/range | Verified role and caller chain | Active in standard YR |
|---:|---|---|---|---|
| 1 | `0x005A1350` | two sites; `0..1` or `1..2` depending on bridge-tile case | RMG bridge-tile index remapper. Its only entry caller is `0x005A17F0`; `0x00598960` calls that owner in RMG bridge stages for map types 3/4. Body/disassembly: `0x005A172C..0x005A175E`; xrefs to `0x005A1350`, `0x005A17F0`. | **Conditional RMG** |
| 2 | `0x005A3AE0` | `0..(linear_width²-1)` at `0x005A3F83..0x005A3F92` | RMG terrain/LAT/rock generation; random scratch-cell candidate. Directly called by `0x00598960 @ 0x00599232`, under the decoded configuration/theater branch. | **Conditional RMG** |
| 3 | `0x005A5020` | `0..(linear_width²-1)` at `0x005A50B1..0x005A50C0` | RMG-shaped resource/terrain cluster generator; retries scratch candidates and builds a cluster. `get_xrefs_to(0x005A5020)` and `get_function_callers` find no code/data caller, while the normal “Creating tiberium” route uses `0x005A23A0`. | **UNCHECKED / statically unreferenced**; do not call it active or TS legacy without further evidence |
| 4 | `0x005A95B0` | random scratch cell `0..(linear_width²-1)` at `0x005A971E..0x005A972D` | RMG tech-building placement. Only caller is `0x00598960 @ 0x00598ED5`; body gates placement on RMG configuration fields, including a separate region path that returns without this direct helper. | **Conditional RMG** |
| 5 | `0x0057F6A0` | `0..3` only for overlay `0x4E..0x52` or `0x64`; result `+0x4A` | NS low repair walker. Only called by low repair dispatcher `0x0057F200`, reached by engineer/CABHUT repair owner `0x00570050`. | **Yes, conditional repair** |
| 6 | `0x0057FBC0` | `0..3` only for `0x57..0x5B` or `0x65`; result `+0x53` | EW low repair walker; same low repair dispatcher/caller chain. | **Yes, conditional repair** |
| 7 | `0x005800D0` | `0..3` only for `0xD1..0xD5` or `0xE7`; result `+0xCD` | NS high repair walker. Only called by high repair dispatcher `0x0057F440`, reached by high engineer/CABHUT repair owner `0x00573540`. | **Yes, conditional repair** |
| 8 | `0x00580600` | `0..3` only for `0xDA..0xDE` or `0xE8`; result `+0xD6` | EW high repair walker; same high repair dispatcher/caller chain. | **Yes, conditional repair** |
| 9 | `0x0057ACF0` | unconditional `0..5` at entry (`0x0057ACF9..0x0057AD04`) before later surface-mask early returns | RMG low bridge surface/tile selector, not gameplay destruction. Called twice by RMG map-cell construction owner `0x0057A0C0` and at raw sites `0x00585858`/`0x005858B1`; the latter sit in missed-boundary code with the same cell-iterator/type-1/type-2 shape. Callers of `0x0057A0C0` are RMG construction functions (`0x0059A6C0`, `0x0059BBC0`, `0x0059C920`, `0x0059D510`, `0x0059E740`). | **Conditional RMG**; exact owner of two raw callsites remains Medium-confidence because the Ghidra boundary is absent |
| 10 | `0x00579620` | one unconditional `0..5` roll at entry (`0x00579629..0x00579630`), reused by all `%3` and `&1` branches | RMG low-bridge destroyed-looking tile selector/stamper, not a live destruction event. Only caller is map-cell owner `0x00578E60`, which calls it for clear cells; callers of that owner are `0x00598960` and RMG helper `0x005A1E10`. | **Conditional RMG** |

The four repair bodies were cold-read in full. Their deterministic overlay transitions do not call the helper; only the healthy-variant restoration cases in the table above do. A repair can therefore make zero, one, or multiple MapGen draws depending on the visited overlay states and rejection behavior. “Every bridge repair is one MapGen draw” would be false.

The apparent destruction implications came from stale names. Fresh body/caller reads establish that local labels `ProcessBridgeDestruction_Low @ 0x00570050` and its high twin `0x00573540` are engineer/CABHUT repair owners: they perform the 5x5 neighborhood scan and invoke `RepairBridge_Low/High`. `InfantryClass__PerCellProcess` reaches the low owner, and stock/YR INI data keeps the repair hut live (`rulesmd.ini [CABHUT] BridgeRepairHut=yes`, with base fallback). The names must not be used as role evidence.

## 6. Destruction-side receiver audit

### 6.1 Destruction walker `0x00575BA0`

Fresh full disassembly finds four `Random__RandomRanged @ 0x0065C7E0` sites in each eligible walker-animation iteration: two position jitter draws, delay `1..5`, and explosion-slot `0..count-1`. Before every call, the receiver is loaded from `[0x00A8B230]+0x218`, i.e. the Scenario-owned RNG (`0x00575D25..0x00575D30`, `0x00575D5E..0x00575D6B`, `0x00575DB4..0x00575DBE`, `0x00575DC9..0x00575DD6`). There is no call to `0x00598030` and no `0x00ABE890` receiver.

The allocation-sensitive count is two Scenario draws when the eligible animation allocation fails after jitter, and four when it succeeds and therefore also selects delay and explosion slot. A center/destroyed-anchor exclusion can skip the animation and its draws. The commonly cited full high-walker fixture consumes 48 draws only under its stated geometry and successful allocations; it is not an unconditional per-collapse constant. Evidence: `disassemble_function(0x00575BA0)` and the independently checked ordering in [BRIDGE_COLLAPSE_VISUAL_RNG_ORDER_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/BRIDGE_COLLAPSE_VISUAL_RNG_ORDER_TRACE.md).

**Active in standard YR:** yes when a destroyable bridge collapse reaches this high NS walker; conditional on geometry, animation list, and allocation.

### 6.2 Per-cell fallout `CellClass__BlowUpBridge @ 0x0047DD70`

Fresh full disassembly establishes seven possible `Random__RandomRanged` callsites. Every site explicitly loads the Scenario receiver from `[0x00A8B230]+0x218`: outer 95% gate, X jitter, Y jitter, metallic 50% coin, metallic slot, explosion delay, and explosion slot. No helper call, Main receiver, or MapGen receiver exists in this body.

The exact allocation/branch count per eligible cell is:

| Condition | Scenario draws |
|---|---:|
| map editor, or empty `BridgeExplosions` list | 0 |
| outer gate fails | 1 |
| outer gate passes | 4 base: gate + two jitter + metallic coin |
| metallic coin passes and metallic allocation succeeds | +1 slot draw |
| bridge-explosion allocation succeeds | +2 delay/slot draws |

Thus a passed gate can consume 4, 5, 6, or 7 draws depending on allocations and the metallic branch. Stock YR lists are nonempty (`rulesmd.ini` defines `MetallicDebris` and `BridgeExplosions`) and `DestroyableBridges=yes`; modded empty-list behavior remains governed by the explicit native gates.

**Active in standard YR:** yes for eligible destruction cells outside the map editor.

### 6.3 Reconciled bridge statement

There is no direct path from either audited destruction body to `0x00598030`. The two low selector callers `0x0057ACF0` and `0x00579620` are RMG map-construction selectors, not destruction callbacks. All active-match `0x00598030` callers in the ten-function census are the four repair walkers. Therefore:

- destruction state transitions themselves do not take an RNG argument in current Rust `BridgeState::destroy_bridge_high/low` and their walkers, consistent with the audited native transition bodies;
- destruction presentation/fallout uses Scenario RNG in native and current Rust;
- repair variant restoration uses MapGen RNG in native and current Rust routing;
- current Rust still begins that MapGen stream from the wrong state/lifetime.

## 7. Current Rust comparison

This is a read-only comparison, not an implementation plan approval.

| Rust surface | Current behavior | Native implication/verdict |
|---|---|---|
| `src/sim/rng.rs:16-49` | `SimRng` models disabled byte, indices, and 250 words, but `zeroed()` creates all-zero indices/table | **DRIFT:** fresh native MapGen is `Seed(0)`, indices 0/103 with generated state |
| `src/sim/world/mod.rs:286-321` | `Simulation` owns Scenario/Main/MapGen; comments say fixed MapGen is never seeded and yields zero forever | **DRIFT/stale wording:** CRT actively calls `Seed(0)` |
| `src/sim/world/mod.rs:531-543` | every `Simulation::construct` installs `SimRng::zeroed()` | **DRIFT:** wrong fresh state, and construction is per simulation rather than proven process-lifetime ownership |
| `src/sim/world/mod.rs:643-652` | `reseed_both` also resets MapGen to zeroed | **DRIFT:** no native per-match MapGen zero reset was found; exact later-match policy awaits C0/C1/C2 |
| `src/app_init_helpers.rs:441` and `src/app_transitions.rs:84-93` | a new Simulation is built for map load and replaces `state.simulation` | likely lifecycle mismatch if native retention is confirmed; **UNCHECKED** until runtime proof |
| `src/sim/world/world_orders.rs:380-389`; `src/sim/bridge_state/walker.rs:65-154` | engineer repair passes `mapgen_rng`; transition gates select random healthy variants | routing/gates agree with the static native census; comments that fixed state forces variant 0 are stale |
| `src/sim/bridge_state/walker.rs:449-495,839-844,1214-1219` | destruction transition walkers take no RNG | agrees with the audited transition mechanism; presentation/fallout is separate |
| `src/sim/world/bridge_orchestrator.rs:186-197,1187-1263,1436-1438` | destruction presentation/debris is routed to Scenario RNG | receiver routing agrees with `0x00575BA0`/`0x0047DD70`; allocation/list edge behavior still requires its own parity tests |
| `src/sim/world/world_hash.rs:69-77` | all three RNG streams are hashed | good downstream visibility; does not correct wrong construction/lifetime |
| `src/sim/snapshot.rs:119-163` | complete `Simulation`, including RNGs, is serialized/deserialized | retains Rust MapGen within a saved Simulation; native save/load ownership was not investigated here |

Rust has no current RMG generator path that can reproduce the `0x00598960` whole-object replacement. No later-match ownership change should be implemented from static inference alone. The immediately proved correction is fresh construction semantics (`Seed(0)`, including object fields); process-global retention policy remains gated on C0/C1/C2.

## 8. Runtime evidence contract — blocked, not waived

The following cases require an owned, safely re-enterable `gamemd.exe` process. This worker was explicitly prohibited from launching it, so all three are **BLOCKED RUNTIME**. Header indices or a state hash alone are insufficient; each checkpoint must record all `0x3F4` bytes and a SHA-256, including padding.

### C0 — fixed map, no MapGen draw

Use one owned process and a fixed `DeepFrze` match with no repair/RMG draw. Capture full MapGen:

1. immediately after CRT MapGen thunk return `0x0058B77C`;
2. at Match 1 accepted-session boundary;
3. at Match 1 first pre-tick/L0 observation;
4. immediately before/after the identified shell teardown/reentry boundary;
5. at Match 2 accepted-session boundary;
6. at Match 2 L0.

Acceptance: every capture equals the exact `Seed(0)` object byte-for-byte, with zero fresh padding, and no hidden reset event is observed. This is the control needed before interpreting C1/C2.

Proposed executable test name: `oracle_mapgen_c0_fixed_no_draw_same_process_retains_full_object`.

### C1 — fixed map, one verified repair helper invocation

Construct or select a fixed-map fixture with exactly one repair walker healthy-variant case from section 5.1 and no concurrent MapGen/RMG activity. Capture full MapGen immediately before and after the helper, verify the one helper invocation (and raw draw count, including rejection), then capture Match 2 accepted/L0 in the same process.

Acceptance: post-repair object is the exact native `Random__Next` successor; Match 2 either equals it (retention) or a separately observed writer/reset output (reset). The test must not infer one draw only from wrapped indices and must preserve/check padding.

Proposed executable test name: `oracle_mapgen_c1_bridge_repair_advance_survives_same_process_fixed_reentry`.

### C2 — RMG whole-object replacement followed by fixed map

In one process, capture full MapGen immediately after the named `REP MOVSD @ 0x0059899B`, recording `MapSeed+0x74` and whether the writer came from preview, generate, or accepted-launch path. Capture it again at end of the RMG interaction and at Match 2 fixed-map accepted/L0.

Because setup preview itself can invoke the writer, use two subfixtures if feasible:

- C2a: accepted RMG launch -> later fixed match;
- C2b: RMG preview/generate then cancel -> fixed match.

Acceptance: the post-writer object matches an independently replayed `Seed(recorded_seed)` table plus the observed three padding bytes; later fixed-map capture proves retention or records the exact intervening reset writer.

Proposed executable test names: `oracle_mapgen_c2a_rmg_launch_object_survives_fixed_reentry` and `oracle_mapgen_c2b_rmg_preview_cancel_object_survives_fixed_launch`.

## Remaining Uncertainty

- **Same-process lifecycle:** C0/C1/C2 runtime captures are still required to decide whether fixed-map no-draw state, a repair-advanced object, and an RMG-replaced object are retained or reset across shell/match reentry.
- **`0x005A5020` liveness:** its RMG-shaped body is decoded, but no direct code/data xref exists; computed or indirect activation remains UNCHECKED.
- **Raw callsite ownership:** `0x00585858` and `0x005858B1` have the local RMG cell-iterator/type-selection shape, but their exact owning function boundary remains unresolved because Ghidra has no function boundary there.
- **Rust raw-padding policy:** native/oracle captures must retain and compare bytes `+1..+3`, but whether Rust must store, serialize, hash, or preserve those bytes remains UNKNOWN pending active-path consumption/xref or controlled perturbation evidence.

## 9. Coverage ledger

| Assigned surface | Coverage | Active in YR | Evidence/result | Residual |
|---|---|---|---|---|
| `g_MapGenRng 0x00ABE890` | FULL static xref census | Yes | 141 xrefs classified; Seed/copy/Next/sentinel separated | indirect/bulk runtime exclusion |
| raw CRT `0x0058B770` | FULL static | Yes, PE startup | exact bytes, initializer table, CRT iterator, PE entry | runtime execution count only |
| RMG writer `0x00598960` | FULL writer/lifecycle slice | Conditional | exact seed + 253-dword copy; scenario and setup callers | runtime C2 retention |
| `Random__Seed 0x0065C6D0` | FULL object-boundary slice | Yes | exact header/table writes, padding omission, caller census | none for assigned slice |
| helper `0x00598030` | FULL | Conditional | exact receiver/scaling/retry; 10 functions/11 CALLs | rejection frequency is data-dependent |
| `0x005A1350` | FULL classification | Conditional RMG | RMG bridge tile remap | none |
| `0x005A3AE0` | FULL classification | Conditional RMG | RMG terrain/LAT/rocks | none |
| `0x005A5020` | FULL body, no liveness proof | UNKNOWN | RMG-shaped, no code/data xrefs | computed/indirect reachability |
| `0x005A95B0` | FULL classification | Conditional RMG | RMG tech placement | none |
| four repair callers | FULL assigned RNG slice | Yes conditional | exact overlay gates/ranges and repair chains | runtime C1 |
| `0x0057ACF0` | FULL body/callsite slice | Conditional RMG | RMG tile selector | two raw callsites lack Ghidra owner boundary |
| `0x00579620` | FULL | Conditional RMG | one roll reused in RMG tile stamping | none |
| `0x00575BA0` | FULL receiver/order slice | Yes conditional destruction | Scenario receiver at all four sites | allocation outcome runtime not needed for mechanism |
| `0x0047DD70` | FULL receiver/order slice | Yes conditional destruction | Scenario receiver at all seven sites | none for assigned slice |
| `ScenarioClass__Read_Scenario 0x00684620` | MEDIUM branch slice | Yes | fixed INI vs `.SED` writer branch | outer two-match runtime only |
| C0/C1/C2 | BLOCKED runtime | Yes/conditional | evidence contract specified | executable launch not authorized |
| current Rust ownership/routing | FULL read-only comparison | Yes | fresh-state DRIFT; later-match policy UNCHECKED | implementation deferred |

## 10. Open-question ledger

| ID | Final state | Answer |
|---|---|---|
| C01 | **[DEFERRED — runtime blocked]** | No teardown/reentry reset was found statically; C0/C1/C2 must exclude missed indirect/bulk writes and prove actual same-process behavior. |
| C02 | **[RESOLVED static]** | Raw `0x0058B770` is in the CRT initializer table reached once by the ordinary PE-entry startup path; it has no match-loop caller. Runtime execution count was not collected. |
| C03 | **[DEFERRED — runtime blocked]** | C1 must decide whether a bridge-repair advancement survives a later fixed match in the same process. |
| C04 | **[DEFERRED — runtime blocked]** | C2 must decide whether an RMG-written object survives later fixed launch/reentry. |
| C05 | **[RESOLVED]** | `0x0057ACF0` and `0x00579620` are conditional RMG map-construction tile selectors, not active destruction-event owners. |
| C06 | **[RESOLVED]** | Active destruction uses Scenario RNG; MapGen enters active match gameplay through later engineer/CABHUT repair variant restoration. |
| C07 | **[PARTLY RESOLVED / DEFERRED]** | RMG copies stack-derived padding bytes. Whether those exact bytes survive fixed reentry requires C2. |
| C08 | **[DEFERRED — runtime policy]** | Rust currently reconstructs MapGen per `Simulation`. If retention is observed, ownership must move to or be handed through a process/session-shell boundary; choose only after C0/C1/C2. |
| C09 | **[RESOLVED]** | All ten direct helper callers are classified by body and static caller chain. |
| C10 | **[DEFERRED]** | `0x005A5020` has no static caller; computed/indirect activation remains UNCHECKED. |
| C11 | **[DEFERRED]** | Exact function owner of raw `0x00585858`/`0x005858B1` is unresolved because Ghidra has no boundary; their local RMG cell-iterator role is established. |
| C12 | **[RESOLVED]** | Native object state ends at `+0x3F3`; prior `[+0x0C..+0x3F7]` wording is stale. |

## 11. Negative facts and stale wording corrections

- `g_MapGenRng` is not raw-zero on fresh startup; CRT executes `Seed(0)`.
- `Random__Seed` does not write padding `+1..+3`; RMG therefore does not guarantee zero padding.
- `0x005AC192` is not a MapGen reset; the MapGen address is an end sentinel for objects below it.
- `0x00598030` is not RMG-only; four active bridge repair walkers call it.
- `0x00598030` is not a destruction RNG helper; representative destruction paths use Scenario RNG.
- `0x0057ACF0` and `0x00579620` must not be classified from their current labels alone; their callers place them in RMG construction.
- `ProcessBridgeDestruction_Low/High @ 0x00570050/0x00573540` are stale labels for engineer-repair owners.
- `0x00579620` makes one entry roll and reuses it; old two-roll descriptions are stale.
- no static reset found between matches does not equal verified retention.
- Rust-vs-Rust RNG tests and hashes are regression evidence, not gamemd parity certification.

Required prose replacements:

| Location/current wording | Correct replacement |
|---|---|
| `vera20k-oracle:docs/research/ORACLE_NATIVE_STARTUP_AUTHORITY_GATES_GHIDRA_REPORT.md`: “reachable from active bridge repair/destruction chains” / “bridge repair/destruction actively consumes MapGen” | “`0x00598030` is consumed in active match gameplay by the four bridge repair walkers. Its remaining statically reachable direct callers are conditional RMG map-construction functions; `0x005A5020` remains statically unreferenced. Audited destruction walkers and `CellClass::BlowUpBridge` use Scenario RNG.” |
| `RMG_RNG_SEED_MAPGENRNG_GHIDRA_REPORT.md`: state `[0xC..0x3F7]` | state `[0x0C..0x3F3]`, complete object/copy `[0x000..0x3F3]` (`0x3F4` bytes) |
| Rust comments: fixed MapGen “never seeded”, “all-zero”, or “variant 0 forever” | fresh native MapGen is `Seed(0)`; later bridge-repair result follows its actual process state; same-process retention remains runtime-UNCHECKED |

This report records the corrections but does not patch older documents or Rust because the worker's allowed write set is exactly this report.

## 12. Implementation handoff and acceptance scenarios

### Verified deltas now safe to carry forward

1. Fresh Rust MapGen construction must use native `Seed(0)` semantics, not `SimRng::zeroed()`.
2. Rust RNG logic must reproduce the verified disabled byte, both indices, and 250 generated state words. The three native padding bytes remain part of full-object oracle evidence, but a Rust storage/serialization/hash requirement for them is **UNKNOWN** pending active-path consumption/xref or perturbation proof.
3. Bridge repair random healthy-variant selection must remain routed to MapGen and preserve the four exact draw gates/ranges in section 5.1.
4. Bridge destruction transition and presentation RNG must not be rerouted to MapGen; audited destruction presentation/fallout is Scenario-owned.
5. Future RMG support must reproduce the logical MapGen state produced by seeding the stack temporary from `MapSeed+0x74` and the native 253-dword copy. Oracle tests must capture the copied padding, but Rust need not model those raw bytes unless the pending padding-consumption proof makes them behaviorally authoritative.

### Deltas still blocked

Do not yet choose process-global ownership, reset on each Rust `Simulation`, carry-over from shell state, or fixed-map restoration behavior. C0/C1/C2 must settle that lifecycle contract first.

### Native-derived acceptance scenarios

| Scenario | Native evidence | Rust implication | Proposed test |
|---|---|---|---|
| fresh process, fixed map | CRT `Seed(0)` and no fixed-load writer | exact generated table, indices 0/103; not zero-state | `mapgen_fresh_state_matches_native_seed_zero_full_object` |
| one NS-low repair on overlay `0x64` | `0x0057F6A0` calls helper `0..3`, adds `0x4A` | one helper invocation plus possible rejection; Scenario/Main unchanged | `bridge_repair_ns_low_destroyed_anchor_consumes_mapgen_only` |
| deterministic repair transition outside random healthy band | repair body bypasses `0x00598030` | no MapGen draw | `bridge_repair_deterministic_transition_preserves_all_rngs` |
| destruction walker animation | `0x00575BA0` Scenario receiver at all draw sites | MapGen unchanged; Scenario advances in exact allocation-dependent order | `bridge_collapse_walker_animation_uses_scenario_not_mapgen` |
| per-cell bridge fallout | `0x0047DD70` Scenario receiver at seven possible sites | exact 0/1/4..7 Scenario draws; MapGen unchanged | `bridge_blowup_fallout_draw_matrix_uses_scenario_only` |
| RMG writer | `0x00598960` copies 0xFD dwords | reproduce the logical seeded state; capture native padding separately while its Rust authority remains UNKNOWN | `rmg_writer_replaces_complete_mapgen_object` |
| later fixed match in same process | static census finds no reset, but runtime missing | ownership/reset policy remains gated | C0/C1/C2 executable oracle tests in section 8 |

## 13. Final status

**Static Track C status: COMPLETE.** Every named direct `0x00598030` caller was inspected and classified; the repair-versus-destruction contradiction is resolved; writer/reset mechanisms and the exact object boundary are revalidated; representative destruction receivers are audited; current Rust drift is mapped.

**Whole Track C status: PARTIAL.** Same-process lifecycle certification is deliberately not claimed. C0, C1, and C2 remain blocked because this worker was instructed not to launch `gamemd.exe`. The next authorized action is the bounded runtime suite in section 8, not further static expansion.

## 14. Sources consulted

Binary evidence:

- fresh read-only Ghidra `get_function_callers`, `get_xrefs_to`, `get_bulk_xrefs`, `get_function_by_address`, `decompile_function`, `disassemble_function`, and `read_memory` calls for the addresses named above;
- local retail `gamemd.exe` with the pinned SHA-256 at the top of this report.

Repository evidence:

- `vera20k-oracle:docs/plans/2026-07-13-oracle-startup-certification-gaps-mapgen-lifecycle-investigation-plan.md`
- `vera20k-oracle:docs/research/ORACLE_NATIVE_STARTUP_AUTHORITY_GATES_GHIDRA_REPORT.md`
- `docs/research/skirmish-ui/RMG_RNG_SEED_MAPGENRNG_GHIDRA_REPORT.md`
- `docs/research/skirmish-ui/SKIRMISH_RANDOM_MAP_GENERATOR_00598960_GHIDRA_REPORT.md`
- [docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md)
- [docs/research/bridges/05-damage-collapse-repair-cabhut/REPAIRBRIDGEWALKER_BODIES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/REPAIRBRIDGEWALKER_BODIES_GHIDRA_REPORT.md)
- [docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_RNG_CALL_ORDER_CLASSIFICATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_RNG_CALL_ORDER_CLASSIFICATION_GHIDRA_REPORT.md)
- [docs/research/bridges/08-traces/BRIDGE_COLLAPSE_VISUAL_RNG_ORDER_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/BRIDGE_COLLAPSE_VISUAL_RNG_ORDER_TRACE.md)
- [docs/research/substrate/tables/BRIDGE_OVERLAY_SUBSTRATE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/tables/BRIDGE_OVERLAY_SUBSTRATE_STUDY.md)
- `ini/rules.ini`, `ini/rulesmd.ini`
- current Rust files and line ranges cited in section 7.
