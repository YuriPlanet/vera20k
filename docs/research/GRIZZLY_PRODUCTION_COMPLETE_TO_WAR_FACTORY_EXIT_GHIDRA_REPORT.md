# Grizzly Production Complete To War Factory Exit - Ghidra Research Report

**Address(es):** `0x004C9B20` (`FactoryClass::AI` settled input), `0x006A8B30` (`StripClass::AI`), `0x00734250` (vehicle delivery global setter), `0x004C6CB0` (`EventClass::Execute`), `0x004FB0E0` (`HouseClass::Place_Production`), `0x00443C60` (`BuildingClass::ExitObject_Main`), `0x004CA1A0` (`FactoryClass::CompletedProduction`), `0x004FAA10` (production cleanup/restart helper), `0x004CA5A0` (`FactoryClass::StartNextQueued`)
**Investigation Mode:** swarm-slot exhaustive slice
**Claimed Scope:** stock MTNK/Grizzly delivery after `FactoryClass::AI` has already reached `Production_Value == 54`: completed vehicle delivery dispatch, initial war factory unlimbo, blocked exit behavior, queue restart boundary, and Rust-facing handoff.
**Non-Scope:** factory build step math before completion, naval yard delivery, exact 5x3 `ExitList[10]` value, full war-factory door animation state machine, and runtime UI/EVA frame capture.
**Confidence:** High for static control-flow ordering and stock land war-factory path; Medium for exact player-facing retry/UI timing when a blocked completed vehicle remains pending.
**Active in YR:** Yes. Stock `MTNK` is a `VehicleType` produced by stock land war factories (`GAWEAP`, `NAWEAP`, `YAWEAP`), whose `rulesmd.ini` entries set `WeaponsFactory=yes`, `Factory=UnitType`, `ExitCoord=512,256,0`, `NumberImpassableRows=1`, and do not set `Naval=yes`.

## Working Notes Gate

- **Target question:** After stock MTNK factory-complete state, what path delivers the produced Grizzly, what happens if the war-factory exit is blocked, when does the next queued item start, and what must Rust preserve?
- **Non-goals:** Do not re-investigate `661 -> 12 -> 648` build cadence, do not investigate naval delivery, do not recover `ExitList[10]`, and do not implement Rust.
- **Evidence needed to mark COMPLETE:** decompile plus assembly/context for `Place_Production` success/failure ordering; decompile plus assembly/context for stock WF `ExitObject` unlimbo; caller evidence for `Place_Production`; INI evidence for stock land WF activity; focused Rust scan.
- **Stop conditions:** stop if Ghidra read-only access is unavailable, if the path expands into full sidebar/UI mechanics, or if the stock land WF path cannot be distinguished from naval/TS legacy paths.

## Summary

The completed MTNK does not become a normal spawned unit at the moment `FactoryClass::AI` reaches `Production_Value == 54`. That prior step leaves a completed factory object for later delivery. `StripClass::AI` notices changed/complete factories; for vehicles it plays ready EVA and stores the produced unit pointer in a delivery global instead of queuing the same `Place_Production` command used for buildings/infantry/aircraft.

The actual land war-factory delivery commit runs through `HouseClass::Place_Production`. It asks the producing building to `ExitObject`; for stock non-naval weapons factories, `BuildingClass::ExitObject_Main` uses `GetExitCoord` and attempts `Unlimbo` at `ExitCoord=512,256,0` with facing byte `0x40`. If this succeeds, `Place_Production` then calls `FactoryClass::CompletedProduction`, then `FUN_004FAA10`, which can call `FactoryClass::StartNextQueued` in the same command execution.

If the stock land war-factory exit/unlimbo fails for a vehicle, `Place_Production` checks `WhatAmI() == 6` and returns `0` before `CompletedProduction`, before `FUN_004FAA10`, and before `StartNextQueued`. The completed Grizzly remains pending in the factory. There is no refund, no queue pop, and no next queued vehicle start from this blocked delivery attempt.

## Verified Findings

### 1. Vehicle completion is surfaced by sidebar delivery globals, not immediate spawn

`StripClass::AI @ 0x006A8B30` polls cameo entries, calls `FactoryClass::HasChanged`, `FactoryClass::IsComplete`, and `FactoryClass::GetObject`. For produced objects with `WhatAmI() == 6`, it plays EVA and calls `FUN_00734250`, while building/infantry/aircraft cases build command `0x0B`.

**Active in YR:** Yes. This is live sidebar strip AI for completed factories. Evidence: decompile `0x006A8B30`; assembly context `0x006A8DC6..0x006A8E48` shows factory pointer poll and vehicle case branch; `FUN_00734250 @ 0x00734250`.

`FUN_00734250` reads produced unit type at `produced+0x520`, checks `type+0xE08 == 5`, and writes either naval pending global `DAT_00B0FE60` or non-naval pending global `DAT_00B0FE5C`.

**Active in YR:** Yes for stock land Grizzly via the non-naval slot. Evidence: decompile `0x00734250`; assembly `0x00734250..0x0073426C` (`CMP [EAX+0xE08],0x5`, write `0x00B0FE5C` on non-naval).

### 2. `Place_Production` is the delivery commit and is called from command execution

`EventClass::Execute @ 0x004C6CB0` is the direct caller of `HouseClass::Place_Production @ 0x004FB0E0` for production placement command `0x0B`.

**Active in YR:** Yes. Evidence: Ghidra caller query for `0x004FB0E0` returns `EventClass__Execute @ 0x004C6CB0`; assembly context at `0x004C710B` shows event fields pushed and `CALL 0x004FB0E0`.

On the invalid-cell auto-exit path used for completed vehicles, `Place_Production` fetches the complete factory object, finds the producing building, calls building vtable `+0x100` (`ExitObject`), and only consumes production after an accepted return.

**Active in YR:** Yes. Evidence: decompile `0x004FB0E0`; assembly context `0x004FB560..0x004FB663` shows `CALL [EDX+0x100]`, accepted return checks, then success-only `CALL 0x004CA1A0` and `CALL 0x004FAA10`.

### 3. Stock land war factories use `ExitCoord`, not a fallback cell search

For `WeaponsFactory=yes` and `Naval=no`, `BuildingClass::ExitObject_Main @ 0x00443C60` calls building vtable `+0xB4` (`GetExitCoord`), then calls produced unit vtable `+0xD8` (`Unlimbo`) with facing byte `0x40`. Failure jumps to the common failure return; success sets location/contact/mission state.

**Active in YR:** Yes for `GAWEAP`, `NAWEAP`, `YAWEAP`. Evidence: decompile `0x00443C60`; assembly context `0x00444583..0x00444594` shows `CALL [EDX+0xB4]`, `CALL [EBP+0xD8]`, `TEST AL,AL`, `JZ 0x00444EDE`; `rulesmd.ini` lines `11775/11777/11800/11804`, `12565/12567/12594/12598`, `13309/13311/13335/13339`.

Existing sibling reports independently verify `GetExitCoord @ 0x0044F640` adds `Type+0xEC8/+0xECC/+0xED0`; stock WFs use `ExitCoord=512,256,0`, i.e. NW+(2,1) in cells.

**Active in YR:** Yes. Evidence: [BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md); stock `rulesmd.ini` entries above.

### 4. Blocked vehicle exit preserves the completed object and does not restart the queue

If `ExitObject` fails, `Place_Production` checks the produced object's `WhatAmI()`. When it is a vehicle (`6`), it returns before the log/helper block. Therefore it does not call `FactoryClass::CompletedProduction`, `FUN_004FAA10`, `FactoryClass::AbandonProduction`, or `FactoryClass::StartNextQueued`.

**Active in YR:** Yes for blocked stock Grizzly delivery. Evidence: decompile `0x004FB0E0`; assembly context `0x004FB589..0x004FB5BA` shows `CALL [EAX+0x2C]`, `CMP EAX,0x6`, `JZ 0x004FB5BA`, while success-only `CALL 0x004CA1A0` is at `0x004FB64B` and `CALL 0x004FAA10` at `0x004FB663`.

`FactoryClass::CompletedProduction @ 0x004CA1A0` clears `Object`, sets suspended/changed, zeros `Production_Value`, and clears the production timer, but it only runs after accepted delivery.

**Active in YR:** Yes. Evidence: decompile `0x004CA1A0`; caller ordering in `Place_Production` above.

### 5. Queue restart is success/cancel cleanup, not factory-complete itself

`FUN_004FAA10 @ 0x004FAA10` can call `FactoryClass::AbandonProduction`, then, if `QueuedObjects_Count != 0`, `FactoryClass::StartNextQueued @ 0x004CA5A0`. `StartNextQueued` requires a queued object, no current `Object`, and timer/suspended state, then removes queue front and calls `HouseClass::Begin_Production`.

**Active in YR:** Yes. Evidence: decompile `0x004FAA10` and `0x004CA5A0`; assembly context `0x004FABA4..0x004FABB2` shows `CALL 0x004C9FF0`, queue count compare, and nonzero branch to the restart tail; `0x004CA5A0..0x004CA5B0` checks queued count and current object.

For blocked Grizzly exit, this helper is not reached, so the next queued vehicle waits until a later successful delivery/cleanup path.

**Active in YR:** Yes. Evidence: vehicle failure branch `0x004FB589..0x004FB5BA` skips the success block containing `0x004FB64B` and `0x004FB663`.

## Rust Reconnaissance

Current Rust has already corrected some older spawn-cell deltas:

- `src/sim/production/production_spawn.rs:101..105` routes stock land vehicles through `find_exact_exitcoord_spawn_cell`.
- `src/sim/production/production_placement_tests.rs:1068` pins "blocked active war factory does not spawn from second factory".
- `src/sim/production/production_placement_tests.rs:1105` pins "blocked ExitCoord must fail initial war-factory delivery instead of probing neighboring cells".
- `src/sim/production/production_queue.rs:522..525` keeps a completed vehicle pending when no spawn cell is available (`continue`), instead of refunding/popping.

Remaining Rust deltas:

- `src/sim/production/production_queue.rs:450..535` still advances completion and delivery from `tick_production`; there is no explicit command-stage `Place_Production` equivalent or pending land/naval delivery global equivalent.
- `BuildQueueState::Done` approximates a completed factory object, but it is not tied to a produced limbo object pointer, a specific factory object, or sidebar delivery globals.
- `mark_war_factory_spawn_contact` exists in `production_spawn.rs:156`, which is good for the post-success contact requirement, but this report did not verify the later mission/contact-clear lifecycle.

## Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Acceptance scenario | Proposed test name | Risk |
|---|---|---|---|---|---|---|
| Blocked stock land WF vehicle exit returns before `CompletedProduction` and queue restart | `0x004FB589..0x004FB5BA`; success calls at `0x004FB64B/0x004FB663` | mostly fixed for no spawn (`continue`), but needs explicit pending-complete semantics across retries/UI | `src/sim/production/production_queue.rs`, `production_types.rs` | Build MTNK, queue second vehicle, block `GAWEAP` `ExitCoord`; MTNK remains completed/pending and second vehicle does not start | `blocked_grizzly_exit_keeps_completed_vehicle_and_holds_queue` | Do not refund, pop, or start next item on blocked vehicle exit |
| Successful delivery consumes completed object, then cleanup can restart next queue in the same command execution | `0x004FB64B`, `0x004FB663`, `0x004FAA10`, `0x004CA5A0` | Rust starts next item after pop in production tick, not after a command-stage delivery commit | `production_queue.rs`, future delivery command surface | Clear `ExitCoord`; completed MTNK spawns, production object is consumed, next queued vehicle becomes active only after that success | `grizzly_queue_restarts_after_successful_factory_exit_commit` | Do not mark completion as consumed before `ExitObject` success |
| Completed vehicle delivery is surfaced through land/naval pending slots before `Place_Production` | `StripClass::AI 0x006A8DC6..0x006A8E48`; `FUN_00734250 0x00734250..0x0073426C` | Rust has no pending delivery slot/UI-equivalent; `Done` front item is invisible to that distinction | `ProductionState`, sidebar/UI production surfaces, queue view | A completed land Grizzly is represented as pending non-naval delivery until placed/exited; a naval unit would use separate pending naval semantics | `completed_grizzly_uses_land_pending_delivery_slot_until_exit` | Do not collapse land and naval ready vehicle delivery if UI parity is modeled |

## Negative Facts / Do Not Do

- Do not treat factory-complete frame 648 as the visible spawned-Grizzly frame. Delivery is later `Place_Production`/`ExitObject` work. Evidence: `StripClass::AI`, `EventClass::Execute`, `HouseClass::Place_Production`.
- Do not call `CompletedProduction` before successful vehicle `ExitObject`. Evidence: `0x004FB560..0x004FB663`.
- Do not refund, abandon, pop, or start the next queued vehicle when the stock land WF `ExitCoord` is blocked. Evidence: `0x004FB589..0x004FB5BA`.
- Do not route a blocked completed Grizzly through another war factory or neighboring cell. Evidence: stock WF `ExitObject` uses `GetExitCoord` and returns failure on `Unlimbo` failure; sibling [BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md).
- Do not model `FUN_00734250` as the actual exit/spawn algorithm; it only stores the completed vehicle pointer in the land/naval pending delivery globals. Evidence: `0x00734250..0x0073426C`.

## Remaining Uncertainty

- Exact runtime UI/EVA retry presentation after a blocked completed Grizzly delivery remains unmeasured; static evidence proves the completed object and queue are preserved.
- Exact command-frame latency between `StripClass::AI` setting a vehicle delivery global and the eventual `EventClass::Execute -> Place_Production` commit was not runtime-measured.
- Exact stock 5x3 `ExitList[10]` pair and final door/drive-out contact clear remain delegated to existing/future war-factory mission work.

## Stale Docs / Replacement Wording

- [timing/unit-build-time.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/unit-build-time.md) should distinguish the 648-frame factory-complete state from final Grizzly delivery:

> Stock MTNK reaches factory-complete state after the 54th 12-frame production step, but the produced vehicle is not consumed/spawned by `FactoryClass::AI` itself. Sidebar/command delivery later runs `HouseClass::Place_Production`; successful stock land war-factory `ExitObject` unlimbo at `ExitCoord=512,256,0` then calls `FactoryClass::CompletedProduction -> FUN_004FAA10`, which may start the next queued item in the same command execution. If the vehicle exit is blocked, `Place_Production` returns before `CompletedProduction` and before queue restart, leaving the completed Grizzly pending.

- Any build-queue doc claiming blocked vehicle exit refunds/cancels should be replaced with:

> Blocked stock land war-factory vehicle exit is a pending delivery failure, not failed production. The completed factory object remains complete, no refund is paid, and the next queued item does not start until successful delivery/cleanup.

## Sources

- Ghidra read-only decompile: `0x004FB0E0`, `0x00443C60`, `0x004CA1A0`, `0x004FAA10`, `0x004CA5A0`, `0x006A8B30`, `0x00734250`.
- Ghidra read-only assembly/context: `0x004C710B`, `0x004FB560..0x004FB663`, `0x00444583..0x00444594`, `0x004FABA4..0x004FABB2`, `0x006A8DC6..0x006A8E48`, `0x00734250..0x0073426C`.
- Existing reports: `GRIZZLY_FACTORY_STEP_CADENCE_GHIDRA_REPORT.md`, [STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md), `HOUSE_PLACE_PRODUCTION_BLOCKED_WAR_FACTORY_EXIT_GHIDRA_REPORT.md`, [BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_GETDOCKCELLFOROBJECT_STOCK_WAR_FACTORY_EXIT_GHIDRA_REPORT.md), [WAR_FACTORY_EXIT_CONTACT_ROW_SKIP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WAR_FACTORY_EXIT_CONTACT_ROW_SKIP_GHIDRA_REPORT.md).
- INI: `ini/rulesmd.ini` stock `GAWEAP`, `NAWEAP`, `YAWEAP`.
- Rust scan: `src/sim/production/production_queue.rs`, `src/sim/production/production_spawn.rs`, `src/sim/production/production_placement_tests.rs`, `src/sim/production/production_types.rs`.

## Status

COMPLETE.
