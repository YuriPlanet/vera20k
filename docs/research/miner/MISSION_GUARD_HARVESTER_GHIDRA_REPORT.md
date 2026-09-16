# UnitClass::Mission_Guard_Harvester — Ghidra Research Report

Reverse-engineered from `gamemd.exe` via Ghidra MCP.
Target: `0x00740810` (`UnitClass__Mission_Guard_Harvester`)
Date: 2026-05-19

---

## 1. Identity and Dispatch

**Address:** `0x00740810`
**Label:** `UnitClass__Mission_Guard_Harvester` (pre-existing Ghidra annotation)
**Vtable:** UnitClass vtable at `0x007F5C70`, offset `+0x21C`
**Evidence:** Single xref to `0x740810` is from `[DATA] 0x007F5E8C`.
`0x7F5E8C - 0x7F5C70 = 0x21C`. Verified: memory at `0x7F5E8C` = `10 08 74 00` = `0x00740810`.

**Mission code:** 5 (Guard / Sticky).
`MissionClass__Mission_Dispatch` (0x5B3060) case 5 dispatches via `(*vtable + 0x21C)()`.
Case 6 (Sticky) also dispatches via the same vtable+0x21C slot.
**This is a top-level mission — called directly by the main mission dispatcher.**
It is NOT a sub-state of Mission_Harvest (10). When Mission_Harvest state 4 calls
`Queue_Mission(5)`, control transfers here on the next Mission_Dispatch call.

**Active in YR:** YES. Unconditional — any UnitClass instance with mission code 5
routes here via vtable dispatch. Confirmed live in normal YR skirmish play (every
harvester/weeder that gets stranded triggers this path).

---

## 2. Full State-Machine Logic (decompiled and annotated)

### 2.1 Block 1 — Slave Manager Recall (Slave Miner Only)

```
Condition:
  param_1[0xB6] != 0                           // UnitClass+0x2D8: has slave master
  AND RulesClass+0x1790 + param_1[0xC0] < CurrentFrame  // kick frame delay elapsed
  AND SlaveManagerClass__ShouldRecallSlaves()  // slave state permits recall

Action:
  SlaveManagerClass__RecallAllSlaves()
  → return MissionTimerEntry + Random(0,2)     // early exit, stay in Guard mission
```

`param_1[0x30]` is offset `0xC0` — appears to be a saved frame counter used as a
kick-frame tracker for the slave delay. `RulesClass+0x1790` = `SlaveMinerKickFrameDelay`
(from HARVESTER_MISSION_HARVEST_GHIDRA_REPORT.md, confirmed).

This entire block applies **only to Slave Miners** (units with `SlaveMaster` non-null).
Chrono Miners have no slave master and skip this block unconditionally.

### 2.2 Block 2 — Harvester/Weeder: AI Re-trigger to Mission_Harvest

Condition: unit type has `Harvester=yes` (`TypeClass+0xE0E`) OR `Weeder=yes` (`TypeClass+0xE0F`).

#### Sub-block A — AI-controlled harvester (not player-controlled)

```
if !HouseClass__IsPlayerControl(owner):
  count = TypeClass+0x3F8  // number of Dock= building types
  for i in 0..count:
    dock_type = TypeClass.DockList[i]
    owned = HouseClass__CountOwnedInstances(dock_type+0xDF8)  // count owned refineries
    if owned > 0:
      if (Harvester=yes && House+0x242 == 0):  // not flagged "ore depleted"
        OR (Weeder=yes):
          Queue_Mission(10)   // re-issue Harvest mission
          return 1            // immediately exits Guard
      break  // owned refinery exists but ore-depleted flag set → stay in Guard
```

`param_1[0x87]` = offset `0x21C` = Owner (HouseClass pointer).
`House+0x242` = house AI flag "ore depleted" (set by Mission_Harvest state 4 for
chrono miners, confirmed from HARVESTER_MISSION_HARVEST_GHIDRA_REPORT.md).

**Key observation:** An AI-controlled Harvester=yes unit will stay in Guard indefinitely
if `House+0x242` is set — it does NOT re-trigger Harvest automatically when that flag
is set. A Weeder=yes always re-triggers regardless of the ore-depleted flag.

#### Sub-block B — Player-controlled chrono miner (Teleporter=yes)

```
if HouseClass__IsPlayerControl(owner) AND TypeClass+0xCD4 (Teleporter=yes):
  call vtable+0x1BC()        // clear destination / path
  for cell_offset in 0..8:   // check 8 adjacent cells
    Pathfinding_update_continued(cell_offset)
    bldg = Look_up_building_in_cell()
    if bldg != null
       AND BuildingTypeClass+0x16BB != 0  // building is a refinery/dock type
       AND bldg.Owner == this.Owner:      // same house
      Queue_Mission(10)   // found a refinery nearby → switch back to Harvest
      return 1

  // Check if locomotor reports full storage
  storage_pct = (*vtable+0x2B4)()   // UnitClass__Get_Storage_Percentage
  if storage_pct == 1.0 (full):
    loco = param_1[0x19D]   // offset 0x674 = Locomotor* (ILocomotion**)
    loco_ok = loco.vtable+0x10()    // Is_Ok_To_End (IPiggyback)
    if Is_Ok_To_End == true:
      Queue_Mission(10)   // storage full and loco is ready → switch to Harvest
      return 1
```

`param_1[0x19d]` = offset `0x19D * 4 = 0x674` = the `Locomotor*` field on FootClass.
`vtable+0x10` on the locomotor interface = `Is_Ok_To_End` from IPiggyback (confirmed
from [CHRONO_MINER_SYSTEM_OVERVIEW.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_SYSTEM_OVERVIEW.md) §IPiggyback table).
`BuildingTypeClass+0x16BB` — the flag checked is a "dock-capable/refinery" flag on
building type. Exact name not independently verified but the adjacent-refinery context
is unambiguous.

**This block is chrono-miner-specific (Teleporter=yes check).** It provides two recovery
paths back to Harvest that do not exist for normal harvesters:
1. A nearby friendly refinery was found (player may have rebuilt one).
2. The unit is player-controlled, storage is full, and the locomotor has settled.

### 2.3 Block 3 — Ore Purifier / Weeder Building Check

```
if TypeClass+0x404 != 0:   // unit has a "purifier building type" reference
  scan RulesClass OrePurifier list (RulesClass+0x8B0, count at +0x8BC):
    if list[i] == TypeClass+0x404:
      if House+499 != 0 (house flag byte) AND !IsPlayerControl:
        Queue_Mission(0x10)   // assign mission 16 (Unload)
        goto return_with_timer
      break
```

`RulesClass+0x8B0` = pointer to OrePurifier building-type list (verified via
context: looping over list entries comparing against a unit's associated building type).
`RulesClass+0x8BC` = count of entries in that list.
This block handles weeder/ore-purifier units that should unload somewhere specific.
Not relevant to chrono miners (their `TypeClass+0x404` is almost certainly 0).

### 2.4 Block 4 — Weeder Re-trigger on Flag Clear

```
if TypeClass+0xE0F (Weeder=yes) AND (char)param_1[0x1AE * 4 = 0x6B8] != 0:
  param_1[0x6B8] = 0           // clear the flag
  Queue_Mission(10)             // re-trigger Harvest
  // falls through to FootClass__Mission_Guard
```

`+0x6B8` is a unit instance byte flag (name not independently verified) used to
signal "re-arm for harvest". Cleared here and Harvest re-queued. Execution continues
to FootClass::Mission_Guard below regardless (no early return here, unlike block 2).

### 2.5 Final Delegation — FootClass::Mission_Guard

```
iVar2 = FootClass__Mission_Guard()   // 0x004D5070
return iVar2
```

After all harvester-specific checks, the function unconditionally delegates to
`FootClass__Mission_Guard` for the actual idle/scan/turn behavior.
**Mission_Guard_Harvester does NOT re-implement guard idle logic.** All the actual
Guard behavior (target scanning, auto-attack, timer management) lives in the base
class. Mission_Guard_Harvester is purely a pre-filter that may re-issue Harvest
before guard logic runs.

---

## 3. Callers / Dispatch Chain

Direct callers: none (the "no callers found" result from Ghidra confirms dispatch
is 100% through the vtable). The call chain is:

```
MissionClass__Mission_Dispatch (0x5B3060)
  case 5 → (*vtable + 0x21C)()
    → UnitClass__Mission_Guard_Harvester (0x740810)   [for UnitClass instances]
    → FootClass__Mission_Guard (0x004D5070)             [for FootClass base]
```

Mission_Guard_Harvester is also reached when Mission_Harvest state 4 calls
`Queue_Mission(5, false)` (confirmed in HARVESTER_MISSION_HARVEST_GHIDRA_REPORT.md §State 4).

---

## 4. Answers to Slot Questions

**(a) Sub-mission of Mission_Harvest or top-level?**
Top-level. Vtable+0x21C = Mission 5 (Guard), dispatched directly from
`MissionClass__Mission_Dispatch`. Mission_Harvest state 4 assigns mission 5, which
routes here on the next dispatch call. There is no nesting or callback.

**(b) Just wraps FootClass::Mission_Guard or has unique logic?**
It has substantial unique logic (blocks 1–4 above) that runs before delegating to
`FootClass__Mission_Guard`. The key additions are: slave-recall for slave miners,
AI re-trigger to Harvest when a refinery exists and ore isn't depleted, and two
chrono-miner-specific recovery paths (adjacent-refinery scan and full-storage+loco-ready check).

**(c) Conditions that transition back to Mission_Harvest state 0?**
Four distinct transitions to `Queue_Mission(10)`:
1. AI-controlled, refinery owned, Weeder=yes → immediate re-trigger (no ore-depleted gate).
2. AI-controlled, refinery owned, Harvester=yes, House+0x242 == 0 → immediate re-trigger.
3. Player-controlled, Teleporter=yes, friendly refinery in adjacent 8 cells → re-trigger.
4. Player-controlled, Teleporter=yes, storage 100% full, `Is_Ok_To_End` true → re-trigger.
5. Weeder=yes, flag byte at +0x6B8 set → re-trigger (then falls through to Guard).
All `Queue_Mission(10)` calls land in Mission_Harvest state 0 (SCAN) because
Mission_Harvest resets `param_1[0xBC]` (MissionSubState) to 0 on entry.

**(d) Chrono-miner-specific or generic?**
Generic entry point (all UnitClass instances use vtable+0x21C). The chrono-miner-specific
code is block 2 sub-block B, gated on `TypeClass+0xCD4 == 1` (Teleporter=yes). Normal
harvesters see only the AI re-trigger path in sub-block A. Slave miners see block 1.

**(e) Chrono-miner-specific branches and +0xCD4 check?**
Yes. Confirmed `Teleporter=yes` (`TypeClass+0xCD4`) check in sub-block B. The branch:
- Scans 8 adjacent cells via `Pathfinding_update_continued` + `Look_up_building_in_cell`
  for a same-house refinery, re-triggers Harvest if found.
- Checks `Is_Ok_To_End` on the locomotor at `+0x674` — this is the IPiggyback method
  that returns true when warp is fully complete and no piggybacking is active.
- No other chrono-specific fields (no `+0x271 BeingWarped`, no `+0x280 PendingWarpPhase`)
  are touched in this function; those are handled by the locomotor layer.

---

## 5. Relation to [MISSION_GUARD_AREAGUARD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_GUARD_AREAGUARD_GHIDRA_REPORT.md)

Mission_Guard_Harvester at 0x740810 is distinct from generic Guard/AreaGuard:
- It overrides vtable+0x21C in UnitClass (guard mission slot) only for UnitClass objects.
- The base `FootClass__Mission_Guard` (0x4D5070) and AreaGuard remain separate.
- `UnitClass__Mission_Guard` (0x740A90, vtable+0x22C = Move mission slot) is a different
  function that handles unit movement with guard-mode semantics; it does not overlap.

---

## 6. Key Verified Facts (≤5)

| # | Claim | Evidence |
|---|-------|----------|
| 1 | Top-level Mission 5 (Guard), dispatched via vtable+0x21C | Memory read at 0x7F5E8C = 0x00740810; MissionClass__Mission_Dispatch case 5 confirmed |
| 2 | Chrono-miner branch gated on TypeClass+0xCD4 (Teleporter=yes) | Decompiled at LAB sub-block B: `*(char *)(param_1[0x1b1] + 0xcd4) != 0` |
| 3 | Locomotor Is_Ok_To_End checked via param_1[0x19D] (offset 0x674) vtable+0x10 | Decompiled: `(**(code **)(*(int *)param_1[0x19d] + 0x10))(...)` |
| 4 | Unconditional tail call to FootClass__Mission_Guard (0x4D5070) — no re-impl of idle logic | Decompiled final line; callee confirmed via get_function_callees |
| 5 | AI-controlled harvester re-triggers Harvest (mission 10) only when House+0x242 == 0 | Decompiled block 2A: `*(char *)(param_1[0x87] + 0x242) == 0` guard before Queue_Mission(10) |

---

## 7. Implementation Notes for Rust Port

- When implementing Mission::Guard for UnitClass, run these pre-checks before delegating
  to the base guard logic:
  1. Slave-miner recall (if SlaveManager present and kick timer elapsed).
  2. AI harvester/weeder: check owned refineries, conditionally re-trigger Harvest.
  3. Player chrono miner: scan 8 adjacent cells for friendly refinery; check loco
     readiness + full storage.
  4. Weeder re-arm flag at instance+0x6B8.
- The chrono-miner adjacent-refinery scan uses `Pathfinding_update_continued` + cell
  building lookup — the exact semantics of `BuildingTypeClass+0x16BB` should be
  verified separately before porting (likely `Refinery=yes` or similar flag).
- `Is_Ok_To_End` must be called on the **active** locomotor via the IPiggyback vtable;
  this is already wired in the chrono overview but the Guard path is an additional caller.
