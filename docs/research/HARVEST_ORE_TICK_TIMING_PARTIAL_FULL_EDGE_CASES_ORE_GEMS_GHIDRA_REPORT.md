# Harvest Ore Tick Timing, Partial/Full Edge Cases, Ore/Gems - Ghidra Research Report

**Address(es):** `0x0073D450` (`UnitClass::Harvest_Ore_Tick`), immediate caller `0x0073E5E0` (`UnitClass::Mission_Harvest` state 1)  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** standard YR `HARV`/`CMIN` ore and gem harvesting in `Mission_Harvest` state 1: timer gate, destination-present shortcut, request amount, storage full/partial behavior, `Reduce_Tiberium` return handling, ore-vs-gem storage type, full/partial cell reduction outcomes, Weeder exclusion, and non-tiberium-cell behavior.  
**Non-Scope:** refinery unload/deposit, return-to-refinery pathing, state-0 scan selection, slave miner slave infantry harvest, full `CellClass::Reduce_Tiberium` internals beyond caller-visible return/side effects, selected-unit pip drawing, runtime debugger frame capture.  
**Confidence:** High for branch ordering, timer field writes, destination/full/non-tiberium behavior, `Reduce_Tiberium` call/return use, and ore/gem type/value activation. Medium for sub-integer remaining-capacity FPU edge wording because Ghidra decompilation hides the x87 stack detail, but `Math__ftol` truncation and normal integer-storage scenarios are verified.  
**Active in YR:** Yes for standard `[HARV]` and `[CMIN]` ore/gem harvest through `Harvester=yes`; Conditional for the TS `Weeder` branch, which is not active on standard `HARV`/`CMIN`.

**2026-07-24 timing correction:** Live disassembly reopened the previously
deferred global AI/StepTimer order. `TechnoClass::AI_Update` calls
`Mission_Dispatch` at `0x006FA655` before it maintains the shared StepTimer at
`0x006FABC4..0x006FAC22`. With stock rate `2`, the ninth increment is written
after the mission call at `F+18`; `Mission_Harvest` first observes counter `9`
and calls `Harvest_Ore_Tick` at `F+19`. The older exact-`F+18` conclusion and
the resulting Rust "one tick late" claim are superseded.

## Working Notes

**Target question:** What exactly does standard YR do when a `HARV` or `CMIN` harvest tick fires with partial cargo, near-full cargo, full cargo, ore or gem cells, a movement destination still present, a depleted/non-tiberium current cell, or a TS `Weeder` flag?

**Non-goals:** Do not cover refinery unload, return logic, dock radio, long scan selection, complete spread/growth queue internals, slave-miner worker harvest, or Rust patches.

**Evidence needed:** direct Ghidra read of `Harvest_Ore_Tick @ 0x0073D450`; direct Ghidra read of caller state-1 branch in `Mission_Harvest @ 0x0073E5E0`; xrefs proving the caller set; helper reads for storage percentage/total/add and `Reduce_Tiberium`; INI proof for standard `HARV`/`CMIN`, Riparius/Cruentus values, and scan/timer keys; current Rust scan for `handle_harvest`, `extract_bales_max`, `CargoBale`, `ResourceType`, and tests.

**Stop conditions:** Stop once every scoped branch of `Harvest_Ore_Tick` has a caller-visible result, every material caller reaction in state 1 is classified, every Rust-facing mismatch has an acceptance test proposal, and no open question remains except explicitly deferred out-of-scope/FPU/runtime-debugger items.

## 1. Overview

`UnitClass::Harvest_Ore_Tick` is the single standard YR per-harvest extraction helper called from `Mission_Harvest` state 1 after the state timer reaches 9 steps. It either returns success without harvesting while a destination exists, extracts a clamped integer amount from the current tiberium/gem cell and adds that amount to the correct storage slot, or returns failure after resetting timer fields when the unit is full, not a harvester, or not standing on LandType `5`.

For standard YR `HARV` and `CMIN`, the live path is the non-Weeder `Harvester=yes` path. Gems are not a special harvester branch; they are tiberium type `Cruentus` with `Value=50`, so the same call path stores the removed amount into storage slot 1 instead of Riparius slot 0.

## 2. Class Layout / Key Offsets

| Owner | Offset | Meaning in this slice | Evidence | Active in YR |
|---|---:|---|---|---|
| `UnitClass` | `+0x9C/+0xA0/+0xA4` (`param_1[0x27..0x29]`) | Current world coordinate used to get the current `CellClass`. | `0x0073D450` decompile | Yes |
| `UnitClass` | `+0xBC` (`param_1[0x2F]`) | `Mission_Harvest` substate; state 1 calls `Harvest_Ore_Tick`. | `0x0073E5E0` switch | Yes |
| `UnitClass` | `+0xF8` (`param_1[0x3E]`) | Harvest step counter; state 1 gates on `< 9`. Reset by `Harvest_Ore_Tick` success/failure paths. | `0x0073E96F`, `0x0073D450` | Yes |
| `UnitClass` | `+0x100` (`param_1[0x40]`) | Step timer start frame; reset to `g_CurrentFrameCounter`. | `0x0073E946`, `0x0073D450` | Yes |
| `UnitClass` | `+0x108` (`param_1[0x42]`) | Step timer amount/duration; set to `HarvesterLoadRate` on ore success, `HarvesterLoadRate*3` on Weeder success, `0` on failure reset. | `0x0073D450` | Yes / Conditional |
| `UnitClass` | `+0x10C` (`param_1[0x43]`) | Step timer rate; `0` triggers state-1 initialization in caller. | `0x0073E934`, `0x0073D450` | Yes |
| `UnitClass` | `+0x5A4` (`param_1[0x169]`) | Destination pointer/cell. If nonzero, `Harvest_Ore_Tick` returns success immediately without harvesting or resetting timer. | `0x0073D450` first branch | Yes |
| `UnitClass` | `+0x6C4` (`param_1[0x1B1]`) | `UnitTypeClass*`. | `0x0073D450` | Yes |
| `UnitTypeClass` | `+0x800` | `Storage=` capacity in tiberium units. `CMIN=20`, `HARV=40`. | `0x0073D450`; `rulesmd.ini` | Yes |
| `UnitTypeClass` | `+0xE0E` | `Harvester=yes` flag. Enables standard ore/gem harvest. | `0x0073D450`; `rulesmd.ini [CMIN]/[HARV]` | Yes |
| `UnitTypeClass` | `+0xE0F` | `Weeder=yes` flag. Selects TS weed branch. | `0x0073D450`; rules comments | Conditional; not standard `HARV`/`CMIN` |
| `CellClass` | `+0xEC` | LandType; `5` is tiberium/ore/gem. Non-5 makes harvest fail. | `0x0073D450`; prior LandType table | Yes |
| `CellClass` | `+0x44`, `+0x11E` | Overlay type and overlay data used by `Reduce_Tiberium`. | `0x00480A80` report | Yes |
| `RulesClass` | `+0x1520` | `HarvesterLoadRate`, default `2`. | `0x0073D450`, `RULESCLASS_FIELDS.csv` | Yes |

## 3. Core Logic

### 3.1 State-1 timing gate before `Harvest_Ore_Tick`

Active in YR: Yes.

`Mission_Harvest @ 0x0073E5E0` state 1 first initializes the step timer if
`UnitClass+0x10C == 0`, using `RulesClass+0x1520`
(`HarvesterLoadRate`). It then checks `UnitClass+0xF8 < 9`; while true, it
returns `1` without calling `Harvest_Ore_Tick`.

The exact stock sequence is:

1. Timer initialization/reset at frame `F` writes counter `0`, start `F`,
   duration/repeat `2`.
2. The later same-`AI_Update` timer maintenance sees elapsed `0`.
3. Post-mission maintenance increments the counter at
   `F+2,F+4,...,F+18`.
4. Mission dispatch at `F+18` still reads counter `8`, after which maintenance
   writes `9`.
5. Mission dispatch at `F+19` first reads `9` and calls
   `Harvest_Ore_Tick`.

Thus `9 * HarvesterLoadRate = 18` is the elapsed threshold at which the ninth
post-mission increment is written, while helper observation is one frame later.
This applies both to state-1 timer initialization and to a successful standard
extraction reset.

State-1 initialization is not physical-arrival initialization:
`Mission_Harvest` writes state 1 and the timer immediately after the
search-and-move helper succeeds at `0x0073E87D..0x0073E8B7`. The timer can
mature during travel. Once mature, destination-present helper calls keep
returning success without resetting it, so physical-arrival-to-extraction
latency is variable rather than a fixed 19 frames.

The important caller-visible ordering is:

1. Timer setup if rate is zero.
2. `< 9` gate.
3. Call `Harvest_Ore_Tick`.
4. If the call returns false, only then clear `UnitClass+0x6D2` and run full/retarget/return branch logic.

### 3.2 Destination-present shortcut

Active in YR: Yes.

At the top of `Harvest_Ore_Tick`, after resolving the current cell, the function checks `UnitClass+0x5A4`. If it is nonzero, the function returns a low-byte success value (`1`) immediately.

Caller-visible consequences:

- No `Harvester=yes`, storage-full, LandType, Weeder, tiberium type, or `Reduce_Tiberium` work runs.
- No storage is added.
- No timer fields are reset by `Harvest_Ore_Tick`.
- `Mission_Harvest` sees success and exits state 1 for this tick without clearing `+0x6D2` or short-retargeting.

This matters for blocked/retarget edge cases: a miner en route to ore can be in harvest substate 1, but while a destination remains present, the extraction helper deliberately does nothing and reports success.

### 3.3 Failure/reset gate: not harvester, full, or not tiberium

Active in YR: Yes for full/non-tiberium standard harvester cases; Conditional for non-harvester mission misuse.

If there is no destination, `Harvest_Ore_Tick` checks:

- `UnitTypeClass+0xE0E == 0` (`Harvester=no`);
- storage percentage from vtable `+0x2B4` is `>= 1.0`;
- current cell `CellClass+0xEC != 5`.

If any is true, it resets the StepTimer fields and returns failure (`0` in the low byte). The reset writes include step counter `0`, start frame `g_CurrentFrameCounter`, step amount `0`, and rate `0`. The rate-zero write is important because the next state-1 tick re-enters the timer-initialization path if state 1 remains active.

Caller-visible consequences:

- Full storage at the harvest gate does not call `Reduce_Tiberium` with amount 0.
- A depleted cell whose LandType reverted from `5` to native terrain does not call `Reduce_Tiberium`; it fails and lets `Mission_Harvest` state 1 perform the short-scan/return branch.
- For a standard full harvester, `Mission_Harvest` then checks storage percentage again and, when it is exactly `1.0`, writes substate `2` (return-to-refinery). This full branch runs before short-scan continuation.

Tiny edge: the caller's full-return check is an equality comparison against `1.0` after `Harvest_Ore_Tick` failure, while the callee's full gate is `>= 1.0`. Standard integer harvest storage should reach exactly full, but an overfull/modded/fractional state could fail the callee full gate without matching the caller equality. That remains a conditional edge, not a normal stock `HARV`/`CMIN` scenario.

### 3.4 Weeder branch

Active in YR: Conditional; not active for standard `HARV` or `CMIN`.

If the unit type has `Weeder=yes` (`UnitTypeClass+0xE0F != 0`) after passing the standard `Harvester=yes`, non-full, LandType-5 gates, `Harvest_Ore_Tick` does not use the normal ore/gem path. It calls the weed reduction helper at `0x00486E30`, adds exactly `1.0` to storage slot 0, and resets the timer with `HarvesterLoadRate * 3`.

Do not apply this branch to standard YR ore miners. `rulesmd.ini [CMIN]` and `[HARV]` set `Harvester=yes` and do not set `Weeder=yes`.

### 3.5 Standard ore/gem branch

Active in YR: Yes.

For `Harvester=yes`, `Weeder=no`, no destination, not full, and current LandType `5`, the helper:

1. Gets the tiberium type from the current cell through `CellClass::GetTiberiumType`, which wraps overlay-to-tiberium-index lookup.
2. Reads `UnitTypeClass+0x800` (`Storage=` capacity).
3. Reads current load via `StorageClass::GetTotalAmount` (four storage float slots).
4. Computes remaining capacity as `Storage - current_load` and clamps it to at most one unit: `FSUBR` at `0x0073D565`, `FCOMP float [0x007E2AC8]` (= `1.0f`) at `0x0073D569`, `TEST AH,0x41 / JNZ 0x0073D57E` keeps the difference only when it is `<= 1.0`, otherwise `FLD float [0x007E2AC8]` at `0x0073D576` loads `1.0`. The request is therefore `min(1.0, Storage - current_load)`, never the whole free capacity.
5. Converts that value through `Math__ftol` (`CALL 0x007C5F00` at `0x0073D599`).
6. Calls `CellClass::Reduce_Tiberium(amount)` (`PUSH EAX; CALL 0x00480A80` at `0x0073D59E..0x0073D5A1`), so a normal gate removes exactly one density level.
7. If removed amount is positive, calls `StorageClass::AddAmount((float)removed, tib_type)`.
8. Resets the StepTimer to `HarvesterLoadRate` and returns success.
9. If removed amount is zero, returns failure without the success timer reset.

`Math__ftol @ 0x007C5F00` is documented in prior direct disassembly reports as truncating toward zero under the game FPU control word. For normal non-negative remaining capacity this is floor/truncation. Concrete normal cases:

- Empty `CMIN`: `Storage=20`, total `0.0`, difference `20.0 > 1.0`, request `1`.
- `CMIN` with 19 carried units: difference `1.0 <= 1.0`, request `ftol(1.0) = 1`.
- `HARV` with 38 carried units: difference `2.0 > 1.0`, request `1`.
- An 11-density cell therefore takes 11 successful gates (each re-armed to `HarvesterLoadRate`) to reach `OverlayData=0`; the twelfth gate's `Reduce_Tiberium(1)` on data `0` takes the full-removal path and returns `0`.
- If total load leaves less than 1.0 free capacity, `ftol` can produce `0`; then `Reduce_Tiberium(0)` returns `0`, and the caller treats the harvest tick as failure. This is conditional because standard harvesting adds integer amounts, but it is the expected edge if fractional storage can be produced by another system.

### 3.6 Partial vs full cell reduction from the caller's point of view

Active in YR: Yes.

`Harvest_Ore_Tick` does not separately know whether a cell reduction was partial or full. It trusts `Reduce_Tiberium`'s return value.

Caller-visible outcomes:

- Partial reduction: `Reduce_Tiberium(amount)` returns the requested amount; `Harvest_Ore_Tick` adds that exact float amount to storage, resets timer, returns success.
- Full removal: `Reduce_Tiberium(amount)` clears the overlay, recalculates cell attributes, marks radar/tactical dirty paths, reseeds spread queues per the separate `Reduce_Tiberium` report, returns the removed amount, and `Harvest_Ore_Tick` adds that amount to storage.
- Exact/full boundary: the callee's full-removal branch runs when request is not less than the cell's current threshold. The current verified max-density edge is `OverlayData=11`, request `20`, return `11`, not `12`. This is the source of the known Rust 12-vs-11 overharvest mismatch for real overlay-backed max ore.

### 3.7 Ore vs gems

Active in YR: Yes.

Ore and gems both use the same standard harvester branch. The difference is the tiberium type returned from the current cell overlay:

- `[Tiberiums] 0=Riparius`, `[Riparius] Value=25`, standard ore overlays.
- `[Tiberiums] 1=Cruentus`, `[Cruentus] Value=50`, GEM overlays.

`Harvest_Ore_Tick` passes the tiberium type index into `StorageClass::AddAmount`. It does not multiply by the credit value here. Credit value is applied later by storage/deposit/economy systems. Therefore a gem harvest tick carries "N units of Cruentus", not "2N ore units"; the doubled value comes from `Cruentus.Value=50`.

Rust currently represents this as `CargoBale { resource_type: Gem, value: 50 }`, which matches the value distinction at the cargo-bale abstraction level, but the same overlay-density overharvest risk applies to gems if real GEM overlay frame `11` is seeded as 12 harvestable bales.

## 4. INI Keys

| INI file / section | Key | Stock YR value | Effect in this slice | Binary evidence | Active in YR |
|---|---|---:|---|---|---|
| `rulesmd.ini [General]` | `HarvesterLoadRate` | default `2` from RulesClass field reports | StepTimer rate and success reset for ore/gem harvest; Weeder uses `* 3`. | `RulesClass+0x1520` reads in `0x0073E5E0` and `0x0073D450`; `RULESCLASS_FIELDS.csv` | Yes |
| `rulesmd.ini [General]` | `TiberiumShortScan` | `6` | Used by caller after a false harvest tick on non-full harvester; not inside `Harvest_Ore_Tick`. | `0x0073EAA6..0x0073EAB9` | Yes |
| `rulesmd.ini [General]` | `TiberiumLongScan` | `48` | State-0 scan only; not this state-1 helper. | prior state-0 reports | Yes, out of scope |
| `rulesmd.ini [CMIN]` | `Harvester` | `yes` | Enables standard harvest branch. | `UnitTypeClass+0xE0E` in `0x0073D450` | Yes |
| `rulesmd.ini [CMIN]` | `Storage` | `20` | Capacity used as request basis and full gate denominator. | `UnitTypeClass+0x800` in `0x0073D450`; INI | Yes |
| `rulesmd.ini [CMIN]` | `Teleporter` / chrono locomotor data | stock chrono miner | Not read by `Harvest_Ore_Tick`; relevant to return logic only. | no read in `0x0073D450` | Yes, out of scope here |
| `rulesmd.ini [HARV]` | `Harvester` | `yes` | Enables standard harvest branch. | `UnitTypeClass+0xE0E` in `0x0073D450` | Yes |
| `rulesmd.ini [HARV]` | `Storage` | `40` | Capacity used as request basis and full gate denominator. | `UnitTypeClass+0x800` in `0x0073D450`; INI | Yes |
| `rulesmd.ini [Tiberiums]` | `Riparius` | index `0`, `Value=25` | Standard ore storage type/value. | `CellClass::GetTiberiumType`; INI | Yes |
| `rulesmd.ini [Tiberiums]` | `Cruentus` | index `1`, `Value=50` | Gems storage type/value. | `CellClass::GetTiberiumType`; INI | Yes |
| Unit type | `Weeder` | not set on `HARV`/`CMIN` | Selects TS weed path when set. | `UnitTypeClass+0xE0F` in `0x0073D450` | Conditional; not standard target |

## 5. Integration Points

| Function / point | Role | Evidence | Active in YR |
|---|---|---|---|
| `UnitClass::Mission_Harvest @ 0x0073E5E0` | Only direct caller of `Harvest_Ore_Tick`; state 1 timer gate and false-return handling. | `get_function_xrefs 0x0073D450` returned one call from `0x0073E987`; decompile of caller | Yes |
| `UnitClass::Harvest_Ore_Tick @ 0x0073D450` | Scoped extraction helper. | direct decompile | Yes |
| `UnitClass::Get_Storage_Percentage @ 0x007414A0` | Fullness gate helper; returns total storage / `Storage=` for harvesters/weeders, else 0. | direct decompile | Yes |
| `StorageClass::GetTotalAmount @ 0x006C9650` | Sums/converts storage float slots; decompilation is terse but confirms four-slot aggregate. | direct decompile; prior docs | Yes |
| `StorageClass::AddAmount @ 0x006C9690` | Adds float amount into storage slot indexed by tiberium type. | direct decompile | Yes |
| `CellClass::GetTiberiumType @ 0x00485010` | Returns overlay tiberium type index for storage slot. | direct decompile wrapper plus `Reduce_Tiberium` report | Yes |
| `CellClass::Reduce_Tiberium @ 0x00480A80` | Removes ore/gem density and returns removed amount. | direct decompile and dedicated report | Yes |
| `FUN_00486E30` | Weed reduction helper. | direct decompile | Conditional; not standard target |
| `Math__ftol @ 0x007C5F00` | Converts remaining capacity to integer request. | direct decompile plus prior disassembly report for truncation | Yes |

Tick-cycle correction (2026-07-24): live disassembly reopened the global object
AI scheduling order. `UnitClass::AI -> FootClass::AI ->
TechnoClass::AI_Update` reaches `Mission_Dispatch` at `0x006FA655`, then reaches
the shared StepTimer maintenance only at `0x006FABC4..0x006FAC22`.
`Main_Tick` increments `g_CurrentFrameCounter` later still at
`0x0055DE73..0x0055DE81`. Within a mission call, state 1 gates on the
pre-maintenance StepTimer value before invoking `Harvest_Ore_Tick`; storage/cell
mutations remain synchronous before the caller chooses return/retarget
behavior.

## 6. Current Rust Implementation Status

No Rust files were modified.

| Rust surface | Current behavior | Binary delta |
|---|---|---|
| `src/sim/miner/miner_system.rs` arrival branch | Physical arrival arms the frame-anchored timer for `harvest_tick_interval + 1`. | The `+1` correctly matches native reset/init-to-helper observation at `F+19`, but the anchor is too late: native initializes state 1/timer on search-and-move success, so it can mature during travel. |
| `src/sim/miner/miner_system.rs::handle_harvest` | Inclusive `MissionTimer` due check plus `interval + 1` re-arm. | Matches the bounded stock success-reset-to-next-helper cadence. Do not remove `+1`; generic StepTimer intermediate-state equivalence remains separately unchecked. |
| `src/sim/miner/miner_system.rs:526` | Empty capacity is `capacity_bales - cargo.len()`. | Matches normal integer storage cases, but cannot represent fractional storage edge cases from gamemd storage floats. |
| `src/sim/miner/miner_system.rs:534` / `:804` | `extract_bales_max` drains `min(empty_capacity_bales, density_levels)` in one call. | Shape matches caller request/clamp, but live overlay-backed max-density cells are seeded as 12 levels in Rust, while gamemd `OverlayData=11` returns 11. |
| `src/sim/miner/miner_system.rs::handle_harvest` positive branch | Successful extraction re-arms for `harvest_tick_interval + 1`, but immediately archives/begins return if that extraction fills cargo. | Cadence matches `F+19`; the immediate full transition does not. Native success remains state 1 and defers full failure/archive/return selection until the later helper gate. |
| `src/sim/miner/miner_system.rs:561` | Empty-cell short scan hit switches top-level state to `MoveToOre`. | Separate visual/substate mismatch already covered by retarget visual report. |
| `src/sim/miner/miner_system.rs:810` | Zero empty capacity returns no bales. | Equivalent to a zero request returning failure, but Rust's caller `is_full()` branch happens after no-bale result, not before extraction call. |
| `src/sim/miner/miner_system.rs:821` | Gems use `gem_bale_value=50`, base `180`. | Value distinction matches `Cruentus.Value=50`; density model still has the overlay frame +1 issue. |
| `src/sim/production/production_queue.rs:155` | Overlay frame is seeded as `frame.min(11)+1` richness. | Main known mismatch: real `OverlayData=11` should harvest as 11 from the verified `Reduce_Tiberium` path. |
| `src/sim/miner/mod.rs:253`, `:345` | Cargo is discrete `Vec<CargoBale>` and fullness is `cargo.len() >= capacity`. | Matches standard integer unit storage, but not fractional float-storage edge cases. |
| `src/sim/miner/miner_tests.rs:3731` | Tests label `11 * 120` as "11 density levels". | These tests do not cover real overlay-backed `OverlayData=11` seeding, where Rust currently creates 12 levels. |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `Harvest_Ore_Tick` destination-present shortcut | verified | `0x0073D450` decompile; destination `+0x5A4` branch | none |
| `Harvest_Ore_Tick` not-harvester/full/not-tiberium reset | verified | `0x0073D450` decompile | exact assembly not separately dumped; decompile is unambiguous |
| Standard ore/gem request and add path | verified | `0x0073D450`, `0x006C9690`, `0x00485010`, `0x00480A80` | sub-integer FPU edge marked medium |
| Weeder branch | verified for branch shape | `0x0073D450`, `0x00486E30` | exact weed overlay reduction internals out of scope |
| `Mission_Harvest` state-1 timer gate | verified | `0x0073E5E0`; `TechnoClass::AI_Update 0x006F9E50`; `Main_Tick 0x0055D360` disassembly | generic modded-rate and intermediate-state Rust equivalence remain separate |
| `Mission_Harvest` false-return full branch | verified | `0x0073E5E0` decompile | overfull/fractional abnormal branch needs runtime if pursued |
| `Mission_Harvest` false-return short scan branch | touched-not-exhausted | `0x0073E5E0`; dedicated visual retarget report | full scan selection internals out of this slot |
| `Reduce_Tiberium` partial/full internals | touched-not-exhausted | `0x00480A80`; dedicated report | exact direction table values and queue architecture are sibling slots |
| Ore value/Riparius activation | verified | `rulesmd.ini [Tiberiums]/[Riparius]`; tiberium type add path | none |
| Gem value/Cruentus activation | verified | `rulesmd.ini [Tiberiums]/[Cruentus]`; tiberium type add path | none |
| Current Rust `handle_harvest` | verified | Codegraph and source scan | implementation not performed |
| Current Rust `extract_bales_max` | verified | Codegraph and source scan | implementation not performed |
| Selected cargo pips | deferred | prior trace left formula unchecked | out-of-scope; run a pip rendering investigation |
| Refinery unload/return | deferred | user non-scope | out-of-scope; existing unload/return traces cover it |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-01 - Is `Harvest_Ore_Tick` live in standard YR? -> Yes, `Mission_Harvest` state 1 is the only direct xref and standard `HARV`/`CMIN` have `Harvester=yes`.` (evidence: `get_function_xrefs 0x0073D450`; `0x0073E5E0`; `rulesmd.ini [HARV]/[CMIN]`)
- `[RESOLVED] OQ-02 - Does a destination suppress harvesting? -> Yes, nonzero `UnitClass+0x5A4` returns success before all extraction gates.` (evidence: `0x0073D450`)
- `[RESOLVED] OQ-03 - What gates the normal ore/gem path? -> `Harvester=yes`, not full, current LandType `5`, and `Weeder=no`.` (evidence: `0x0073D450`)
- `[RESOLVED] OQ-04 - What happens when current cell is not tiberium? -> Timer fields reset to zero/rate-zero and the function returns false; caller then handles short-scan/return.` (evidence: `0x0073D450`; `0x0073EA8D..0x0073EB19`)
- `[RESOLVED] OQ-05 - What happens when storage is full? -> The callee resets timer and returns false before `Reduce_Tiberium`; the caller's full branch moves to substate 2 when percentage equals `1.0`.` (evidence: `0x0073D450`; `0x0073E99A..0x0073EA7B`)
- `[RESOLVED] OQ-06 - How is requested amount computed for normal cargo? -> `ftol(Storage - StorageClass::GetTotalAmount())`, then passed to `Reduce_Tiberium`.` (evidence: `0x0073D450`; `0x006C9650`; prior [ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-07 - What rounding does request conversion use? -> `Math__ftol`; prior direct disassembly docs establish truncate toward zero under YR's FPU control word.` (evidence: `0x007C5F00`; `ADD_TIBERIUM_CREDITS_PURIFIER...`)
- `[RESOLVED] OQ-08 - Does the function add credits or storage? -> Storage only: `StorageClass::AddAmount((float)removed, tib_type)`. Credit value is later.` (evidence: `0x0073D450`; `0x006C9690`)
- `[RESOLVED] OQ-09 - Are gems special-cased? -> No. Gems are `Cruentus` tiberium type index 1 with `Value=50`; same branch stores the removed amount into that type slot.` (evidence: `0x00485010`; `rulesmd.ini [Tiberiums]/[Cruentus]`)
- `[RESOLVED] OQ-10 - Does the Weeder path apply to CMIN/HARV? -> No for stock CMIN/HARV; conditional for modded/TS units with `Weeder=yes`.` (evidence: `0x0073D450`; `rulesmd.ini [CMIN]/[HARV]`)
- `[RESOLVED] OQ-11 - Does successful extraction reset the timer? -> Yes; success writes step counter `0`, start frame current, step amount/rate `HarvesterLoadRate`, then returns success.` (evidence: `0x0073D450`)
- `[RESOLVED] OQ-12 - Does zero removed amount reset the success timer? -> No success reset occurs after `Reduce_Tiberium` returns 0; the function returns false.` (evidence: `0x0073D450`)
- `[RESOLVED] OQ-13 - What is the max-density known edge? -> `OverlayData=11`: `Reduce_Tiberium` removes the overlay and returns 11 only if the request is >= 11 (the trace probed with an artificial request of 20); the live harvest gate requests `min(1.0, Storage - load)` = 1 per fire, so an 11-density cell takes 11 gates.` (evidence: `0x00480A80`; [CMIN_HARVEST_DENSITY_CARGO_REDUCE_TIBERIUM_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/CMIN_HARVEST_DENSITY_CARGO_REDUCE_TIBERIUM_TRACE.md))
- `[RESOLVED] OQ-14 - Does Rust currently represent cargo as float storage? -> No; it uses discrete `Vec<CargoBale>` and `cargo.len()` capacity.` (evidence: `src/sim/miner/mod.rs:253`, `src/sim/miner/mod.rs:345`)
- `[RESOLVED] OQ-15 - Does Rust have a real-overlay max-density test? -> Not in the scanned focused tests; current tests seed `11 * 120` directly and do not cover `production_queue.rs` `frame+1` seeding.` (evidence: `src/sim/miner/miner_tests.rs:3731`; `src/sim/production/production_queue.rs:155`)
- `[DEFERRED] OQ-16 - What exact x87 instruction sequence handles `remaining <= epsilon`?` (category: bounded-cost-too-high; reason: Ghidra decompile hides the FPU stack around one comparison, while normal integer storage behavior is already clear; next-step-if-pursued: instruction-level disassembly around the `Storage - total` comparison and `Math__ftol` call)
- `[DEFERRED] OQ-17 - What exact selected-unit pip count does gamemd draw for 11/20 CMIN storage?` (category: out-of-scope; reason: pip rendering was explicitly not part of `Harvest_Ore_Tick`; next-step-if-pursued: investigate `UnitClass::DrawPips`/sidebar pip path)
- `[DEFERRED] OQ-18 - What does the weed reduction helper do internally?` (category: out-of-scope; reason: TS Weeder branch is excluded from standard `HARV`/`CMIN` target; next-step-if-pursued: dedicated Weeder/weed overlay investigation)

Deferred ratio is 3/18. The report is complete for standard `HARV`/`CMIN` integer-storage harvest tick behavior, but not for pip rendering, weed harvesting internals, or instruction-level fractional-storage FPU trivia.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| State 1 calls `Harvest_Ore_Tick` after 9 StepTimer increments; with stock rate `2`, the ninth post-mission increment is written at `F+18` and the next mission observes it at `F+19`. State 1 starts on search-and-move success, not proven physical arrival. | `0x73E87D..0x73E8B7`; `0x73E96F..0x73E987`; `0x6FA655`; `0x6FABC4..0x6FAC22`; `RulesClass+0x1520` | Rust's `interval+1` matches reset-to-helper cadence, but anchoring only on arrival can add a full post-arrival wait after long travel. | `src/sim/miner/miner_system.rs` search/move, arrival, and harvest handlers | Preserve the `+1`; separately move timer/state-1 authority to the verified search-and-move transition and honor destination-present success without reset. | `harvest_timer_matures_during_travel`: after a travel longer than 19 frames, the first no-destination mission call can extract without another 19-frame arrival delay. | Do not encode a fixed physical-arrival-to-extraction interval; native latency depends on how much of the timer elapsed while moving. |
| A positive extraction that fills storage is still a successful state-1 tick; only the later `F+19` full check returns false, sets substate 2, and writes the archive. | Success/reset `0x73D5A1..0x73D5F7`; caller success `0x73E98E`; later full failure `0x73D4B6..0x73D626`; caller/archive `0x73E9D0..0x73EA7B` | Rust immediately archives and begins return on the filling extraction. | `src/sim/miner/miner_system.rs::handle_harvest` | Keep Harvest and re-arm on positive fill; at the later due full gate, check full before reduction, reset timer, set Return, scan/write the archive, and defer state-2 refinery work to the next tick. | `filling_extraction_waits_for_next_full_gate`: state remains Harvest through `F+18`, changes to Return/archive at `F+19`, and ore is not reduced at the full gate. | Do not call `begin_return` from the successful fill or recursively execute state 2 from the later state-1 full branch. |
| Destination-present harvest ticks return success without extracting or resetting the timer. | `0x0073D450`; drive-blocked retarget report | likely missing/unchecked: Rust `handle_harvest` does not explicitly gate on an active movement destination once state is `Harvest` | `src/sim/miner/miner_system.rs:513` and movement/arrival transition | If a miner is still carrying a movement destination while in harvest-continuation state, skip extraction and preserve harvest continuation for that tick. | `harvest_tick_with_destination_present_does_not_extract`: HARV/CMIN in harvest state on ore with movement target still set; tick leaves cargo and ore unchanged and does not trigger return/retarget. | Do not mine while the unit is still considered moving just because its cell coordinate equals the target cell. |
| Full cargo at harvest gate does not call `Reduce_Tiberium`; it resets timer, returns false, and caller goes to return substate before short scan. | `0x0073D450`; `0x0073E99A..0x0073EA7B` | current Rust calls `extract_bales_max` with empty capacity 0, then checks `is_full()`; observable ore unchanged, but branch ordering/timer side effects differ | `src/sim/miner/miner_system.rs:526`, `:810`, full branch after no bales | Make full-cargo harvest gate a first-class branch if timer/visual state begins to matter; at minimum keep ore unchanged and return behavior before continuation scan. | `full_miner_on_ore_returns_without_reducing_cell`: full HARV on ore at harvest gate leaves node/overlay unchanged and enters return/reservation path, no continuation target. | Do not let a full miner deplete ore or save a new nearby ore archive from a zero-capacity extraction. |
| Request amount is `ftol(min(1.0f, Storage - current_load))`: one density level per gate (`FCOMP float [0x007E2AC8]` = `1.0f` at `0x0073D569`, `FLD 1.0f` at `0x0073D576`), then `Reduce_Tiberium` clamps to cell content. | `0x0073D450` (`0x0073D556..0x0073D5A1`); `0x007E2AC8`; `0x007C5F00`; `0x00480A80` | Rust previously requested the whole free capacity (`capacity_bales - cargo.len()`, up to 40), draining a cell in one gate at ~10x native throughput; corrected in `handle_harvest` to `min(1, empty)`. | `src/sim/miner/miner_system.rs::handle_harvest`, `Miner::cargo` | Request exactly one level per successful gate; if future fractional storage is modeled, the `<= 1.0` branch truncates toward zero and requests 0 when less than one unit remains. | `harvester_takes_one_bale_per_gate_over_eleven_gates` / `harvester_clears_density_zero_overlay_without_bale_and_moves_on`: an 11-density cell yields 11 bales over 11 gates at the 19-frame cadence; the twelfth gate on the density-0 overlay clears it with no bale and the miner moves on. | Do not request the free capacity; do not round to nearest or ceil the `<= 1.0` remainder. |
| Max real overlay `OverlayData=11` can return 11, not 12, from `Reduce_Tiberium`. | `0x00480A80`; CMIN harvest density trace | mismatch: real overlay seeding uses `frame + 1`, producing 12 Rust bales for frame 11 | `src/sim/production/production_queue.rs:155`; `src/sim/miner/miner_system.rs:824` | Align harvestable density with gamemd return semantics for real overlay-backed cells. | `cmin_overlaydata_11_extracts_11_bales`: seed actual overlay frame 11, run extraction, assert 11 bales/275 ore value and overlay cleared. | Do not "fix" only direct test nodes while leaving map-overlay seeding at 12 harvestable units. |
| Gems use the same branch but storage type `Cruentus` and value 50 later. | `0x00485010`; `rulesmd.ini [Cruentus] Value=50`; `0x006C9690` | value abstraction exists; overlay-density edge likely same mismatch as ore | `ResourceType::Gem`; `extract_bales_max`; `seed_resource_nodes_from_overlays` | Keep gem cargo type/value distinct; ensure real GEM overlay frame 11 does not overharvest by one. | `cmin_gem_overlaydata_11_extracts_11_gem_bales_value_550`: seed GEM frame 11 and assert 11 gem bales at value 50. | Do not convert gems into two ore bales; stock stores one Cruentus unit worth 50 later. |
| Non-tiberium/current-empty cell returns false after timer reset; retargeting is caller-owned. | `0x0073D450`; `0x0073EA8D..0x0073EB19` | current Rust's no-bale branch short-scans/returns, broadly matching; visual state mismatch covered elsewhere | `src/sim/miner/miner_system.rs:542..586` | Preserve the separation: extraction helper returns no bales; state machine decides short scan vs return. | `empty_current_cell_short_scans_only_after_harvest_gate`: depleted current cell does not mutate storage and only then runs `TiberiumShortScan`. | Do not make `extract_bales_max` choose the next ore cell. |
| Weeder branch is excluded for standard `HARV`/`CMIN`. | `0x0073D450`; `rulesmd.ini [HARV]/[CMIN]` | Rust does not implement weeders in miner path; no standard delta | miner kind detection | No action for standard chrono/war miner fixes. | `standard_harv_and_cmin_are_not_weeder_path`: parsed rules for HARV/CMIN produce normal miner kind and no weed harvest behavior. | Do not import TS weed timing (`HarvesterLoadRate*3`) into standard ore/gem harvest. |

### Stale Docs / Follow-up Docs

- [docs/research/miner/traces/CHRONO_MINER_MISSION_HARVEST_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/traces/CHRONO_MINER_MISSION_HARVEST_TRACE.md) contains stale wording that subsequent bales happen every `HarvesterLoadRate=2` frames because the step counter is not reset. The current direct `0x0073D450` decompile shows successful standard extraction resets `param_1[0x3E]=0` and rate/step fields to `HarvesterLoadRate`. Replacement wording: "After successful standard ore/gem extraction, `Harvest_Ore_Tick` resets the StepTimer fields to a fresh `HarvesterLoadRate` cycle; tests should verify the observed gate from direct runtime or StepTimer semantics, not assume a 2-frame subsequent cadence."
- Any document or test that equates `9 * HarvesterLoadRate = 18` with a
  helper call exactly at `F+18` is stale. The ninth increment is post-mission at
  `F+18`; the helper call is `F+19` on the ordinary live path.
- Existing Rust tests around `extract_bales_max` are useful for direct resource-node levels, but they should not be treated as real overlay-frame parity tests until they seed through `seed_resource_nodes_from_overlays`.

## 10. Negative Facts / Do Not Do

- Do not harvest while `UnitClass+0x5A4` destination is present; gamemd returns success without touching ore or storage.
- Do not call `Reduce_Tiberium` when storage is already full.
- Do not treat gems as two ore bales; they are a separate tiberium type with value 50.
- Do not apply the Weeder `HarvesterLoadRate * 3` branch to `HARV` or `CMIN`.
- Do not put retarget search inside the extraction helper; gamemd retargets in `Mission_Harvest` after `Harvest_Ore_Tick` returns false.
- Do not request the whole remaining capacity per gate; the request is clamped to `1.0` before `Math__ftol` (`FCOMP 1.0f` at `0x0073D569`). Do not use `ceil` or round-to-nearest for the `<= 1.0` remainder; the conversion is `Math__ftol` truncation.
- Do not assume Rust tests that insert `remaining = 11 * 120` prove parity for real map overlay `OverlayData=11`.

## 11. Remaining Uncertainty

- Resolved (2026-09-05, direct disassembly of `0x0073D556..0x0073D5A1`): the decompiler's `Storage - total <= 1.0` comparison is `FCOMP float [0x007E2AC8]` against the constant `0x3F800000` (`1.0f`); the branch keeps the difference when `<= 1.0` and otherwise loads `1.0f`, i.e. the request is `min(1.0, Storage - total)`, one density level per gate. Earlier revisions of this document stated the request was the full truncated remaining capacity; that was wrong.
- Runtime frame capture was not performed. Static live disassembly now proves
  the ordinary AI-order boundary and exact `F+19` helper observation. A
  debugger trace could corroborate it but is no longer load-bearing for this
  static mechanism claim.
- Pip rendering is still not proven. The cargo amount findings strongly imply visible pip differences for 11/20 vs 12/20 if gamemd uses proportional five-pip flooring, but that remains a separate rendering investigation.

## Sources

- Ghidra MCP, read-only:
  - `decompile_function 0x0073D450` - `UnitClass::Harvest_Ore_Tick`
  - `decompile_function 0x0073E5E0` - `UnitClass::Mission_Harvest`
  - `get_function_xrefs 0x0073D450`
  - `decompile_function 0x007414A0` - `UnitClass::Get_Storage_Percentage`
  - `decompile_function 0x006C9650` - `StorageClass::GetTotalAmount`
  - `decompile_function 0x006C9690` - `StorageClass::AddAmount`
  - `decompile_function 0x00485010` - `CellClass::GetTiberiumType`
  - `decompile_function 0x00480A80` - `CellClass::Reduce_Tiberium`
  - `decompile_function 0x00486E30` - weed reduction helper
  - `decompile_function 0x007C5F00` - `Math__ftol`
- Prior docs read:
  - `docs/research/miner/HARVESTER_MISSION_HARVEST_GHIDRA_REPORT.md`
  - [docs/research/miner/HARV_HARVEST_STATE_RETARGET_VISUAL_FLAG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARV_HARVEST_STATE_RETARGET_VISUAL_FLAG_GHIDRA_REPORT.md)
  - [docs/research/CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md)
  - [docs/research/TIBERIUM_QUEUE_SEEDING_AND_TIMING_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBERIUM_QUEUE_SEEDING_AND_TIMING_REPORT.md)
  - [docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md)
  - [docs/research/miner/DRIVE_BLOCKED_DELAY_EXPIRY_MINER_RETARGET_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DRIVE_BLOCKED_DELAY_EXPIRY_MINER_RETARGET_GHIDRA_REPORT.md)
  - [docs/research/traces/CMIN_HARVEST_DENSITY_CARGO_REDUCE_TIBERIUM_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/CMIN_HARVEST_DENSITY_CARGO_REDUCE_TIBERIUM_TRACE.md)
  - [docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md)
  - `docs/research/RULESCLASS_FIELDS.csv`
- INI checked:
  - `ini/rulesmd.ini`
  - `ini/rules.ini`
- Rust scanned read-only:
  - `src/sim/miner/miner_system.rs`
  - `src/sim/miner/mod.rs`
  - `src/sim/miner/miner_tests.rs`
  - `src/sim/production/production_queue.rs`
