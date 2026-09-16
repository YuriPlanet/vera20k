# Harvester Mission_Harvest State Machine — Ghidra Research Report

Reverse-engineered from `gamemd.exe` via Ghidra MCP. All addresses reference the
YR executable. Extends findings from [HARVESTER_DOCK_UNLOAD.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/miner/HARVESTER_DOCK_UNLOAD.md) and
CHRONO_MINER_TELEPORT_GHIDRA_REPORT.md.

**2026-07-24 timer correction:** Live disassembly of
`TechnoClass::AI_Update @ 0x006F9E50` proves mission dispatch precedes shared
StepTimer maintenance. `+0x108` is the current duration and `+0x10C` is its
repeat duration; the step amount is `+0x110` (constructed as `1`). With stock
rate `2`, counter `9` is written after the mission at `F+18` and first observed
by `Mission_Harvest` at `F+19`.

---

## 1. Mission Numbering (Corrected)

**Table address:** `0x00816CAC` (32 entries of char* pointers)
**Confidence:** 99% (verified from Mission_From_Name at 0x005B3910 and Mission_Name at 0x005B3950)

| Code | Name | VTable Offset | Purpose |
|------|------|---------------|---------|
| 0 | Sleep | +0x204 | |
| 1 | Attack | +0x210 | |
| 2 | Move | +0x22C | |
| 3 | QMove | (default) | |
| 4 | Retreat | +0x230 | |
| 5 | Guard | +0x21C | |
| 6 | Sticky | +0x21C | |
| **7** | **Enter** | **+0x240** | **Harvester enters refinery** |
| 8 | Capture | +0x214 | |
| 9 | Eaten | +0x218 | |
| **10** | **Harvest** | **+0x224** | **Harvester gathers ore** |
| 11 | Area Guard | +0x220 | |
| 12 | Return | +0x234 | |
| 13 | Stop | +0x238 | |
| 14 | Ambush | +0x20C | |
| 15 | Hunt | +0x228 | |
| 16 | Unload | +0x23C | |
| 17 | Sabotage | +0x214 | |
| 18 | Construction | +0x244 | |
| 19 | Selling | +0x248 | |
| 20 | Repair | +0x24C | |
| 21 | Rescue | +0x258 (600) | |
| 22 | Missile | +0x250 | |
| 23 | Harmless | +0x208 | |
| 24 | Open | +0x254 | |
| 25 | Patrol | +0x25C | |

**Critical correction:** Mission 7 = "Enter" (not "Harvest"). Mission 10 = "Harvest".
The prior report labeled 0x004D9290 as "UnitClass__Mission_Harvest" but it is actually
**FootClass__Mission_Enter** (vtable+0x240, mission code 7). Corrected in this session.

The `Mission_Dispatch` function at `0x005B3060` reads `param_1[0x2B]` (offset 0xAC) as the
current mission code and dispatches to the corresponding vtable slot.

---

## 2. UnitClass::Mission_Harvest (0x0073E5E0)

**VTable slot:** +0x224 (mission code 10)
**UnitClass vtable at:** 0x007F5C70
**Confidence:** 95% (fully decompiled, 335 lines)

This is a **5-state state machine** driven by `param_1[0x2F]` (byte offset 0xBC).

### Preconditions (before entering state machine)

```c
iVar8 = param_1[0x1B1];  // UnitTypeClass* at offset 0x6C4
// Check if unit type has Harvester=yes or Weeder=yes AND has slave master
if (((TypeClass+0x5ED == 0) || (TypeClass+0x5EC == 0)) || (this+0x2D8 == 0)) {
    // Normal harvester path
    if (TypeClass+0xE0E == 0 && TypeClass+0xE0F == 0) {
        return 0x1C2;  // Not a harvester, return default timer
    }
```

Key type flags:
- `TypeClass+0xE0E` = **Harvester** (bool)
- `TypeClass+0xE0F` = **Weeder** (bool)
- `TypeClass+0xCD4` = **Teleporter** (bool) — chrono miner flag

The function first checks if the unit type has any dockable buildings (iterates
`TypeClass+0x3F8`, count of Dock= entries). If no dock types exist and not under
player control, assigns Guard mission (5) and returns.

### State 0: SCAN_FOR_ORE

**Purpose:** Find an ore patch and navigate to it.

**2026-07-24 live recheck:** `decompile_function(0x0073E5E0)`,
`decompile_function(0x004DD0A0)`,
`decompile_function(0x004DCE80)`, and
`decompile_function(0x00485020)` against retail
`gamemd.exe` SHA-256
`1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`
supersede the older shorthand below where noted.

1. **If full (storage >= 1.0):** Transition to state 2 (return to refinery).
   - Fullness checked via `vtable+0x2B4` (UnitClass__Get_Storage_Percentage at 0x007414A0)
   - Returns `current_load / TypeClass+0x800` (Storage capacity)

2. **Consume an archive/ghost cell if present:** Call `Set_Destination(archive, 1)`,
   clear the archive, and disable the zone restriction for the scan that follows.
   Separately, a live TeleportLocomotion destination is cleared before scanning.

3. **Scan for ore:**
   - **Weed eaters** (`TypeClass+0xE0F`): Call `FootClass__Search_For_Tiberium_Short_And_Move`
     (0x004DDB90) which uses `FootClass__Scan_For_Tiberium_NoZone` (0x004DD890)
   - **Chrono and normal harvesters** (`TypeClass+0xE0E` && not Weeder): after the
     chrono-locomotor cancellation check, both call
     `FootClass__Search_For_Tiberium_And_Move` (0x004DCFE0) with
     `TiberiumLongScan`. Active-binary reads at `0x0073E772` and `0x0073E851`
     both use `RulesClass+0x177C`; `TiberiumShortScan` is the state-1
     continuation radius, not a state-0 chrono special case.

4. **If search/move succeeds:** Set state 1, set `this+0x6D2` to 1, clear the
   accumulated step count, and initialize both timer duration fields to the
   literal value `2`. State 1 replaces that provisional value with
   `HarvesterLoadRate` only if it later observes a zero repeat duration.
   A `LandType == Tiberium` cell remains a successful candidate when
   `OverlayData == 0`; its value is `type.Value * (0 + 1)`.

5. **If no ore found:** Transition to state 4 only when destination and archive
   are both null. If a destination already exists, remain in state 0 and return
   the default mission timer.

### State 1: HARVESTING (extracting ore from current cell)

**Purpose:** Extract ore bales tick-by-tick from the current cell.

```c
// Initialize timer on first entry
if (param_1[0x43] == 0) {     // rate == 0 means first tick
    param_1[0x3E] = 0;          // step counter (offset 0xF8)
    param_1[0x43] = HarvesterLoadRate;  // rate = RulesClass+0x1520
    param_1[0x40] = g_CurrentFrameCounter;  // start frame
    param_1[0x42] = HarvesterLoadRate;  // current duration
}

// Wait for timer to accumulate 9 steps
if (param_1[0x3E] < 9) {
    return 1;  // Come back next tick
}
```

The RateTimer at offsets 0xF8-0x10C is a standard Westwood `StepTimerClass`:
- `+0xF8` (param_1[0x3E]) = accumulated steps
- `+0x100` (param_1[0x40]) = start frame
- `+0x104` (param_1[0x41]) = (unused/secondary)
- `+0x108` (param_1[0x42]) = current duration
- `+0x10C` (param_1[0x43]) = repeat duration/rate (frames per step)
- `+0x110` = amount added to `+0xF8` on expiry (constructed as `1`)

`TechnoClass::AI_Update` calls `Mission_Dispatch` before updating this timer.
At stock rate `2`, maintenance writes the ninth step after the `F+18` mission
call, so `Mission_Harvest` first calls `UnitClass__Harvest_Ore_Tick`
(`0x0073D450`) at `F+19`.

**On extraction failure** (no more ore on this cell):
- If chrono harvester and full: transition to state 2
- Otherwise: scan with TiberiumShortScan radius for nearby ore
  - If found: stay in state 1, reset timer
  - If not found: transition to state 2

### State 2: RETURN_TO_REFINERY

**Purpose:** Find nearest refinery, navigate to it, decide drive vs teleport.

1. **Find refinery:** `vtable+0x528` = `FootClass__Find_Docking_Bay` (0x004DF040)
   - Iterates TypeClass Dock= list
   - For each dock type, calls `vtable+0x52C` to find nearest building of that type
   - Picks closest one

2. **If no destination (not already moving to a refinery):**

   **Normal harvester** (`Teleporter=no`):
   ```c
   distance = sqrt(dx^2 + dy^2 + dz^2);
   if (distance <= RulesClass+0xD78 * 0x100) {
       // Close enough — reserve refinery
       Can_Enter_Building(Move, refinery);
       if (accepted) state = 3;
   }
   ```
   - `RulesClass+0xD78` = **HarvesterTooFarDistance** (default 5, in cells)
   - Multiplied by 0x100 (256 leptons/cell) for distance comparison

   **Chrono harvester** (`Teleporter=yes`):
   ```c
   distance = sqrt(dx^2 + dy^2 + dz^2);
   if (distance <= RulesClass+0xD7C * 0x100) {
       // Close enough — teleport is worthwhile
       Can_Enter_Building(Move, refinery);
       if (accepted) state = 3;
   }
   ```
   - `RulesClass+0xD7C` = **ChronoHarvTooFarDistance** (default 50, in cells)

3. **If too far:** Do a second search (fog-ignoring: `Find_Docking_Bay(type, 0, 1)`)
   - If found and (distance > 0x300 OR is chrono miner):
     - Pathfind to a cell near the refinery (uses `Pathfinding_validate_alternate`)
     - Navigate toward it

### State 3: ENTER_REFINERY

**Purpose:** Switch to Mission_Enter to dock at the refinery.

```c
(**(code **)(*param_1 + 0x1E8))(7, 0);  // Queue_Mission(Enter, false)
```

Simply assigns Mission_Enter (mission 7). The Mission_Enter handler at 0x004D9290
(FootClass__Mission_Enter) takes over and handles the approach and docking.

### State 4: NO_ORE_FOUND (wander)

**Purpose:** Handle the case where no ore could be found anywhere.

1. Sets `this+0x3D0` flag = 1 (marking "ore depleted" for AI)
2. If the unit type has `Harvester=yes`: sets `House+0x242` flag = 1
   (notifies house AI of an ore problem)
3. Checks current cell for adjacent refinery/weeder building
4. If found: moves to it
5. Assigns Guard mission (5) and returns with 0x69 (105 frames delay)

---

## 3. FootClass::Mission_Enter (0x004D9290)

**VTable slot:** +0x240 (mission code 7)
**Confidence:** 90% (decompiled)

Handles unit entering a building (refinery, repair pad, etc.)

### Logic Flow

1. **Check reachability:** `FUN_0065AD30(0)` — checks if unit can reach destination.
   `FUN_0040DD70` — secondary reachability check.

2. **If can't reach:**
   - If destination is null or not a valid building type: clear destination, process next mission
   - Otherwise keep trying

3. **If can reach:**
   - Calls `vtable+0x278(0xE, target)` — `Can_Enter_Building(Enter, target)`
   - **If accepted (return 1) or already docking:**
     - If no destination but waypoint queue has entries: pop next waypoint as destination
     - **If TypeClass+0xCD4 (Harvester) is set:** Clear internal dock references and
       re-set destination to the building (ensures correct approach path)
   - **If not accepted:**
     - Calls `vtable+0x274(3)` — approach/navigate closer
     - Clears via `vtable+0x484(0,1)`

4. Returns random timer (1-3 ticks jitter added to base timer)

---

## 4. Ore Scanning Algorithm

### 4.1 FootClass::Scan_For_Tiberium (0x004DD0A0)

**VTable slot:** +0x338
**Confidence:** 95% (fully decompiled)
**Used by:** Both normal and chrono harvesters from Mission_Harvest state 0

**Pattern:** Diamond/rhombus spiral expanding outward.

```
Parameters:
  param_2 = scan radius in cells (e.g., TiberiumShortScan=6)

Algorithm:
  Start at unit's current cell
  If current cell has LandType == Tiberium (5): return immediately
  (OverlayData 0 remains eligible)

  For radius = 1 to param_2:
    best_value = -1
    For offset = -radius to +radius:
      Check 4 cells per offset (forming diamond perimeter):
        (x + offset, y - radius)   -- top
        (x + offset, y + radius)   -- bottom
        (x - radius, y + offset)   -- left
        (x + radius, y + offset)   -- right

      For each cell:
        1. Call FootClass__Is_Cell_Harvestable (0x004DCE80):
           - Cell must be in playfield
           - Cell not shrouded (single player only)
           - Unit's SpeedType can traverse the cell
           - Can_Enter_Cell returns passable
           - CellClass.LandType == 5 (Tiberium)
        2. If valid: get CellClass__Get_Tiberium_Value (0x00485020)
        3. Track cell with highest value

    If any ore found at this radius: return best cell (break early)

  Return best cell found (or invalid coords if none)
```

**Key:** Scans from innermost ring outward. Stops at the first radius that
contains ore, but picks the highest-value cell within that ring.

### 4.2 FootClass::Scan_For_Tiberium_NoZone (0x004DD890)

**Confidence:** 90% (decompiled)
**Used by:** Weed eaters (from Mission_Harvest state 0)

Same diamond spiral pattern as Scan_For_Tiberium, but:
- Calls FUN_004DD9F0 for cell validation instead of Is_Cell_Harvestable
- FUN_004DD9F0 checks `CellClass.LandType == 0xB (Weeds)` instead of `== 5 (Tiberium)`
- Also requires `CellClass+0x11E > 0x2F` (overlay data > 47)
- Returns first valid cell found (does NOT optimize for highest value)

### 4.3 FootClass::Search_For_Tiberium_And_Move (0x004DCFE0)

**Confidence:** 90% (decompiled)
**Used by:** Normal harvesters (from Mission_Harvest state 0)

Wrapper that:
1. Calls `vtable+0x338` (Scan_For_Tiberium) with given radius
2. If found cell != current cell: sets it as destination via `vtable+0x480`
3. Returns success/failure

### 4.4 FootClass::Search_For_Tiberium_Short_And_Move (0x004DDB90)

**Confidence:** 90% (decompiled)
**Used by:** Normal harvesters (from Mission_Harvest state 1, continuation scan)

Same as above but calls Scan_For_Tiberium_NoZone instead of Scan_For_Tiberium.

---

## 5. Ore Extraction (Per-Bale Harvesting)

### 5.1 UnitClass::Harvest_Ore_Tick (0x0073D450)

**Confidence:** 90% (decompiled)

Called from Mission_Harvest state 1 when the step timer reaches 9.

1. Gets CellClass at unit's position
2. If unit has a destination set: return 1 (moving, don't harvest)
3. **If not a chrono harvester, OR storage < 1.0, OR cell LandType != Tiberium:**
   - Reset timer, return failure (no harvest)
4. **If Weeder=yes:**
   - Calls `FUN_00486E30` (reduce weed from cell)
   - Adds 1.0 to storage: `StorageClass__Add_Amount(1.0, 0)`
   - Timer delay = HarvesterLoadRate * 3 (weeders are slower)
5. **Normal ore extraction:**
   - Gets tiberium type index from cell (`FUN_00485010` = overlay-to-tiberium-type lookup)
   - Gets current load via `FUN_006C9650` (sum of storage floats)
   - Calculates remaining capacity: `TypeClass+0x800 - current_load`
   - Clamps to available ore
   - Calls `CellClass__Reduce_Tiberium` (0x00480A80) to extract bales from the cell
   - Adds extracted amount to storage: `StorageClass__Add_Amount(amount, tib_type)`
   - Timer delay = HarvesterLoadRate

### 5.2 CellClass::Reduce_Tiberium (0x00480A80)

**Confidence:** 95% (decompiled)

Removes ore from a cell:
- `CellClass+0x11E` = overlay data (ore density, 0-11)
- If `param_2` (amount) < density+1: decrements `field_0x11E` by amount
- If `param_2` >= density+1: removes overlay entirely:
  - Sets `OverlayTypeIndex = -1`, `field_0x11E = 0`
  - Calls RecalcAttributes
  - Re-evaluates adjacent cells for ore spreading
- Returns number of bales actually extracted
- Triggers screen redraw on affected area

### 5.3 CellClass::Get_Tiberium_Value (0x00485020)

**Confidence:** 95% (decompiled)

```c
int CellClass::Get_Tiberium_Value() {
    int overlay_idx = IsWallOverlay();  // gets overlay index
    if (overlay_idx == -1) return 0;
    return OverlayTypeClass[overlay_idx]+0xB8 * (this->field_0x11E + 1);
}
```

Value = base value per overlay type * (density + 1).

---

## 6. LandType Enum

**Table address:** 0x0081DA28 (pointer array)
**Confidence:** 99% (verified from RecalcAttributes and SpeedType_TablePopulator)

| Index | Name | Notes |
|-------|------|-------|
| 0 | Clear | Default ground |
| 1 | Road | Paved surfaces |
| 2 | Water | Naval passable |
| 3 | Rock | Impassable cliff |
| 4 | Wall | Wall overlays |
| 5 | **Tiberium** | Ore (Riparius/Vinifera overlays) |
| 6 | Beach | Shore transition |
| 7 | Rough | Rough terrain |
| 8 | Ice | Frozen surfaces |
| 9 | Railroad | Rail tracks |
| 10 | Tunnel | Tunnel entries |
| 11 | **Weeds** | Weed overlays (Veinholeish stuff) |

**CellClass+0xEC** stores the LandType for the cell. Set from `OverlayTypeClass+0x298`
(the `Land=` key in the overlay type's INI entry) via `CellClass::RecalcAttributes` at
0x0047D2B0.

---

## 7. RulesClass Harvester Offsets

| Offset | Field | Type | Default | Read At |
|--------|-------|------|---------|---------|
| +0x0D78 | HarvesterTooFarDistance | int | 5 | 0x0066FFE3 |
| +0x0D7C | ChronoHarvTooFarDistance | int | 50 | 0x00670003 |
| +0x1520 | HarvesterLoadRate | int | 2 | 0x00670CF4 |
| +0x1528 | HarvesterDumpRate | double | 0.016 | 0x00670CD4 |
| +0x1778 | TiberiumShortScan | int | 6 | 0x00670299 |
| +0x177C | TiberiumLongScan | int | 48 | 0x006702B8 |
| +0x1780 | SlaveMinerShortScan | int | | 0x006702D9 |
| +0x1784 | SlaveMinerSlaveScan | int | | 0x006702F8 |
| +0x1788 | SlaveMinerLongScan | int | | 0x00670317 |
| +0x178C | SlaveMinerScanCorrection | int | | 0x00670336 |
| +0x1790 | SlaveMinerKickFrameDelay | int | | 0x00670355 |

---

## 8. TechnoTypeClass Harvester Flags

| Offset | Field | Type | INI Key | ReadINI At |
|--------|-------|------|---------|------------|
| +0x0800 | Storage | int | Storage | 0x00713130 |
| +0x0CD4 | Teleporter | bool | Teleporter | 0x00713FE9 |
| +0x0E0E | Harvester | bool | Harvester | 0x007476A6 |
| +0x0E0F | Weeder | bool | Weeder | 0x007476BF |

---

## 9. CellClass Ore Fields

| Offset | Field | Type | Notes |
|--------|-------|------|-------|
| +0x44 | OverlayTypeIndex | int (short?) | Index into OverlayTypeClass array, -1 = none |
| +0xEC | LandType | int | Enum from table at 0x81DA28, 5=Tiberium, 11=Weeds |
| +0x11E | OverlayData | byte | Ore density/frame (0-11), decremented during extraction |

---

## 10. FootClass/UnitClass Harvester Instance Fields

| Byte Offset | int* Index | Field | Notes |
|-------------|------------|-------|-------|
| 0xAC | [0x2B] | CurrentMission | Mission enum value (7=Enter, 10=Harvest) |
| 0xB4 | [0x2D] | QueuedMission | Next mission to execute |
| 0xBC | [0x2F] | MissionSubState | Harvest state machine phase (0-4) |
| 0xF8 | [0x3E] | StepTimer.Steps | Accumulated steps (wait until 9) |
| 0x100 | [0x40] | StepTimer.StartFrame | Frame when timer started |
| 0x108 | [0x42] | StepTimer.Duration | Current duration until the next expiry |
| 0x10C | [0x43] | StepTimer.RepeatDuration | Recurring rate (= HarvesterLoadRate); zero triggers state-1 initialization |
| 0x110 | — | StepTimer.StepAmount | Amount added to Steps on expiry (constructed as 1) |
| 0x218 | [0x86] | Archive | Saved target/ore field location |
| 0x21C | [0x87] | Owner | HouseClass pointer |
| 0x2B4 | [0xAD] | Target | Current TechnoClass target |
| 0x2D8 | [0xB6] | SlaveMaster | Slave miner master reference |
| 0x2E4 | [0xB9] | Disabled/EMP | Non-zero when unit is disabled |
| 0x5A4 | [0x169] | Destination | Current movement destination |
| 0x690 | (byte) | HeadingToDock | Flag set when heading to refinery |
| 0x6C4 | [0x1B1] | Type | UnitTypeClass pointer |
| 0x6D2 | (byte) | IsHarvesting | Flag set during active ore extraction |

---

## 11. Complete Harvester Lifecycle

```
1. Harvester spawns → Mission = Guard (5)
2. Player/AI orders harvest → Mission = Harvest (10), State = 0
3. State 0 (SCAN):
   - Scan for ore using diamond spiral (TiberiumLongScan radius)
   - If found → move to ore cell, State = 1
   - If not found → State = 4 (wander)
4. State 1 (HARVEST):
   - Wait 9 timer steps (each step = HarvesterLoadRate frames)
   - Extract ore from cell via Reduce_Tiberium
   - Add to storage (4-element float array by tiberium type)
   - If cell depleted → short scan (TiberiumShortScan radius)
     - Found more → stay State 1
     - Not found → State 2
   - If full → State 2
5. State 2 (RETURN):
   - Find nearest refinery via Find_Docking_Bay
   - Normal harvester: if distance <= HarvesterTooFarDistance*256 → State 3
   - Chrono miner: if distance <= ChronoHarvTooFarDistance*256 → State 3
   - If too far: navigate toward refinery area, keep trying
6. State 3 (DOCK):
   - Assigns Mission_Enter (7)
   - FootClass::Mission_Enter handles approach and docking
   - Refinery receives harvester, unloads via HarvesterDumpRate
   - After unload: undock, Mission = Harvest (10), State = 0 (cycle restarts)
7. State 4 (LOST):
   - No ore found anywhere
   - Sets ore-depleted flag for AI
   - Assigns Guard mission, waits
```

---

## 12. Ghidra Functions Labeled This Session

| Address | Name | Purpose |
|---------|------|---------|
| 0x0073E5E0 | UnitClass__Mission_Harvest | 5-state harvest mission handler |
| 0x004D9290 | FootClass__Mission_Enter | Enter/dock mission handler |
| 0x004DD0A0 | FootClass__Scan_For_Tiberium | Diamond spiral ore scanner (zone-aware) |
| 0x004DD890 | FootClass__Scan_For_Tiberium_NoZone | Diamond spiral weed scanner |
| 0x004DCFE0 | FootClass__Search_For_Tiberium_And_Move | Scan + move wrapper |
| 0x004DDB90 | FootClass__Search_For_Tiberium_Short_And_Move | Short scan + move wrapper |
| 0x0073D450 | UnitClass__Harvest_Ore_Tick | Per-bale extraction logic |
| 0x00480A80 | CellClass__Reduce_Tiberium | Remove ore from cell |
| 0x00485020 | CellClass__Get_Tiberium_Value | Ore value = type_value * (density+1) |
| 0x004DCE80 | FootClass__Is_Cell_Harvestable | Cell validation for ore scanning |
| 0x004DF040 | FootClass__Find_Docking_Bay | Find best building from Dock= list |
| 0x006C9690 | StorageClass__Add_Amount | storage[type] += amount |
| 0x007414A0 | UnitClass__Get_Storage_Percentage | current_load / max_capacity |
| 0x004D6AA0 | FootClass__Mission_Harvest | Base class harvest handler (complex, 230 lines) |
| 0x005B35E0 | MissionClass__Queue_Mission | Sets queued mission + optional force |
| 0x005B3060 | Mission_Dispatch | Main mission switch dispatcher |

---

## 13. Notes on Prior Report Corrections

The [HARVESTER_DOCK_UNLOAD.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/miner/HARVESTER_DOCK_UNLOAD.md) report contains a misidentification:
- `0x004D9290` was labeled `UnitClass__Mission_Harvest` — it is actually
  **FootClass__Mission_Enter** (mission 7 = Enter)
- `0x00739EC0` was labeled `UnitClass__Mission_Enter` — this may be a different function
- The actual UnitClass::Mission_Harvest is at **0x0073E5E0**
- Find_Nearest_Dock assigns mission **8** (Capture), not "Enter". This appears to be
  the mission code used for approaching and capturing/entering buildings. The comment
  "Mission_Enter" in the prior report should be "Mission_Capture" (code 8).

**Note on mission 8:** The name table says "Capture" but Find_Nearest_Dock uses it for
docking harvesters. In the RA2 engine, "Capture" mission may serve double duty for
"approach and enter building" actions, not just engineer capturing.
