# House Place Production Blocked War Factory Exit - Ghidra Research Report

**Address(es):** `0x004FB0E0` (`HouseClass::Place_Production`), `0x00443C60` (`BuildingClass::ExitObject_Main`), `0x0044F640` (`BuildingClass::GetExitCoord`), `0x004FAA10` (production cleanup/restart helper), `0x004CA1A0` (`FactoryClass::CompletedProduction`)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** Completed non-naval vehicle delivery from a stock land war factory when the initial `ExitCoord` unlimbo is blocked or not accepted.
**Non-Scope:** Naval yards, refineries, full sidebar delivery timing, full build queue economics, and the complete post-spawn war-factory door mission.
**Confidence:** High for blocked vehicle ordering after parent reconciliation spot-check; High for stock land WF `GetExitCoord` branch after cross-checking sibling slot 4; Medium for exact player-facing retry/UI presentation without runtime tracing.
**Active in YR:** Yes. Stock `GAWEAP`, `NAWEAP`, and `YAWEAP` have `WeaponsFactory=yes`, `Factory=UnitType`, no `Naval=yes`, and `ExitCoord=512,256,0` in `ini/rulesmd.ini`.

## 1. Overview

For stock land war factories, the initial completed-vehicle delivery path does **not** use `BuildingClass::GetDockCellForObject`; sibling slot 4 independently verified that it branches to `GetExitCoord @ 0x0044F640` and attempts `Unlimbo` at `ExitCoord=512,256,0` relative to the factory. If that `Unlimbo` fails, `ExitObject_Main` returns `0`.

`HouseClass::Place_Production` treats that return as delivery failure, but the vehicle failure branch is narrower than the original swarm-slot draft claimed. A parent reconciliation spot-check of `0x004FB0E0` shows the failed object is queried with `WhatAmI()`: if it is a vehicle (`6`), the function returns `0` without calling `FactoryClass::CompletedProduction`, without logging, without calling `FUN_004FAA10`, and without starting the next queued object. The completed vehicle therefore remains attached to the factory for later delivery.

The log plus `FUN_004FAA10` cleanup path exists only for the non-vehicle failure branch in this slice (`WhatAmI() != 6`). It must not be applied to stock land war-factory vehicle delivery.

## 2. Class Layout / Key Offsets

| Owner | Offset / field | Purpose in this slice | Evidence | Active in YR |
|---|---:|---|---|---|
| `HouseClass` | `+0x53B4` | Primary vehicle factory pointer selected by `FUN_004FAA10` for non-naval vehicle production | `FUN_004FAA10` switch | Yes |
| `HouseClass` | `+0x53B8` | Primary ship factory pointer when the naval argument is nonzero | `FUN_004FAA10` switch | Conditional; naval out of scope |
| `HouseClass` | `+0x5650` | Primary vehicle building reset by successful `ExitObject` vehicle setup | `ExitObject_Main @ 0x00444126` area | Yes |
| `BuildingClass` | `Type + 0x16BD` | `WeaponsFactory` gate for stock WF branch and later mission | `ExitObject_Main`, `FUN_0044D880`, stock `rulesmd.ini` WFs | Yes |
| `BuildingTypeClass` | `+0xCCE` | `Naval=yes`; stock land WFs have this false and therefore bypass naval WF dock search | `ExitObject_Main`; slot 4 report | Yes, false for stock land WFs |
| `BuildingTypeClass` | `+0xEC8/+0xECC/+0xED0` | `ExitCoord=512,256,0`; stock land WF initial unlimbo coordinate source | `GetExitCoord @ 0x0044F640`; `rulesmd.ini` | Yes |
| `BuildingTypeClass` | `+0xED4` | `ExitList`; used by later WF mission/bib logic, not initial stock land WF spawn | `FUN_0044D880`, `ClearBibArea @ 0x00449540`; slot 4 report | Yes |
| `FactoryClass` | `Object` | Completed limbo object; remains non-null after build completion until success/cancel cleanup, and stays non-null on blocked vehicle failure | `FactoryClass::AI`, `CompletedProduction`, `AbandonProduction` | Yes |
| `FactoryClass` | `Production_Value` | Completion sentinel `0x36` | `FactoryClass::IsComplete @ 0x004CA130` | Yes |
| `FactoryClass` | `QueuedObjects_Count` / `QueuedObjects_Items` | Source for successful-completion/cancel queue restart; unchanged by blocked vehicle failure | `FactoryClass::StartNextQueued @ 0x004CA5A0` | Yes |

## 3. Core Logic

### 3.1 Successful auto-exit ordering

For auto-exit production, `Place_Production`:

1. Selects the primary factory by RTTI/category and naval flag.
2. Requires `FactoryClass::IsComplete` true: `Object != NULL` and `Production_Value == 0x36`.
3. Fetches the produced object via `FactoryClass::GetObject`.
4. Finds the producing building through the produced object's factory lookup.
5. Calls the producing building vtable `+0x100`, `ExitObject`.
6. Accepts return `2`, or return `1` only for a building-object special case.
7. Only after accepted return, calls `FactoryClass::CompletedProduction`.
8. Calls `FUN_004FAA10` for queue/sidebar bookkeeping.
9. Records last-built and plays completion sound if applicable.

Evidence: `0x004FB563` calls vtable `+0x100`; `0x004FB569..0x004FB589` checks return; `0x004FB649` calls `CompletedProduction`; `0x004FB663` calls `FUN_004FAA10`. Active in YR: Yes.

### 3.2 Failed auto-exit ordering

If `ExitObject` returns failure:

1. `Place_Production` checks the produced object's `WhatAmI()`.
2. If the produced object is a vehicle (`WhatAmI() == 6`), it returns `0` immediately.
3. The vehicle branch does not call `FactoryClass::CompletedProduction`.
4. The vehicle branch does not log `"Failed to exit object from factory"`.
5. The vehicle branch does not call `FUN_004FAA10`, `AbandonProduction`, or `StartNextQueued`.
6. If the produced object is not a vehicle, the non-vehicle branch logs the failure and calls `FUN_004FAA10`.

Evidence: parent reconciliation spot-check of failure branch `0x004FB589..0x004FB5B5`, including the `WhatAmI() == 6` skip around the log/helper call; success-only `CompletedProduction` calls at `0x004FB2A1` and `0x004FB649`. Active in YR: Yes.

### 3.3 Stock land WF rejection point

For `WeaponsFactory=yes` and `Naval=no`, `ExitObject_Main` takes the stock land WF path:

1. Calls vtable `+0xB4`, `GetExitCoord @ 0x0044F640`.
2. Uses facing byte `0x40`.
3. Calls the produced unit's `Unlimbo` at the returned coordinate.
4. If `Unlimbo` returns false, jumps to failure cleanup inside `ExitObject_Main`, decrements the map-editor guard, returns `0`, and feeds the `Place_Production` failure branch.
5. On success, temporarily mark/unmark, reasserts location from `GetExitCoord`, sends radio commands `2` and `0x18`, queues building mission `0x10`, and returns `2`.

Evidence: `ExitObject_Main @ 0x00444583..0x00444594`; failure return `0x00444EDE..0x00444EF5`; `GetExitCoord @ 0x0044F640`. Active in YR: Yes.

`GetExitCoord` itself returns building coord plus `Type+0xEC8/+0xECC/+0xED0`; stock land WFs set `512,256,0`, so the initial cell is factory NW+(2,1). If `ExitCoord` were invalid, the helper would fall back to building center, but stock WFs do not take that fallback.

### 3.4 Cleanup and queue restart boundary

`FUN_004FAA10` is still the production cleanup/restart helper, but it is not reached by a blocked stock land war-factory vehicle exit. For normal completed-item commands with `heapId = -1`, successful `Place_Production` calls `CompletedProduction` first, then `FUN_004FAA10`, and the helper can reach `FactoryClass::StartNextQueued` if the queue is non-empty.

For a blocked stock land war-factory vehicle:

1. `ExitObject_Main` returns `0`.
2. `Place_Production` sees produced `WhatAmI() == 6`.
3. `Place_Production` returns `0` before the log/helper block.
4. `FactoryClass::Object`, `Production_Value == 0x36`, and the queued list remain intact.

The `AbandonProduction` path is relevant to cancel/remove behavior and the non-vehicle failure branch, not to the vehicle-blocked `ExitCoord` case corrected here.

Evidence: `HouseClass::Place_Production @ 0x004FB0E0`; `FUN_004FAA10 @ 0x004FAA10`; `FactoryClass::StartNextQueued @ 0x004CA5A0`; sibling [STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md). Active in YR: Yes.

### 3.5 Later WF mission is not pre-placement preservation

`FUN_0044D880` is the post-success war-factory mission handler. Its `ClearBibArea` state can scatter blockers and retry the door/drive-out state after a unit already exists on the map. It is not responsible for the blocked initial `ExitCoord` preservation; that comes earlier from `Place_Production` returning before vehicle cleanup.

Evidence: `FUN_0044D880 @ 0x0044DE27`; `BuildingClass::ClearBibArea @ 0x00449540`. Active in YR: Yes.

## 4. INI Keys

| Key / section | Stock YR value | Effect for this slice | Evidence | Active in YR |
|---|---|---|---|---|
| `[GAWEAP] WeaponsFactory` | `yes` | Selects WF branch via `Type+0x16BD` | `ini/rulesmd.ini:11775` | Yes |
| `[NAWEAP] WeaponsFactory` | `yes` | Same for Soviet WF | `ini/rulesmd.ini:12565` | Yes |
| `[YAWEAP] WeaponsFactory` | `yes` | Same for Yuri WF | `ini/rulesmd.ini:13309` | Yes |
| `[GAWEAP]/[NAWEAP]/[YAWEAP] Factory` | `UnitType` | Vehicle factory category | `ini/rulesmd.ini:11777`, `12567`, `13311` | Yes |
| `[GAWEAP]/[NAWEAP]/[YAWEAP] ExitCoord` | `512,256,0` | Actual stock land WF initial unlimbo coordinate | `ini/rulesmd.ini:11800`, `12594`, `13335`; `0x0044F640` | Yes |
| `[GAYARD]/[NAYARD]/[YAYARD] Naval` | `yes` | Splits naval production away from this slice | `ini/rulesmd.ini:11854`, `12642`, `13392` | Out of scope |
| WF art bib/door keys | `BibShape`, `DeployingAnim`, `UnderDoorAnim`, etc. | Later visual/door mission support, not blocked initial unlimbo preservation | `ini/artmd.ini:1214..1438`; `0x0044D880` | Yes for visuals |

No INI key in this slice changes the blocked vehicle branch. The completed vehicle remains ready inside the factory because of `Place_Production` control flow, not because of an INI option.

## 5. Integration Points

| Direction | Function | Role | Evidence |
|---|---|---|---|
| Caller | `EventClass::Execute @ 0x004C6CB0` | Dispatches production placement command into `Place_Production` | Ghidra callers for `0x004FB0E0` |
| Primary | `HouseClass::Place_Production @ 0x004FB0E0` | Delivery commit; success calls `CompletedProduction`; blocked vehicle failure returns before cleanup helper | decompile + assembly spot-check |
| Callee | `BuildingClass::ExitObject_Main @ 0x00443C60` | Attempts stock land WF `GetExitCoord` unlimbo; returns `0` on failure | `0x00444583..0x00444594` |
| Callee | `BuildingClass::GetExitCoord @ 0x0044F640` | Supplies `ExitCoord=512,256,0` coordinate | decompile |
| Callee | `FactoryClass::CompletedProduction @ 0x004CA1A0` | Clears object only on accepted success path | `0x004FB649` |
| Callee | `FUN_004FAA10 @ 0x004FAA10` | Success/cancel cleanup and queue restart helper; not reached by blocked vehicle failure | helper body; parent reconciliation |
| Callee | `FactoryClass::AbandonProduction @ 0x004C9FF0` | Cancel/non-vehicle cleanup path; not reached by blocked vehicle failure | helper body; parent reconciliation |
| Callee | `FactoryClass::StartNextQueued @ 0x004CA5A0` | Pops queued entry 0 and calls `HouseClass::Begin_Production` after successful completion/cancel paths; not reached by blocked vehicle failure | helper body |

Tick-cycle context: `FactoryClass::AI @ 0x004C9B20` sets `IsSuspended = true` and timer duration/time-left to zero when `Production_Value` reaches `0x36`, but leaves `Object` for later delivery.

## 6. Current Rust Implementation Status

| Surface | Current behavior | Binary delta |
|---|---|---|
| `src/sim/production/production_queue.rs:410` | `tick_production` advances and delivers completed items in one loop | gamemd separates completion from delivery command; blocked vehicle failure remains pending |
| `src/sim/production/production_queue.rs:465` | Pops the completed queue item before spawn validation | gamemd keeps `FactoryClass::Object` until success; blocked vehicle failure does not pop it |
| `src/sim/production/production_queue.rs:512` | If no spawn cell, refunds object cost and continues | gamemd blocked vehicle failure keeps the completed factory object pending; no refund and no queue advance |
| `src/sim/production/production_spawn.rs:19` / `:112` | Tries preferred factories and fallback structures | gamemd delivery is tied to the produced object's factory/primary building path |
| `src/sim/production/production_spawn.rs:161` | Falls back to `nearest_walkable_around` within radius 12 | stock land WF attempts `Unlimbo` at `GetExitCoord`; no nearest-cell rescue before cleanup |
| `src/sim/production/production_spawn.rs:355` | Generates eight neighbor fallback cells around `ExitCoord` | stock land WF does not probe neighboring cells for initial production unlimbo |
| `src/sim/production/production_placement_tests.rs:933` | Test expects fallback to next factory when first exit is blocked | opposite of verified blocked active-factory cleanup behavior |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `HouseClass::Place_Production` auto-exit success branch | verified | `0x004FB563`, `0x004FB649`, `0x004FB663` | none |
| `HouseClass::Place_Production` auto-exit vehicle failure branch | verified | `0x004FB589..0x004FB5B5`; parent reconciliation | runtime retry/UI presentation |
| `FactoryClass::CompletedProduction` effects | verified | `0x004CA1A0` | none |
| `FUN_004FAA10` success/cancel cleanup/restart | verified | `0x004FAA10`, `0x004FAB3D..0x004FABA6` | not reached by blocked vehicle failure |
| `FactoryClass::AbandonProduction` refund/object clear | verified | `0x004C9FF0` | cancel/non-vehicle cleanup only for this slice |
| `FactoryClass::StartNextQueued` queue restart | verified | `0x004CA5A0` | not reached by blocked vehicle failure |
| Stock non-naval WF `GetExitCoord` unlimbo path | verified | `0x00444583..0x00444594`, `0x0044F640`; slot 4 report | none |
| Later WF mission `ClearBibArea` scatter/retry | touched-not-exhausted | `0x0044D880`, `0x00449540` | full door mission outside scope |
| Naval WF / naval yard dock search | deferred | scope constraint | slot not about naval production |
| Sidebar delivery timing globals | deferred | parent slot 5 | not needed to answer cleanup ordering |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-1 - Is `Place_Production` active in YR for production delivery? -> Yes, caller is `EventClass::Execute`; stock WFs are `WeaponsFactory=yes` / `Factory=UnitType`.` (evidence: callers for `0x004FB0E0`; `ini/rulesmd.ini`)
- `[RESOLVED] OQ-2 - Does blocked vehicle auto-exit call `CompletedProduction`? -> No; the vehicle failure branch returns before the success-only `CompletedProduction` call.` (evidence: `0x004FB589..0x004FB5B5`, `0x004FB649`)
- `[RESOLVED] OQ-3 - What return values from `ExitObject` count as success? -> Return `2` succeeds; return `1` only succeeds for a building-object special case; stock WF unlimbo failure returns `0`.` (evidence: `0x004FB569..0x004FB589`)
- `[RESOLVED] OQ-4 - Does the failed vehicle factory object remain complete for retry? -> Yes; the `WhatAmI() == 6` branch skips cleanup and leaves the completed object pending.` (evidence: `0x004FB589..0x004FB5B5`; parent reconciliation)
- `[RESOLVED] OQ-5 - Does queue restart happen after blocked vehicle exit? -> No; the vehicle failure branch does not reach `FUN_004FAA10` or `FactoryClass::StartNextQueued`.` (evidence: `0x004FB589..0x004FB5B5`, `0x004FAA10`, `0x004CA5A0`)
- `[RESOLVED] OQ-6 - Is the blocked vehicle outcome only a log/no-op? -> No log either; it is a failed delivery return with the completed object still pending.` (evidence: `0x004FB589..0x004FB5B5`)
- `[RESOLVED] OQ-7 - What is the stock land WF exit coordinate source? -> `GetExitCoord`, adding `ExitCoord=512,256,0`; not `GetDockCellForObject`.` (evidence: `0x00444583`, `0x0044F640`, sibling slot 4 report)
- `[RESOLVED] OQ-8 - What if the initial WF exit coordinate is blocked/not accepted by unlimbo? -> `ExitObject` returns `0`, and `Place_Production` returns failure while keeping the completed vehicle pending.` (evidence: `0x0044458C..0x00444594`, `0x00444EDE..0x00444EF5`, parent reconciliation)
- `[RESOLVED] OQ-9 - Does `FactoryClass::AI` clear completed object before placement? -> No; at progress `0x36` it suspends/zeros timer and leaves `Object` for delivery.` (evidence: `0x004C9B20`)
- `[RESOLVED] OQ-10 - Does `CompletedProduction` itself restart the queue? -> No; it clears object/special item, suspends, marks changed, zeros progress/timer.` (evidence: `0x004CA1A0`)
- `[RESOLVED] OQ-11 - Which current Rust behavior is directly opposite? -> no-spawn/refund/continue and next-factory fallback both conflict with the pending completed-vehicle behavior.` (evidence: `src/sim/production/production_queue.rs`, `src/sim/production/production_placement_tests.rs:933`)
- `[RESOLVED] OQ-12 - Are `QueueingCell` / refinery dock rules involved? -> No for stock land WF initial production exit or cleanup.` (evidence: `0x004FB0E0`, `0x00443C60`, `0x0044F640`; slot 4 report)
- `[DEFERRED] OQ-13 - Exact live-player EVA/retry presentation on a blocked-exit retail scenario` (category: `needs-runtime-debugger`; reason: static path proves no cleanup/restart for vehicles, but not exact perceived UI ordering; next-step-if-pursued: trace a retail blocked-WF scenario through event execution)
- `[DEFERRED] OQ-14 - Exact stock WF `ExitList[10]` value for later bib/drive-out` (category: `out-of-scope`; reason: slot 4 bounded initial branch choice and left exact table bytes separate; next-step-if-pursued: recover runtime-populated foundation table)
- `[DEFERRED] OQ-15 - Full `StripClass::AI` auto-delivery timing` (category: `out-of-scope`; reason: assigned to swarm slot 5; next-step-if-pursued: use `STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART`)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Blocked stock land WF initial `ExitCoord` unlimbo fails delivery, skips `CompletedProduction`, and keeps the completed vehicle pending | `0x00444583..0x00444594`; `0x004FB589..0x004FB5B5`; parent reconciliation | mismatch: Rust pops/refunds before validated delivery | `src/sim/production/production_queue.rs:410`, `:465`, `:512` | A completed vehicle whose producing WF cannot unlimbo at `ExitCoord` must remain the completed factory object; the queue must not advance | Build MTNK then queue HTNK; block GAWEAP `ExitCoord` cell; delivery attempt keeps MTNK ready and HTNK does not start | Do not abandon/refund/pop/start next; proposed test `test_blocked_war_factory_exit_keeps_completed_vehicle_and_holds_queue` |
| Stock land WF initial delivery does not fall back to another factory or arbitrary nearest walkable cell | `0x004FB54C` factory lookup; `0x004FB563` `ExitObject`; `0x00444583..0x00444594`; failure return `0x004FB589..0x004FB5B5` | mismatch: Rust searches ordered bases and nearest walkable fallback | `src/sim/production/production_spawn.rs:19`, `:112`, `:161`; `production_placement_tests.rs:933` | Delivery must be tied to the producing factory's `ExitCoord` unlimbo result | Two GAWEAPs, active/producing first blocked, second clear; completed tank must not spawn from second factory | Do not rotate to another factory after a product is already complete; proposed test `test_blocked_active_war_factory_does_not_spawn_from_second_factory` |
| `CompletedProduction` is success-only for non-building auto-exit and clears `FactoryClass::Object`; blocked vehicle failure is a pending-state return | success `0x004FB649`; vehicle failure `0x004FB589..0x004FB5B5`; `0x004CA1A0` | mismatch/unchecked: Rust lacks explicit success-vs-pending completed-object state | `src/sim/production/production_types.rs`, `production_queue.rs` | Model success consumption separately from blocked pending failure | Same completed state, one clear exit and one blocked exit; clear spawns/emits complete, blocked remains ready with no spawned unit | Do not use same queue-pop path for success and blocked failure; proposed test `test_completed_production_only_consumed_after_successful_factory_exit` |

### Negative Facts / Do Not Do

- Do implement blocked WF initial vehicle exit as "completed vehicle remains pending for later successful delivery"; the vehicle failure branch skips `FUN_004FAA10` and `FactoryClass::AbandonProduction`. Evidence: `0x004FB589..0x004FB5B5`.
- Do not abandon, refund, pop, or start next queued production on blocked stock land WF vehicle exit. Evidence: `0x004FB589..0x004FB5B5`, `0x004FAA10`, `0x004CA5A0`.
- Do not call `CompletedProduction` before accepted `ExitObject` success for vehicles. Evidence: `0x004FB563..0x004FB649`.
- Do not use `GetDockCellForObject` as the stock land WF initial spawn oracle; use `GetExitCoord=512,256,0`, then `Unlimbo` success/failure. Evidence: `0x00444583`, `0x0044F640`; sibling slot 4 report.
- Do not fall back to another war factory after the producing factory's `ExitCoord` unlimbo is blocked. Evidence: `0x004FB54C`, `0x004FB563`, `0x004FB589..0x004FB5B5`.
- Do not use `QueueingCell`, `DockingOffset`, or refinery queue semantics for this path. Evidence: no reads in `0x004FB0E0`, `0x00443C60`, or `0x0044F640`; slot 4 report.
- Do not treat `ClearBibArea` as proof that initial `ExitCoord` unlimbo failures retry forever; it is post-success door mission logic. Evidence: `0x0044D880`, `0x00449540`.

### Remaining Uncertainty

- Runtime-only: exact voice/UI presentation when a human player's completed vehicle delivery is blocked and remains pending.
- Out of scope: exact runtime value of stock 5x3 `ExitList[10]` for later bib/drive-out.
- Delegated to slot 5: exact sidebar auto-delivery timing and whether UI submits another command after blocked delivery failure.

### Stale Docs / Follow-up Docs

- [docs/research/timing/unit-build-time.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/unit-build-time.md): patched 2026-05-21 to describe pending vehicle globals, successful `Place_Production` restart, and blocked WF vehicle pending behavior.
- [docs/research/BUILDINGCLASS_OPEN_QUESTIONS_VERIFICATION_R3.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_OPEN_QUESTIONS_VERIFICATION_R3.md): no correction needed for the claim that stock WF ground exit does not call `GetDockCellForObject`; sibling slot 4 corroborates it. If edited, narrow wording to say stock land WF initial unlimbo uses `GetExitCoord=512,256,0`, while later door/bib state uses `ExitList+0x28`.

## Sources

- Ghidra decompile / assembly spot-checks:
  - `HouseClass::Place_Production @ 0x004FB0E0`
  - `BuildingClass::ExitObject_Main @ 0x00443C60`
  - `BuildingClass::GetExitCoord @ 0x0044F640`
  - `FUN_004FAA10 @ 0x004FAA10`
  - `FactoryClass::CompletedProduction @ 0x004CA1A0`
  - `FactoryClass::AbandonProduction @ 0x004C9FF0`
  - `FactoryClass::StartNextQueued @ 0x004CA5A0`
  - `FactoryClass::AI @ 0x004C9B20`
  - `BuildingClass::ClearBibArea @ 0x00449540`
  - `FUN_0044D880 @ 0x0044D880`
- Prior docs checked:
  - [docs/research/RALLY_POINTS_AND_UNIT_SPAWNING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RALLY_POINTS_AND_UNIT_SPAWNING.md)
  - [docs/research/timing/unit-build-time.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/unit-build-time.md)
  - [docs/research/BUILDINGCLASS_OPEN_QUESTIONS_VERIFICATION_R3.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_OPEN_QUESTIONS_VERIFICATION_R3.md)
  - [docs/research/SCATTER_ALL_CALLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SCATTER_ALL_CALLERS_GHIDRA_REPORT.md)
  - [docs/research/BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md)
- INI files checked:
  - `ini/rulesmd.ini`
  - `ini/artmd.ini`
  - `ini/rules.ini`
  - `ini/art.ini`
- Rust surfaces scanned:
  - `src/sim/production/production_queue.rs`
  - `src/sim/production/production_spawn.rs`
  - `src/sim/production/production_placement_tests.rs`
  - `src/sim/production/production_tech.rs`
  - `src/sim/production/mod.rs`
