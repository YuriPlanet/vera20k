# Drive Queued Click Event / Planning Mode Outcome - Reswarm Research Report

**Address(es):** `0x00700600`, `0x006FFEC0`, `0x004C6CB0`, `0x004D94B0`, `0x00731BF0`, `0x004AC700`, `0x006DAD60`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** exact native outcome of the second user click for a selected DriveLocomotion unit already moving to a cell, with emphasis on Shift/queued-click, EventClass execution, attack-move/planning-mode adjacency, and whether the click appends Foot/NavQueue, replaces NavCom, is ignored, or is owned by a separate queue.  
**Non-Scope:** full pathfinding tick count to `(42,40)`, full Planning Mode command authoring UI, every attack-move target variant, TeamClass waypoint scripts, and non-Drive locomotor movement physics.  
**Confidence:** High for standard empty-cell second-click destination reissue and no Foot/NavQueue append; Medium for full planning-mode command execution after plan exit because this slot reused prior planning overlay evidence and spot-checked only EventClass case `0x2A..0x2C`.  
**Active in YR:** Yes for standard tactical player input and EventClass execution. Planning/waypoint path behavior is conditional on the player entering Planning Mode or the attack-move/planning submode.

## 1. Overview

For the concrete trace scenario - stock `[MTNK]` moving from `(40,40)` toward `(42,40)`, then receiving a second clicked move to `(45,40)` - the native standard-YR outcome is **not** a Foot/NavQueue append. The second normal/Shift empty-cell click becomes a normal player command event; when `EventClass::Execute` drains it, the selected object receives a destination vtable call and `FootClass::Set_Destination_Internal` rewrites the owner destination (`Foot+0x5A4`) and the active locomotor head-to state.

If true Planning Mode / waypoint-path state is active, the click belongs to the separate House/`WaypointPathClass` surface, not to `FootClass+0x58C/+0x598`. That path draws multi-segment waypoint overlays and later exits through planning command events; it is not the Drive arrival `NavQueue` consumer.

## 2. Class Layout / Key Offsets

| Owner | Offset | Type | Purpose | Evidence | Active in YR |
|---|---:|---|---|---|---|
| FootClass | `+0x5A4` | `AbstractClass*` | current NavCom destination | `FootClass::Set_Destination_Internal @ 0x004D94B0`; prior NavCom docs | Yes |
| FootClass | `+0x58C` | `AbstractClass**` | NavQueue item buffer | [NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md) | Conditional, but not written by standard click |
| FootClass | `+0x598` | int | NavQueue count | same | Conditional, remains zero for this click path |
| FootClass | `+0x6B7` | byte | path-failed flag cleared on destination set | `0x004D96C2` in `Set_Destination_Internal` | Yes |
| House/player | `+0x20C` | int | current waypoint path index, `-1` none | `0x005090F0`, `0x004AC700` | Conditional |
| House/player | `+0x210 + index*4` | pointer | `WaypointPathClass*` slots, 12 paths | `0x00504740`, `0x006DAD60` | Conditional |
| WaypointPathClass | `+0x24` | int | loop/closure index, `-1` none | `0x00763BA0` via prior planning report | Conditional |
| WaypointPathClass | `+0x2C` | coord array | stored waypoint coords | `0x00763980` via prior planning report | Conditional |
| WaypointPathClass | `+0x38` | int | active point count | `0x006DAD60`, `0x005090F0` | Conditional |
| Global | `DAT_00a8ebf8/fc` | VK pair | Shift keys, right-click queue modifier | `TechnoClass::What_Action_OnObject @ 0x006FFEC0`, `FUN_00700600` | Yes |
| Global | `DAT_00a8ec00/04` | VK pair | Ctrl keys, force-fire | same | Yes |
| Global | `DAT_00a8ec08/0c` | VK pair | Alt keys, force-move | same | Yes |
| Global | `DAT_00b0fe58` | byte | attack-move/selection submode flag checked by `0x00731BF0` | decompile/assembly `0x00731BF0`; hotkey report | Conditional |
| DisplayClass | `+0x11B3` | byte | planning display flag toggled by `0x004AC700` | decompile `0x004AC700`; planning report | Conditional |

## 3. Core Logic

### 3.1 Empty-cell second click while moving

For a selected human-owned mobile techno, `FUN_00700600` computes the action for a cell click. It reads Ctrl first, Alt second, and Shift third:

1. Ctrl is tested through `DAT_00a8ec00/04`.
2. Alt is tested through `DAT_00a8ec08/0c`.
3. Shift is tested through `DAT_00a8ebf8/fc`.
4. If Alt and Ctrl are both set, the Ctrl force-fire flag is cleared.
5. In the normal move-capable, in-playfield branch, a Shift-held empty-cell click returns action `1`; non-Shift walkable cell handling may return action `1` or `2` depending on passability and unit/type checks.

Evidence:

- Decompile `FUN_00700600`.
- Assembly context around `0x00700600..0x00700706` in the decompile shows the three real-time key-pair checks.
- This is active in standard YR tactical input; no TS-only gate appears in this path.

The crucial finding is that action `1` still flows into the ordinary player command event path. It does **not** append to `Foot+0x58C/+0x598`.

### 3.2 EventClass command execution

`EventClass::Execute @ 0x004C6CB0` cases `4` and `5` are the player object/cell order path. For the movement class of command, the executor:

1. resolves the selected object from the event payload;
2. validates object live/on-map bytes and limbo/dead guards;
3. runs object command pre-work such as mission choice through vtable `+0x4A4`;
4. queues/assigns the resulting mission through vtable `+0x1E8`;
5. resolves the destination object/cell wrapper from the event payload;
6. calls the object vtable `+0x480(destination, ...)`.

For Foot-derived vehicles, vtable `+0x480` reaches `FootClass::Set_Destination_Internal @ 0x004D94B0`. That function:

1. writes `Foot+0x5A4 = destination`;
2. releases the old `+0x304`/suspended helper pointer if present;
3. resolves destination coords through the destination object's virtual coordinate getter;
4. calls active locomotor vtable `+0x44` (`Head_To_Coord`) with the destination coords;
5. clears `Foot+0x6B7`;
6. writes timer triplet fields around `+0x668/+0x66C/+0x670` from `g_CurrentFrameCounter` and `Rules+0x1768`;
7. resets the path retry/rate fields at `+0x640/+0x644/+0x648`.

Evidence:

- Decompile `EventClass__Execute @ 0x004C6CB0`, cases `4`/`5`.
- Assembly context for the normal command destination call includes `0x004C7418 -> 0x006E6E20`, `0x004C7420: CALL dword ptr [EBX + 0x480]`, `0x004C7474 -> 0x006E6E20`, and `0x004C747C: CALL dword ptr [EBP + 0x480]`.
- Adjacent EventClass destination/null-destination variants include `0x004C75ED` and `0x004C7619` (`CALL dword ptr [.. + 0x480]`).
- Decompile `FootClass__Set_Destination_Internal @ 0x004D94B0`; assembly/decompile line at `0x004D96C2` clears `+0x6B7`.
- [NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md) verified no player command path writes `Foot+0x58C/+0x598`.

### 3.3 Outcome relative to Drive arrival

The second clicked destination is applied when the event drains, before that same `Main_Tick` continues into `LogicClass::AI`, `Map::Logic`, render, and `LogicClass::PerTickUpdate` in the standard timing model. Therefore:

- If the second command event drains before the unit has physically reached `(42,40)`, the current destination is replaced/reissued to `(45,40)`. The unit should not be forced to finish `(42,40)` and then pop a queue entry.
- If the second command event drains after the first arrival has already cleared the destination, it is simply a new move order from the current position to `(45,40)`.
- There is no native "wait until arrival, then continue" behavior for this standard player click path unless some separate, verified producer has already populated `Foot+0x598`.

Evidence:

- Timing docs [timing/multiplayer-frame-step.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/multiplayer-frame-step.md) state event drain executes before `LogicClass::AI` in a `Main_Tick`.
- [DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md) and [DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md) verify the empty-NavQueue arrival branch.
- [NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md) verifies the standard command path does not populate the queue.

### 3.4 Attack-move / planning-mode adjacency

`FUN_00731BF0` is not a Foot/NavQueue producer. It returns true immediately when `DAT_00b0fe58` is nonzero; otherwise it requires both Ctrl and Alt key-pair groups (`DAT_00a8ec00/04` and `DAT_00a8ec08/0c`) and then verifies every selected object with object-bit `+0x14 & 1` through vtable `+0x4C0`.

Known callers use that predicate to change the command/cursor outcome:

- At `0x006FFBEC`, if the predicate and per-object `+0x4C0` pass, actions `1` or `2` are converted to action `0x1D`.
- At `0x0070F0B3`, action/cursor mapping returns `0x3E` or `0x3F` for action inputs `1`/`5` when the predicate is true.
- `FUN_00731AF0` enters this attack-move/submode state after validating selection support, writes `DAT_00b0fe58 = 1`, and shows `"MSG:NothingSelected"` or `"MSG:AttackMoveUnsupported"` on invalid selection.

Evidence:

- Decompile `FUN_00731BF0` and `FUN_00731AF0`.
- Assembly context `0x006FFBEC: CALL 0x00731BF0`, followed by `CALL [EAX+0x4C0]` and action conversion.
- Assembly context `0x0070F0B3: CALL 0x00731BF0`, followed by `MOV EAX,0x3e` / `MOV EAX,0x3f`.

This path is active in YR, but it is an attack-move/action-mode predicate, not the Drive arrival queue and not the Foot NavQueue append path.

### 3.5 True Planning Mode / WaypointPath surface

True Planning Mode is separate from both the normal second-click event and the Foot NavQueue consumer. `FUN_004AC700` toggles the display planning byte at `DisplayClass+0x11B3`, ensures/uses a current player path index at `g_PlayerPtr+0x20C`, and restores hover preview coordinates on exit.

`FUN_006DAD60` draws stored `WaypointPathClass` paths from `g_PlayerPtr+0x210 + slot*4`, iterating every stored point and drawing all adjacent segments. EventClass cases `0x2A`, `0x2B`, and `0x2C` call `FUN_00637E00`, which is the planning-event executor family in the existing planning docs.

For the target question, this means a click made while true Planning Mode is active is **planning-mode-owned** by `WaypointPathClass` state, not immediately converted to a `Foot+0x5A4` destination and not appended to `Foot+0x58C/+0x598`.

Evidence:

- Decompile `FUN_004AC700`.
- Decompile `FUN_006DAD60`.
- Decompile `EventClass__Execute @ 0x004C6CB0`, cases `0x2A..0x2C`.
- [PLANNING_QUEUED_WAYPOINT_LINES_AND_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PLANNING_QUEUED_WAYPOINT_LINES_AND_FLAGS_GHIDRA_REPORT.md).

## 4. INI Keys

| Section | Key | Stock YR value | Effect in this slice | Evidence |
|---|---|---:|---|---|
| `[General]` | `MaxWaypointPathLength` | `15` | Limits true Planning Mode `WaypointPathClass` point count through `Rules+0x90`; not used by normal second click. | `ini/rulesmd.ini:424`, `0x00671DBF`, `0x005090F0` |
| `[AudioVisual]` | `StartPlanningModeSound` | `PlanningModeStart` | Planning-mode entry sound; not part of normal click reissue. | `ini/rulesmd.ini:630`, planning report |
| `[AudioVisual]` | `EndPlanningModeSound` | `PlanningModeEnd` | Planning-mode exit sound; not part of normal click reissue. | `ini/rulesmd.ini:631`, planning report |
| `[AudioVisual]` | `AddPlanningModeCommandSound` | `PlanningModeAdd` | Played when adding planning commands; not part of normal click reissue. | `ini/rulesmd.ini:632`, planning report |

No INI key enables standard player move clicks to append `Foot+0x58C/+0x598`.

## 5. Integration Points

| Integration | Native behavior | Evidence | Active in YR |
|---|---|---|---|
| Cell action selection | reads Ctrl, Alt, Shift VK pairs and returns action code; Shift can return action `1` for move-capable in-playfield cells | `FUN_00700600` | Yes |
| Object action selection | Shift on suitable object can return action `1`; Ctrl/Alt priority is handled here too | `TechnoClass::What_Action_OnObject @ 0x006FFEC0` | Yes |
| EventClass execution | command events route to mission assignment and destination vtable calls; no NavQueue append | `EventClass__Execute @ 0x004C6CB0`; NavQueue producer report | Yes |
| Destination setter | rewrites `Foot+0x5A4` and active locomotor head-to state | `FootClass__Set_Destination_Internal @ 0x004D94B0` | Yes |
| Drive arrival | if no `Foot+0x598`, empty-queue arrival clears destination through null destination path | Drive arrival trace/docs | Yes |
| True Planning Mode | stores path points in House/player `WaypointPathClass`, not Foot NavQueue | `0x004AC700`, `0x006DAD60` | Conditional |
| Event queue timing | due events execute before same-tick AI/logic advance | [timing/multiplayer-frame-step.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/multiplayer-frame-step.md) | Yes |

## 6. Current Rust Implementation Status

Current Rust surfaces scanned:

- `src/app_context_order.rs:44` sets `queue_mode` from Shift alone.
- `src/app_context_order.rs:681` emits `Command::Move { queue: queue_mode, ... }`.
- `src/app_target_lines.rs:90` starts the selected action-line timer from command payloads.
- `src/sim/movement/movement_commands.rs:368` only appends movement paths when `queue && !uses_drive_locomotor`.
- `src/sim/movement/movement_commands.rs:551` calls `set_destination_internal_cell` for Drive locomotors.
- `src/sim/movement/movement_commands.rs:556` clears `navigation.nav_queue` for Drive destination issue.
- `src/sim/movement/navcom.rs:66` writes Rust `navigation.nav_com = Some(cell)`.

This means the specific Drive path has already moved closer than the older parent trace's Rust description: queued Drive moves now reissue destination and clear Rust `nav_queue`. Remaining Rust risk is broader input semantics: Shift alone is still modeled as a generic `queue` flag at the app command layer, and non-Drive queued movement still appends path data. Native standard YR does not support treating that as Foot/NavQueue parity.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Parent trace Stage 8 unknown | verified | this report; [DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md) | none for standard Drive empty-cell second click |
| Empty-cell action with Shift/Ctrl/Alt | verified | `FUN_00700600` decompile | none for target scenario |
| Object action with Shift/Ctrl/Alt | touched-not-exhausted | `0x006FFEC0` decompile | object-specific queued actions outside target scenario |
| EventClass normal command execution | verified | `0x004C6CB0` decompile; `0x004C7420`, `0x004C747C`, `0x004C75ED`, `0x004C7619` assembly contexts | full case-label naming outside target |
| Foot destination field writes | verified | `0x004D94B0`; `0x004D96C2` clear | none for handoff fields |
| Foot NavQueue producer absence | verified by prior report, spot-used | [NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md) | no standard producer found; historical producer remains adjacent |
| Attack-move predicate `0x00731BF0` | verified | decompile plus assembly `0x006FFBEC`, `0x0070F0B3`, `0x00731C02..0x00731CA8` | exact UI string/cursor labels not exhausted |
| True Planning Mode WaypointPath surface | touched-not-exhausted | `0x004AC700`, `0x006DAD60`, planning report | full click-to-point writer is a separate UI slice |
| Rust Drive command surface | verified | `movement_commands.rs:368`, `:551`, `:556` | focused test still needed |
| Rust non-Drive queued movement | touched-not-exhausted | `movement_commands.rs:368` | separate non-Drive parity investigation |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-01 - Does the standard second Shift/cell click append `Foot+0x58C/+0x598`? -> No. The click routes through normal command action/EventClass/destination setter; prior producer audit found no player NavQueue append.` (evidence: `0x00700600`, `0x004C6CB0`, [NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-02 - Is the second click ignored while the unit is already moving? -> No for a valid in-playfield move-capable cell. It produces an action/event and reissues destination when drained.` (evidence: `0x00700600`, `0x004C6CB0`, `0x004D94B0`)
- `[RESOLVED] OQ-03 - Is the second click deferred until Drive arrival by a hidden queue? -> No for standard player command. It applies at event execution time; Drive arrival sees whatever NavCom state exists later.` (evidence: [timing/multiplayer-frame-step.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/multiplayer-frame-step.md), `0x004D94B0`, Drive arrival trace)
- `[RESOLVED] OQ-04 - If true Planning Mode is active, is the click immediate unit movement? -> No. It is owned by House/player `WaypointPathClass` state and planning command execution, separate from Foot NavQueue.` (evidence: `0x004AC700`, `0x006DAD60`, EventClass cases `0x2A..0x2C`)
- `[RESOLVED] OQ-05 - Does `FUN_00731BF0` prove Foot/NavQueue queuing? -> No. It checks attack-move/submode plus Ctrl/Alt pairs and selected-object `+0x4C0` support, changing actions/cursors.` (evidence: `0x00731BF0`, `0x006FFBEC`, `0x0070F0B3`)
- `[RESOLVED] OQ-06 - Does native selected action-line endpoint become the unsupported Rust queue endpoint? -> Only if Foot NavQueue is actually nonzero; standard second click does not create that state, so live movement line should follow live NavCom after reissue.` (evidence: [NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md), producer report)
- `[RESOLVED] OQ-07 - What happens if the event drains before first arrival? -> Destination is replaced/reissued to the second target before same-tick logic; the unit does not pop a queued `(45,40)` at `(42,40)`.` (evidence: [timing/multiplayer-frame-step.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/multiplayer-frame-step.md), `0x004D94B0`)
- `[RESOLVED] OQ-08 - What happens if the event drains after first arrival? -> It is an ordinary new move from the current position; no hidden prior queue is consumed.` (evidence: Drive empty-queue arrival docs plus `0x004D94B0`)
- `[RESOLVED] OQ-09 - Is this TS legacy? -> No for the standard input/EventClass/destination path. Planning/attack-move branches are conditional but live in YR tactical code.` (evidence: active paths above)
- `[DEFERRED] OQ-10 - Exact point-add writer for every true Planning Mode click.` (category: `requires-different-system-context`; reason: target only needed to decide whether the Drive second click is planning-owned vs immediate; next-step-if-pursued: trace `PlanMode_MouseDown/MouseUp` through `WaypointPathClass` point add.)
- `[DEFERRED] OQ-11 - Exact cursor ID names for action `0x3E/0x3F`.` (category: `out-of-scope`; reason: outcome is sufficient from action conversion and destination/non-destination behavior; next-step-if-pursued: inspect mouse action table around `DAT_0082D6B8` and cursor strings.)

## 9. Visual / UI Interaction Ledger

This slot is input-path research, not a full visual composition pass. The only visual/UI surfaces needed for the target outcome are listed here.

| Order | Function / address | Condition / flag proof | Asset / frame | Rect / anchor | Palette / convert | Active for target? | Role |
|---|---|---|---|---|---|---|---|
| 1 | `FUN_00700600` | selected mobile object, cell click; real-time key pairs checked | none | n/a | n/a | yes | command action decision |
| 2 | `EventClass::Execute @ 0x004C6CB0` | due command event | none | n/a | n/a | yes | deterministic command execution |
| 3 | `TechnoClass::DrawActionLines @ 0x004DC060` | timer + selected-human + `ArchiveTarget || NavCom` | action-line raster helper | live unit/destination endpoints | palette/action-line path | conditional | selected move line after reissued NavCom |
| 4 | `FUN_006DAD60` | true Planning Mode / nonempty `WaypointPathClass` | `MOUSE.SHA` action table index `0x3C` | waypoint nodes/segments | dashed line pattern `DAT_00842940` | no for normal second click; yes for Planning Mode | planning path overlay |

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Standard Drive second move click reissues destination; it does not append Foot/NavQueue. | `0x00700600`, `0x004C6CB0`, `0x004D94B0`, NavQueue producer report | mostly matched for Drive in current source: Drive skips queue append and clears Rust queue | `src/sim/movement/movement_commands.rs:368`, `:551`, `:556`; `src/sim/movement/navcom.rs:66` | Keep Drive queued/Shift move as destination replacement/reissue, not arrival-time queue pop. | MTNK at `(40,40)` moving to `(42,40)` receives Shift/right-click move to `(45,40)` before arrival; `navigation.nav_queue` remains empty, `nav_com` becomes `(45,40)`, and no arrival pop occurs at `(42,40)`. | Do not restore old Drive `nav_queue` append semantics as "YR waypoint queue." |
| Standard event drain happens before same-tick object AI/logic, so the second click affects the active destination as soon as its event executes. | [timing/multiplayer-frame-step.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/multiplayer-frame-step.md), `EventClass::Execute` | unchecked exact command scheduling in Rust relative to movement tick | `src/app_context_order.rs`, pending command drain order, sim tick orchestration | Apply due move commands before movement processing for the same sim tick if modeling native `Main_Tick` ordering. | A move command scheduled for tick N updates NavCom before Drive locomotion processes tick N movement. | Do not process movement first and then apply same-frame input; that creates one-frame late steering. |
| True Planning Mode uses House/player `WaypointPathClass`, not Foot/NavQueue. | `0x004AC700`, `0x006DAD60`, EventClass cases `0x2A..0x2C` | missing planning path model | future app/input/planning subsystem, not `NavigationState.nav_queue` | Implement Planning Mode as a separate path-command surface with path slots and all-segment overlay. | Enter Planning Mode, add two waypoints, exit/execute: selected unit's `nav_queue` remains untouched while planning overlay draws both segments. | Do not represent Planning Mode points as `FootClass` NavQueue entries. |
| Selected movement line reads live `NavCom` or real NavQueue, not a command-stored queued endpoint. | action-line report plus this producer result | Rust has target-line timer and live nav state; focused Drive test needed | `src/app_target_lines.rs`, `src/sim/components.rs::NavigationState` | For the concrete second-click reissue, line endpoint should update to new live `NavCom`; it should not point to a queued endpoint that is waiting for arrival. | Selected MTNK receives second move to `(45,40)` while moving; line points to `(45,40)` because NavCom was reissued, not because a queued endpoint exists. | Do not store click endpoints in app target-line state as authoritative. |
| Shift alone is a native real-time modifier, but "queued move" does not mean Foot/NavQueue append. | `0x00700600`, `0x006FFEC0`, NavQueue producer report | Rust still has a generic `queue` bool from Shift and non-Drive append path | `src/app_context_order.rs:44`, `src/sim/movement/movement_commands.rs:368` | Narrow or rename semantics so Shift can affect action/mode without implying Foot/NavQueue parity. | Non-Drive follow-up test: Shift move should not be justified by Foot/NavQueue producer evidence unless a specific native producer is found. | Do not let the `queue` bool's name drive implementation; native has separate command action, Planning Mode, and Foot queue concepts. |

### Negative Facts / Do Not Do

- Do not append `(45,40)` to `Foot+0x58C/+0x598` for the standard second click. Active in YR: No for this producer.
- Do not defer the standard second clicked destination until `(42,40)` arrival. Active path reissues destination when the event executes.
- Do not collapse true Planning Mode, attack-move/submode (`DAT_00b0fe58`), selected action lines, and Foot NavQueue into one "queued waypoint" system.
- Do not delete Foot NavQueue storage/readers. Save/load and consumers remain real; this report only rules out the standard second-click producer.
- Do not treat `FUN_00731BF0` as a NavQueue append predicate. It is action/submode gating with selected-object `+0x4C0` support checks.

### Stale Docs / Follow-up Docs

- [docs/research/TARGET_LINES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TARGET_LINES_GHIDRA_REPORT.md) line saying "if Shift+Q held, also pushed to NavQueue" should be replaced with: "Selected action lines can consume `FootClass` NavQueue when it is already nonzero, but follow-up producer audits found no standard YR player command path that pushes Shift/right-click movement into `Foot+0x58C/+0x598`. Standard second move clicks reissue destination; true Planning Mode uses House/player `WaypointPathClass`."
- [docs/research/PLANNING_QUEUED_WAYPOINT_LINES_AND_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PLANNING_QUEUED_WAYPOINT_LINES_AND_FLAGS_GHIDRA_REPORT.md) wording that calls `FUN_00731BF0` the "planning / queue predicate" should be narrowed: "`FUN_00731BF0` is the attack-move/submode and Ctrl+Alt selected-object support predicate. It is adjacent to planning/queued visual surfaces but is not by itself a Foot/NavQueue producer and should not be cited for Drive queued-move arrival."
- [docs/research/traces/DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md) Stage 8 can be closed with: "Standard second clicked move reissues/replaces current NavCom at event execution time. It is not ignored, not converted into Foot/NavQueue, and not deferred to Drive arrival. If true Planning Mode is active, the click is owned by House/WaypointPathClass and remains separate from Foot/NavQueue."

## Sources

- Ghidra read-only decompile: `FUN_00700600` (cell action), `TechnoClass__What_Action_OnObject @ 0x006FFEC0`, `EventClass__Execute @ 0x004C6CB0`, `FootClass__Set_Destination_Internal @ 0x004D94B0`, `FUN_00731BF0`, `FUN_00731AF0`, `FUN_004AC700`, `FUN_005090F0`, `FUN_00504740`, `FUN_006DAD60`.
- Ghidra assembly context: `0x006FFBEC`, `0x0070F0B3`, `0x00731C02..0x00731CA8`, `0x004C7420`, `0x004C747C`, `0x004C75ED`, `0x004C7619`, `0x004D96C2`.
- Existing docs: [docs/research/traces/DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/DRIVE_ARRIVAL_QUEUED_ORDER_LIFECYCLE_TRACE_20260527.md), [docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_PUSH_PRODUCERS_GHIDRA_REPORT.md), [docs/research/NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md), [docs/research/PLANNING_QUEUED_WAYPOINT_LINES_AND_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PLANNING_QUEUED_WAYPOINT_LINES_AND_FLAGS_GHIDRA_REPORT.md), [docs/research/HOTKEY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOTKEY_SYSTEM_GHIDRA_REPORT.md), [docs/research/timing/multiplayer-frame-step.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/multiplayer-frame-step.md).
- INI checked: `ini/rulesmd.ini` keys `MaxWaypointPathLength`, `StartPlanningModeSound`, `EndPlanningModeSound`, `AddPlanningModeCommandSound`; base `ini/rules.ini` matching defaults.
- Rust scanned: `src/app_context_order.rs`, `src/app_target_lines.rs`, `src/sim/movement/movement_commands.rs`, `src/sim/movement/navcom.rs`, `src/sim/components.rs`.
