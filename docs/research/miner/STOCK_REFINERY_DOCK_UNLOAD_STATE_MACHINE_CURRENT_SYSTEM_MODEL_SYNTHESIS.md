# Stock Refinery Dock/Unload State Machine - Current System Model Synthesis

**Date:** 2026-05-24  
**Scope:** stock YR `CMIN/HARV -> GAREFN/NAREFN` refinery return, dock admission, unload, release, and two-miner contention.  
**Non-scope:** slave miners, service depots, aircraft docks, modded multi-dock buildings, save/load reconstruction, and exact rendered-frame capture.  
**Status:** implementation-safe for the static stock refinery FSM split; runtime-sensitive for the exact first `0x15` source in every replay frame.

This document is the current entry point for stock refinery docking. It supersedes older prose that collapses accepted-cell movement, `GetDockCoord`, radio `0x16`, and unload start into one "pad" event.

**Reswarm settlement 2026-05-24:** A five-slot doc-conflict reswarm found no contradiction to this model. The active stock split is: accepted `0x12` target `NW+(3,1)`, `GetDockCoord` / PerCellProcess equality coordinate `NW+(2,1)`, and `QueueingCell=4,1` staging coordinate `NW+(4,1)`. `UnitClass::Receive_Radio(0x16)` does not call `GetDockCoord`, does not set a destination, and does not write location; first ordinary `0x16` may only sync timer/rate and return, while a later/already-synced `0x16` can send `0x15` without physical `NW+3 -> NW+2` movement. Treat older docs that require physical `NW+(2,1)` before every `0x15`, label `0x00739EC0` as mission dispatch, or describe normal stock exit as `ReleaseDockedHarvester` / `Force_Track(0x47)` as stale.

## Evidence Ladder

| Rank | Meaning |
|---|---|
| BINARY_HIGH | Fresh Ghidra body/branch evidence plus active stock YR INI gate |
| RESEARCH_HIGH | Recent focused Ghidra report or reswarm slot with addresses and handoff |
| TRACE_HIGH | Runtime/player-visible trace tied to binary evidence |
| DOC_SYNTHESIS | Older overview, useful only when not contradicted |
| INFERENCE | Plausible but not safe by itself |

## One-Screen Flow

1. `Mission_Harvest` close return sends/uses `HELLO(0x02)` to the refinery contact system.
2. Miner enters mission `7` / `Enter`.
3. Due `FootClass::Mission_Enter` sends one `CAN_DOCK(0x0E)`.
4. Building `0x0E` may run an early `GetDockCoord` side-check against requester `+0x5A4`; this does not change the later move target.
5. Building sends `NEED_TO_MOVE(0x13)`.
6. Building sends `MOVE_TO_CELL(0x12)` with accepted target `building NW+(3,1)`.
7. If `0x12` returns `1`, the miner moves to the accepted cell; no `0x18`, `0x16`, or unload handoff occurs in that pass.
8. `Mission_Enter` returns stock `[Enter]` delay `ftol(.016 * 900) + RandomRanged(0,2) = 14..16` frames.
9. On a later due `Mission_Enter`, if `0x12` returns `0x14` already-there, building sends `0x18` then `0x16`.
10. First ordinary `0x16` may only set/sync facing timer `+0x388` toward `0x4000` and return.
11. Later/already-synced `0x16` can send `0x15` from stopped accepted-cell state when idle, destination is a building, contact flag is set, and unit mission is `7`.
12. Separately, `UnitClass::PerCellProcess` can send `0x15` through cell-entry branches, including a `GetDockCoord` equality branch and a contact-flag adjacent-building branch.
13. Building `0x15` queues miner mission `0x10` / `Mission_Deploy_Building`.
14. `Mission_Deploy_Building` state 3 drains ore/storage and awards credits.
15. Empty-slot gate advances to state 4; state 4 clears unload visual/state, queues Harvest, and sends `BREAK(0x03)` if a valid contact exists.
16. Refinery release does not promote waiters; each waiting miner is admitted only on its own due `Mission_Enter`.

## Claim Table

| Claim | Best evidence | Status | Active YR | Safe? |
|---|---|---|---|---|
| Stock accepted `0x12` target is building NW+(3,1). | `BuildingClass::Receive_Radio @ 0x0043C2D0`, fresh spot-check; `REFINERY_DOCK_0X16_BRIDGE_VERIFICATION` | confirmed | yes | IMPLEMENTATION_SAFE |
| Stock `GetDockCoord` is a separate dock coordinate, not the accepted movement target. | `BuildingClass::GetDockCoord @ 0x00447B20`; coord-cell reports | confirmed | yes | IMPLEMENTATION_SAFE |
| `QueueingCell=4,1` is staging/fallback, not accepted `0x0E` target. | `artmd.ini:[GAREFN]/[NAREFN]`; miner contact synthesis | confirmed | yes | IMPLEMENTATION_SAFE |
| Early `GetDockCoord` in building `0x0E` is a local side-check against requester `+0x5A4`. | `BUILDING_RECEIVE_RADIO_0E_GETDOCKCOORD_SIDE_CHECK` | confirmed | yes | IMPLEMENTATION_SAFE |
| `0x12 == 1` does not send `0x18/0x16`; only `0x12 == 0x14` does. | `FOOTCLASS_MISSION_ENTER_0X0E_REPEAT_TIMING`; fresh building spot-check | confirmed | yes | IMPLEMENTATION_SAFE |
| `Mission_Enter` retry is 14..16 frames with stock `[Enter] Rate=.016`. | `FootClass::Mission_Enter @ 0x004D9290`; `rulesmd.ini:[Enter]` | confirmed | yes | IMPLEMENTATION_SAFE |
| First unsynced `0x16` can return after setting facing/timer only. | `UnitClass::Receive_Radio @ 0x00737430`, fresh spot-check | confirmed | yes | IMPLEMENTATION_SAFE |
| Later/already-synced `0x16` can send `0x15` without `GetDockCoord` equality. | `UNITCLASS_RECEIVE_RADIO_0X16_SECOND_CALL_TIMING`; fresh spot-check | confirmed | yes | IMPLEMENTATION_SAFE |
| `0x16` is not proven as an East-facing pivot for stock refinery unload. | `DOCK_ARRIVAL_PIVOT_SEQUENCE_DOC_CONFLICT_AUDIT`; `RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_DOC_CONFLICT_AUDIT` | confirmed | yes | IMPLEMENTATION_SAFE negative |
| Drive arrival can leave current cell at accepted NW+(3,1), moving false, destination still live. | `DRIVELOCOMOTOR_ACCEPTED_CELL_ARRIVAL_VISIBILITY` | confirmed | yes | IMPLEMENTATION_SAFE |
| `PerCellProcess` has separate `0x15` branches and is not the mission-7 dispatch handler. | `UNITCLASS_PERCELLPROCESS_CALLER_TICK_ORDER`; `DRIVELOCOMOTOR_ACCEPTED_CELL_ARRIVAL_VISIBILITY` | confirmed | yes | IMPLEMENTATION_SAFE |
| Exact first `0x15` source in every retail replay frame is globally fixed by static docs alone. | latest reports preserve source race caveats | unknown | yes | NEEDS_RUNTIME_TRACE if exact frame is required |
| Stock unload completion uses zero-link `Mission_Deploy_Building` state 4, not normal reciprocal `+0x2E4` release. | `CHRONO_MINER_REFINERY_DOCK_UNLOAD_SYSTEM_MODEL_SYNTHESIS`; [miner/README.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/README.md) | confirmed | yes | IMPLEMENTATION_SAFE |
| Refinery release promotes a waiting miner. | two-miner reports and synthesis | contradicted | yes | DO_NOT_IMPLEMENT |

## Coordinate Frames

| Name | Stock refinery NW `(rx, ry)` example `(10,10)` | Meaning |
|---|---:|---|
| Accepted `0x12` movement cell | `(rx+3, ry+1)` = `(13,11)` | Cell the refinery tells the miner to move to during `0x0E` admission |
| `GetDockCoord` cell | `(rx+2, ry+1)` = `(12,11)` | Building dock coordinate used by side-checks and one `PerCellProcess` branch |
| `QueueingCell=4,1` | `(rx+4, ry+1)` = `(14,11)` | Art fallback/staging cell for waiting/refused/far-return behavior |

Do not fold these together. The corrected model keeps all three.

## Timing And Tick Order

- `MissionClass::Mission_Dispatch` gates mission `7`; `FootClass::Mission_Enter` sends only one `0x0E` per dispatch.
- Stock `[Enter] Rate=.016` gives `14..16` frames between normal Enter attempts.
- In a unit AI tick, mission dispatch runs before the locomotor `Process`; per-cell callbacks caused by that locomotor processing are later in the same unit tick.
- Therefore a due mission pass can issue `0x0E -> 0x12 already-there -> 0x18 -> 0x16` before same-tick locomotor/per-cell work.
- A per-cell `0x15` source can still fire before the next mission pass if a later movement/per-cell event reaches its gates.

## Radio Message Catalog For This Path

| Msg | Direction | Stock refinery role |
|---:|---|---|
| `0x02` HELLO | miner -> refinery | Contact admission/relationship setup |
| `0x03` BREAK | miner -> refinery | Contact release on zero-link state-4 exit or cancel paths |
| `0x0E` CAN_DOCK | miner -> refinery | Main Mission Enter admission request |
| `0x13` NEED_TO_MOVE | refinery -> miner | Probe before accepted-cell movement |
| `0x12` MOVE_TO_CELL | refinery -> miner | Move to accepted NW+(3,1), or reply already-there |
| `0x18` ENTER_DOCK | refinery -> miner | Sets Techno/Unit contact-entered flag `+0x418` |
| `0x16` TIMING_SYNC | refinery -> miner | Facing/timer sync; later may cascade `0x15` |
| `0x15` DOCK_NOW | miner -> refinery | Building queues miner mission `0x10` unload |

## Unload FSM

The unload/deposit system starts after building `0x15` queues mission `0x10`.

- `Mission_Deploy_Building` state 3 handles ore drain, storage-slot gates, credit award, and unload display state.
- The next empty-slot gate advances to state 4; there is no extra stock post-empty cooldown.
- State 4 clears the unload visual/state, queues Harvest, and sends `BREAK(0x03)` if a valid contact exists.
- Normal stock DockUnload does not require reciprocal `unit/building +0x2E4` links; those paths are conditional/legacy/interrupt-style and not the ordinary zero-link unload completion path.

## Two-Miner / Busy Refinery Rules

- A releasing miner frees its own contact; the refinery does not call back or promote a queued miner.
- A waiting miner claims only during its own due `Mission_Enter` / `0x0E` pass.
- Same-frame takeover is conditional on live object order and the waiting miner's mission timer being due.
- Waiting/busy retry must respect the Mission Enter timer; do not poll `CAN_DOCK` every tick.

## Do Not Implement

- Do not force a physical move from accepted NW+(3,1) to `GetDockCoord` NW+(2,1).
- Do not use `GetDockCoord` as the accepted `0x12` target.
- Do not use `QueueingCell=4,1` as the accepted `0x12` target.
- Do not collapse `0x0E`, `0x12`, `0x18`, `0x16`, `0x15`, and unload start into one Rust phase.
- Do not start unload merely because movement to accepted cell completed.
- Do not treat `0x16` return `1` as proof that `0x15` was sent.
- Do not require `GetDockCoord` equality before every possible `0x15`.
- Do not model stock refinery release as FIFO waiter promotion.
- Do not model normal stock DockUnload completion as reciprocal `+0x2E4` dock-link release.

## Rust Implementation Checklist

- Preserve separate helpers/names for accepted cell, `GetDockCoord`/pad cell, and `QueueingCell`.
- Keep a stopped-at-accepted-cell state where movement is idle but the logical refinery destination remains live.
- Add Mission Enter retry timing: stock `14..16` frames, with `RandomRanged(0,2)` jitter when RNG parity is in scope.
- Split `0x12 == 1` movement assignment from `0x12 == 0x14` already-there handoff.
- Split first unsynced `0x16` from later/already-synced `0x16`.
- Do not name the `0x16` state as an East-facing pivot unless a runtime trace proves the visible body-facing write for that frame. Static binary evidence supports timer/rate sync and source-specific `0x15`, not a direct `GetDockCoord` move or forced East body-facing operation.
- Represent `0x16 -> 0x15` and `PerCellProcess -> 0x15` as source-aware handoffs.
- Keep unload `Mission_Deploy_Building` separate from dock admission/linking.
- Add focused tests for accepted NW+3, `GetDockCoord` NW+2, no forced NW+3->NW+2 move, no next-tick retry, first `0x16` no unload, later `0x16` unload, and no refinery-side waiter promotion.

## Stale Or Superseded Claims

- "Refinery dock pad fix is NW+3 -> NW+2 for every miner deposit" is superseded. Correct: accepted movement remains NW+3; `GetDockCoord` remains NW+2; unload can be triggered by source-specific radio/per-cell gates.
- "`0x16` bridges/moves the miner to `GetDockCoord`" is contradicted. Correct: `0x16` has no `GetDockCoord`, no `Set_Destination`, and no location write.
- "`0x16` is a proven East-facing pivot" is unproven/stale for stock refinery unload. Correct: the verified static behavior is timer/rate sync toward `0x4000`; exact visible body facing during dump remains runtime-sensitive.
- "`PerCellProcess @ 0x00739EC0` is the UnitClass mission-7 handler" is contradicted. Correct: mission dispatch uses `FootClass::Mission_Enter @ 0x004D9290`; `0x00739EC0` is a per-cell hook.
- "`Is_Moving == false` means destination is gone" is contradicted for Drive arrival. Correct: Drive movement state can be clear while `Foot+0x5A4` still points at the refinery.
- "Stock refinery release promotes queued miners" is contradicted. Correct: waiters retry on their own mission timer.

## Remaining Runtime-Sensitive Questions

- Which exact `0x15` source wins first in every concrete replay frame after the first `0x18/0x16`: later/aligned `0x16`, `PerCellProcess` `GetDockCoord`, or contact-flag adjacent-building branch.
- Exact facing/timer frame count from first unsynced `0x16` to `RateTimer::Current(+0x388) == 0x4000` for each relevant unit `Rot`.
- Exact player-visible frame count for rare refinery-loss unload visual stale-frame cases.

These are not blockers for removing the NW+2 physical-move misconception. They matter if the next Rust patch needs exact first-frame presentation parity.

## Source Ledger

- Fresh Ghidra spot-checks in this synthesis: `BuildingClass::Receive_Radio @ 0x0043C2D0`, `UnitClass::Receive_Radio @ 0x00737430`.
- [miner/DOCK_ARRIVAL_PIVOT_SEQUENCE_DOC_CONFLICT_AUDIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCK_ARRIVAL_PIVOT_SEQUENCE_DOC_CONFLICT_AUDIT_GHIDRA_REPORT.md)
- [miner/RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_DOC_CONFLICT_AUDIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_DOC_CONFLICT_AUDIT_GHIDRA_REPORT.md)
- [UNITCLASS_PERCELLPROCESS_GETDOCKCOORD_VS_0X16_RECONCILIATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_PERCELLPROCESS_GETDOCKCOORD_VS_0X16_RECONCILIATION_GHIDRA_REPORT.md)
- [ACCEPTED_CELL_GETDOCKCOORD_QUEUEINGCELL_DOC_CLUSTER_AUDIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACCEPTED_CELL_GETDOCKCOORD_QUEUEINGCELL_DOC_CLUSTER_AUDIT_GHIDRA_REPORT.md)
- [miner/STOCK_REFINERY_DOCK_RUST_IMPLEMENTATION_CONTRACT_INPUTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/STOCK_REFINERY_DOCK_RUST_IMPLEMENTATION_CONTRACT_INPUTS_GHIDRA_REPORT.md)
- [FOOTCLASS_MISSION_ENTER_0X0E_REPEAT_TIMING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_ENTER_0X0E_REPEAT_TIMING_GHIDRA_REPORT.md)
- [UNITCLASS_RECEIVE_RADIO_0X16_SECOND_CALL_TIMING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_RECEIVE_RADIO_0X16_SECOND_CALL_TIMING_GHIDRA_REPORT.md)
- [UNITCLASS_PERCELLPROCESS_CALLER_TICK_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_PERCELLPROCESS_CALLER_TICK_ORDER_GHIDRA_REPORT.md)
- [BUILDING_RECEIVE_RADIO_0E_GETDOCKCOORD_SIDE_CHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_RECEIVE_RADIO_0E_GETDOCKCOORD_SIDE_CHECK_GHIDRA_REPORT.md)
- [DRIVELOCOMOTOR_ACCEPTED_CELL_ARRIVAL_VISIBILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTOR_ACCEPTED_CELL_ARRIVAL_VISIBILITY_GHIDRA_REPORT.md)
- [REFINERY_DOCK_0X16_BRIDGE_VERIFICATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/07-cross-system-consumers/REFINERY_DOCK_0X16_BRIDGE_VERIFICATION_GHIDRA_REPORT.md)
- [CHRONO_MINER_REFINERY_DOCK_UNLOAD_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONO_MINER_REFINERY_DOCK_UNLOAD_SYSTEM_MODEL_SYNTHESIS.md)
- [miner/README.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/README.md)
- `rulesmd.ini`: `[CMIN] Dock=NAREFN,GAREFN`, `[HARV] Dock=NAREFN,GAREFN`, `[GAREFN]/[NAREFN] DockUnload=yes`, `[Enter] Rate=.016`.
- `artmd.ini`: `[GAREFN]/[NAREFN] QueueingCell=4,1`.
