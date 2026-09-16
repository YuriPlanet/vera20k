# Harvester Docking & Unloading Mechanics — Research Report

Reverse-engineered from `gamemd.exe` via Ghidra MCP. All addresses reference the
YR executable. Confidence levels noted per finding.

---

> **Correction 2026-05-21 - stock refinery DockUnload**
>
> This report is older than the focused 2026-05-20/2026-05-21 refinery radio
> investigations. For stock `CMIN/HARV -> GAREFN/NAREFN`, the normal DockUnload
> path does not establish reciprocal `unit/building +0x2E4`, and TechnoClass
> radio `0x18/0x19` toggles `+0x418`, not `+0x2E4`. Stock dump completion exits
> through `UnitClass::Mission_Deploy_Building` state 4 with `unit+0x2E4 == 0`,
> not through `ReleaseDockedHarvester` / `Force_Track(0x47)`. Keep the function
> body notes below as historical evidence, but prefer
> [RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_GHIDRA_REPORT.md) plus its 2026-05-21
> correction note, [STANDARD_REFINERY_0X2E4_WRITER_INVENTORY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/STANDARD_REFINERY_0X2E4_WRITER_INVENTORY_GHIDRA_REPORT.md),
> and [CHRONO_MINER_FORCE_TRACK_0X47_EXIT_NAVCOM_STEP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONO_MINER_FORCE_TRACK_0X47_EXIT_NAVCOM_STEP_GHIDRA_REPORT.md) for the
> current stock refinery verdict.
>
> **Correction 2026-05-22 - stock unload and queue follow-ups**
>
> Also prefer [STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md),
> [CHRONO_MINER_REFINERY_CONTACT_SATURATION_QUEUE_EVICTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONO_MINER_REFINERY_CONTACT_SATURATION_QUEUE_EVICTION_GHIDRA_REPORT.md), and
> [miner/traces/CHRONO_MINER_FULL_CARGO_CLOSE_RETURN_MISSION_DISPATCH_TIMING_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/traces/CHRONO_MINER_FULL_CARGO_CLOSE_RETURN_MISSION_DISPATCH_TIMING_TRACE.md).
> These close the remaining stock-path issues: `RadioClass::In_Radio_Contact (formerly mislabeled PathType__Has_Valid_Steps)()`
> true continues to RateTimer/state dispatch; false performs cleanup and returns
> `1`; stock refinery full `HELLO(0x02)` returns `10` without evicting receiver
> contacts; sender-side HELLO eviction only evicts the sender's old contact; and
> `QueueingCell=4,1` is fallback/staging data, not the accepted `0x0E` cell.

## 1. Docking System

### 1.1 Docking Pad Coordinates (BuildingTypeClass)

**Function:** `BuildingTypeClass_ReadINI_Water` at `0x0045FE50`
**Confidence:** 95% (verified from INI parsing code)

| INI Key | BuildingTypeClass Offset | Type | Description |
|---------|--------------------------|------|-------------|
| `NumberOfDocks` | `+0x1780` | int | Number of dock pads on this building |
| dock pad array ptr | `+0x1784` | DynVecClass* | VectorClass managing dock pad storage |
| dock pad data ptr | `+0x1788` | int* | Raw array of dock pad coordinate data |
| `DockingOffset%d` | `+0x1788[i*12]` | 3x int (12 bytes each) | X, Y, Z offset for dock pad `i` |

Each dock pad is a 12-byte structure: `{int x, int y, int z}` representing the isometric
pixel offset from the building origin. The INI key `DockingOffset%d` is read per dock
(0-indexed), parsed via `CCINIClass::Read3Int` (at `0x00529CA0`).

Assembly evidence (at `0x0046491C`):
```asm
ADD EDI,0x44    ; stride between dock iteration vars (0x44 = 68 bytes of local state)
; Dock data itself is 0xC (12) bytes per pad
```

The loop at `0x004649AF`–`0x00464A41` iterates `NumberOfDocks` times, reading
`DockingOffset%d` for each pad into the coordinate array at `+0x1788`.

### 1.2 BuildingClass::CanDock (0x00457CE0)

**Confidence:** 85% (decompiled, some checks unclear)

Checks whether a unit can dock at this building. Conditions:
1. Unit (param_2) must be non-null
2. `BuildingTypeClass + 0x157B` must be non-zero (IsGarrisoned/Capturable flag check)
3. Mission must not be 0x12 (Selling) or 0x13 (Construction)
4. Calls `FUN_005785F0` (cell validity check) on building position
5. Building must not be deactivated (vtable+0x1D4 returns false)
6. For Refinery: checks `Type + 0xEB4` (Refinery acceptance flag) — if set, verifies same owner or ally
7. For UnitRepair: checks `Type + 0xEB5` and alliance
8. Building must not be red-HP

### 1.3 FootClass::Find_Nearest_Dock (0x004DFCB0)

**Confidence:** 90% (decompiled)

Iterates the harvester owner's building list (`this[0x87]+0xE4` array, count at `+0xF0`).
For each building:
1. Computes 3D Euclidean distance using `Math::sqrt`
2. Calls `BuildingClass::CanDock` to validate
3. Picks the nearest valid dock

When found:
- Sets `this + 0x690` (`field_0x1A4 * 4`) flag = 1
- If not already moving to this dock: calls vtable+0x480 (SetDestination) and vtable+0x1E8 (SetMission) with mission **8** (Mission_Enter)

### 1.4 BuildingClass::EnterTransport (0x0070FD70)

**Confidence:** 90% (decompiled, called from TechnoClass::Fire_At)

When a harvester reaches the refinery cell:
1. `unit + 0x1D0` = building pointer (link unit → building)
2. `unit[0x73]` (offset `0x1CC`) = building pointer (link on unit side)
3. Sets `House + 0x5778` dirty flag
4. If building has sound (`Type + 0x2BC`), plays enter sound
5. Creates animation from `RulesClass + 0x31C` (Dock animation) at building position
6. Stores anim pointer at `unit[0x75]` (offset `0x1D4`)
7. If unit has cloaking device, decloaks via `FUN_006EA870`

### 1.5 DockUnload Flag

**INI Key:** `DockUnload`
**BuildingTypeClass Offset:** `+0x16B3` (bool)
**Confidence:** 95% (verified from ReadINI at `0x004609DD`)

When `DockUnload=yes`, the building acts as a refinery-style dock where units enter,
unload cargo, and exit. Standard refineries (GAREFN, YAREFN) have this set.

---

## 2. Unloading / Credit Transfer

### 2.1 HarvesterDumpRate

**INI Key:** `HarvesterDumpRate`
**RulesClass Offset:** `+0x1528` (8 bytes, **double**)
**Default Value:** `0.016` (minutes per bail)
**Confidence:** 99% (verified: INI read at `0x00670CD4`, default from constructor at `0x006673D4` = `0x3f90624dd2f1a9fc` = 0.016)

This is a **double-precision float** read via `CCINIClass::ReadDouble`.

**Conversion formula (original engine):**
```
HarvesterDumpRate = 0.016 minutes/bail
= 0.016 * 60 = 0.96 seconds/bail
At 15 fps game logic rate: 0.96 * 15 = 14.4 frames/bail
```

The existing Rust engine uses `unload_tick_interval: 14` which is correct (truncated from 14.4).

### 2.2 HarvesterLoadRate

**INI Key:** `HarvesterLoadRate`
**RulesClass Offset:** `+0x1520` (4 bytes, **int**)
**Default Value:** `2`
**Confidence:** 95% (verified from INI read at `0x00670CF4` and constructor default)

This is an **integer** read via `CCINIClass::ReadInt`. It represents ticks/frames between
each harvest bale pickup. Note: NOT used as minutes — directly as frame count.

### 2.3 Credit Transfer Flow — CORRECTED

**Confidence:** 95% (verified from decompiled `UnitClass::Mission_Deploy_Building` at `0x0073D630`)

**CORRECTION:** The previous version stated the credit dump was in `MissionRepairAndProduce`
via the building's radio command system. This was wrong. The dump logic lives entirely on
the **unit side** in `UnitClass::Mission_Deploy_Building` (`0x0073D630`), which handles
both MCV deployment AND harvester ore dumping.

**Complete flow:**

1. Harvester reaches refinery → `BuildingClass::EnterTransport` (`0x0070FD70`) links unit ↔ building
2. Unit enters Mission_Deploy_Building sub-state 3 (dumping state)
3. A **StepTimer** at `UnitClass+0xF8` auto-increments each frame (CDTimer at +0x100 fires every frame)
4. Each tick: checks `Steps >= HarvesterDumpRate * 900.0`:
   - `HarvesterDumpRate` = 0.016 (double at `Rules+0x1528`) = minutes per bale
   - Multiplied by constant 900.0 (at `0x007E27F8`) = 60sec × 15fps
   - Threshold = 0.016 × 900 = **14.4 frames per bale**
5. When threshold reached:
   a. `StorageClass::FindFirstNonEmpty` (`0x006C9820`) — find ore type in cargo
   b. `StorageClass::GetAmount` (`0x006C9680`) — get amount of that ore type
   c. Compute credit value: `base_ore_value * Rules->OreMultiplier * ore_amount`
      - AI bonus: non-human players get difficulty-based bonus from `Rules+0x1324` table
   d. `StorageClass::Remove` (`0x006C96B0`) — subtract one bale from cargo
   e. `HouseClass::DepositOreCredits` (`0x004F9610`) — add credits + update display counter
      (Weeder uses `HouseClass::DepositWeedCredits` at `0x004F9700` instead)
   f. Reset Steps counter to 0
6. When all bales dumped (FindFirstNonEmpty returns -1): transition to undock state

**Key assembly at 0x0073E355–0x0073E374:**
```asm
MOV EDX, [0x008871E0]          ; EDX = g_RulesClass_Instance
FILD dword ptr [ESI + 0xF8]    ; push Steps counter as float
FLD qword ptr [EDX + 0x1528]   ; push HarvesterDumpRate (double)
FMUL qword ptr [0x007E27F8]    ; multiply by 900.0
FCOMPP                          ; compare: threshold vs steps
```

**Note:** `MissionRepairAndProduce` handles UnitRepair (Service Depot), Hospital, Armory,
Bunker, ConstructionYard, and UnitReload — but NOT Refinery. The Refinery flag (`0x16BB`)
is never checked in that function.

### 2.4 Storage Capacity

**INI Key:** `Storage`
**TechnoTypeClass Offset:** `+0x800` (param_1[0x200], int)
**Confidence:** 95% (verified from ReadINI at `0x00713130`)

Maximum ore storage capacity in credits. War Miner = 1000, Chrono Miner = 500.
At 25 credits per ore bale: War Miner = 40 bales, Chrono Miner = 20 bales.

### 2.5 UnloadingClass

**INI Key:** `UnloadingClass`
**TechnoTypeClass Offset:** `+0x6B8` (param_1[0x1AE], VoxelAnimType index)
**Confidence:** 90% (verified from ReadINI at `0x007146E8`, looked up via `FUN_007480D0`)

A VoxelAnimType played when the harvester's bay is open/empty during unloading.
For Chrono Miner: `CMON` (Chrono Miner Open — empty bay animation).

---

## 3. Refinery Building Logic

### 3.1 Key BuildingTypeClass Flags

| INI Key | Offset | Type | Description |
|---------|--------|------|-------------|
| `Refinery` | `+0x16BB` | bool | Building is an ore refinery |
| `DockUnload` | `+0x16B3` | bool | Units dock here to unload cargo |
| `Weeder` | `+0x16BC` | bool | Building is a weed harvesting refinery |
| `Helipad` | `+0x16CB` | bool | Aircraft dock/reload pad |
| `UnitRepair` | `+0x16A9` | bool | Service depot (repair pad) |
| `UnitReload` | `+0x16AA` | bool | Ammo reload pad |
| `FreeUnit` | `+0xEA0` | UnitType* | Unit spawned on construction (harvester for refineries) |
| `WeaponsFactory` | `+0x16BD` | bool | Can produce vehicles |

### 3.2 Free Harvester Spawning

**Function:** `BuildingClass::OnConstructionComplete` at `0x00445F80`
**Confidence:** 90% (decompiled, flow verified)

When a refinery finishes construction:
1. Checks `BuildingTypeClass + 0xEA0` (FreeUnit) is non-zero
2. Skips if: map editor mode, loading from save, or certain campaign flags
3. Calls `UnitClass::Constructor` with the FreeUnit type and the building's owner
4. Places the unit near the building with initial facing **0xC0** (south)
5. If placement fails, refunds the unit cost via `HouseClass::GiveMoney`

### 3.3 Multiple Harvesters / Queuing

The dock system uses a multi-dock array at `BuildingClass + 0xE4/0xF0` (RadioClass fields).
Each dock slot (`param_1[0x39]` = dock array, `param_1[0x3A]` = dock count) can hold one
unit pointer.

RadioCommand handler at `0x0065A970`:
- **Command 2 (RADIO_DOCKING):** Finds empty dock slot, links unit. If full, sends RADIO_NEGATIVE.
- **Command 3 (RADIO_CLEAR):** Clears unit from dock slot.

The `NumberOfDocks` field determines how many units can dock simultaneously (typically 1
for refineries, but the system supports multiple).

### 3.4 Radio Command Protocol (Complete)

**Confidence:** 95% (decompiled from `BuildingClass::Receive_Radio` at `0x0043C2D0` and
`UnitClass::Receive_Radio` at `0x00737430`)

The docking system uses a numbered radio command protocol. The dispatcher is:
- `vtable+0x274` (`RadioClass::Transmit_Radio_ToFirst` at `0x0065ACB0`) — send to first docked
- `vtable+0x278` (`RadioClass::Transmit_Radio` at `0x0065AAA0`) — send to target
- `vtable+0x27C` (`RadioClass::Transmit_Radio_Impl` at `0x0065A970`) — dock array management
- `vtable+0x280` (`RadioClass::Broadcast_Radio_ToAll` at `0x0065ACE0`) — send to all docked

Commands 2 and 3 are handled by the base `Transmit_Radio_Impl` (dock array management).
All other commands are forwarded to the target's `vtable+0x194` (Receive_Radio override).

**Return values:** 0 = rejected, 1 = accepted, 5 = negative-busy, 10 = negative, 0x17 = redirect

#### Radio Command Map

| # | Hex | Name | Building Handler | Unit Handler |
|---|-----|------|-----------------|--------------|
| 2 | 0x02 | DOCK_LINK | Base: add to dock array | — |
| 3 | 0x03 | CLEAR_LINK | Base: remove from dock array | If on mission 0xC, go to Guard |
| 7 | 0x07 | COME_TO_ME | — | Stop, sleep, radio 2→sender, radio 0x18→sender |
| 8 | 0x08 | CLOSE_ENOUGH? | UnitRepair/Bunker: check < 0x180 leptons | — |
| 0xB | 0x0B | OPEN_DOOR | Set mission to Open (0x14) | — |
| 0xC | 0x0C | CLOSE_DOOR | Return to Guard, clear CY anims | — |
| 0xD | 0x0D | IS_FACTORY? | WeaponsFactory → yes | — |
| 0xE | 0x0E | CAN_DOCK? | Complex: checks all building type flags, returns dock cell | Complex: checks capacity, zone, land type |
| 0xF | 0x0F | CAN_ENTER? | UnitAbsorb/InfAbsorb/Grinding/Bunker/Repair/Hospital/Armory/Helipad/Dock/Weeder | Capacity + alliance check |
| 0x10 | 0x10 | ARE_FREE? | Yes if no cargo AND is Refinery/UnitRepair/Weeder | — |
| 0x12 | 0x12 | ASSIGN_CELL | (Sub-protocol) Cell position negotiation | — |
| 0x13 | 0x13 | IS_DOCKED? | (Sub-protocol) Check if unit in dock array | — |
| 0x15 | 0x15 | PREPARE | UnitRepair/Hospital/Armory→Open+Sleep; Bunker→Open; DockUnload→mission 0x10 | Cargo full → play VXL anim |
| 0x16 | 0x16 | TIMING_SYNC | — | First ordinary call syncs active locomotor/RateTimer with `0x4000` and returns; later eligible call can send `0x15`. It does not stop, call `GetDockCoord`, set destination, write position, start unload, or directly set body facing. |
| 0x17 | 0x17 | REDIRECT | — | Harvester/Weeder: scatter, resume harvest |
| 0x18 | 0x18 | ACCEPTED | (Sub-protocol) Dock negotiation confirmed | — |
| 0x22 | 0x22 | SLOT_TAKEN? | (Sub-protocol) Check if slot has occupant | — |
| 0x23 | 0x23 | SLOT_FREE? | (Sub-protocol) Is there an empty dock slot? | — |
| 0x24 | 0x24 | CAN_DEPLOY? | — | Bridge check, deploy flag check |

**2026-05-22 command-map caveat:** For standard `CMIN/HARV -> GAREFN/NAREFN`,
radio `0x10` is receiver-live but has no standard sender; the `0x10` reached
after pad arrival is a queued mission id from building case `0x15`, not a radio
message. Radio `0x17` is live for factory/repair/bunker-style queues, but not the
normal stock ore-refinery busy response. TechnoClass `0x18/0x19` toggles
`+0x418`, not reciprocal `+0x2E4`.

**2026-05-26 `0x16` caveat:** The old `FACE_DOCK` wording was wrong. Radio
`0x16` is not the unload-start owner and is not a direct East body-facing write.
The deploy-facing gate belongs to mission `0x10` / `Mission_Deploy_Building`,
which checks the `RateTimer` window before setting the unload-active display
latch.

**TechnoClass base commands (at `0x006F4AB0`):**

| # | Hex | Name | Description |
|---|-----|------|-------------|
| 0x18 | TOGGLE_DOCK | Set "deploying/docking" flag, forward to sender |
| 0x19 | CANCEL_DOCK | Clear "deploying/docking" flag |
| 0x1A | SET_ALT_FLAG | Set alternate state flag |
| 0x1B | CLEAR_ALT_FLAG | Clear alternate state flag |
| 0x1C | REPAIR_TICK | **Per-tick Service Depot repair**: spend cost (vtable+0xB0), add health (vtable+0xB4, min 1). Returns 0x20=can't afford, 0x21=repair complete, 1=continue |
| 0x1E | SET_TARGET | Give attack target, set mission to Attack (1) |
| 0x1F | RELOAD_AMMO | Add 1 ammo. Returns 10 when full, 1 when added |

**Repair via radio 0x1C (TechnoClass, `0x006F4AB0` case 0x1C):**
```c
// Only repairs if below ConditionYellow threshold (Rules+0x16F8)
cost = TypeClass->GetRepairCost();      // vtable+0xB0
step = TypeClass->GetRepairStep();      // vtable+0xB4 (min 1)
if (owner.CanAfford(cost)) {
    owner.SpendMoney(cost);
    this.Health += step;
    if (Health >= MaxHealth) return 0x21;  // REPAIR_COMPLETE
    return 1;                               // CONTINUE
}
return 0x20;  // INSUFFICIENT_FUNDS
```

**Building Receive_Radio (case 0x0F) acceptance matrix:**

| Flag | Accepts | Extra Checks |
|------|---------|-------------|
| UnitAbsorb (0x16AE) | UnitClass | Capacity, size (Type+0x380 ≤ Type+0x388) |
| InfantryAbsorb (0x16AF) | InfantryClass | Capacity, size |
| Grinding (0x16AD) | Any | Always returns 1 |
| Bunker (0x16AB) | Infantry | CanAutoDeployHere + slot available (radio 0x23) |
| UnitRepair (0x16A9) | Unit/Aircraft | Slot available (radio 0x23) |
| Hospital (0x16C1) | Infantry | Not mind-controlled, capacity (field_0x2FC) |
| Armory (0x16C2) | Infantry | Not mind-controlled, capacity |
| Helipad (0x16CB) | Aircraft | — |
| DockUnload (0x16B3) | Unit+Harvester | Has cargo (field_0x118 > 0) |
| Weeder (0x16BC) | Unit+Weeder | Has cargo |

### 3.5 Refinery Destruction While Docked

When a building is destroyed, `BuildingClass::OnDestroyed` (at `0x00445880`) is called,
which broadcasts Radio command 3 (CLEAR) to all docked/linked units, releasing them.
The docked harvester gets its dock link cleared and returns to idle/guard state.

---

## 4. Undock Facing — EXIT_FACING = 0x47

### BuildingClass::UndockUnit (0x004593A0)

> **Stock-path caveat:** This function remains valid for conditional
> reciprocal-link, interrupt, or non-stock release paths. It is not the normal
> stock `CMIN/HARV -> GAREFN/NAREFN` dump-completion exit; stock completion uses
> zero-link `Mission_Deploy_Building` state 4.

**Confidence:** 99% (fully decompiled and verified)

```c
void BuildingClass::UndockUnit(int* this) {
    int* docked_unit = this[0xB9];  // offset 0x2E4
    if (docked_unit != NULL) {
        if (docked_unit->IsAlive()) {
            ILocomotion* loco = docked_unit->Locomotor;
            loco->Stop();

            // Get building center coords
            CoordStruct* center = this->GetCoords();

            // Head_To with:
            //   facing = 0x47 (71 decimal ≈ ESE)
            //   offset = (-0x80, +0x80, 0) = (-128, +128) leptons
            loco->Head_To(0x47, center.X - 0x80, center.Y + 0x80, center.Z);

            // Set speed to 1.0 (full speed)
            docked_unit->SetSpeedMultiplier(1.0);

            // Clear dock links on both sides
            docked_unit[0xB9] = 0;  // unit's dock ref
            this[0xB9] = 0;         // building's dock ref

            // Notify production system (radio command 3 = CLEAR)
            this->RadioCommand(3);
        }
    }
}
```

The exit facing **0x47 = 71** in RA2's 0-255 facing system. Converting:
- 0 = North, 64 = East, 128 = South, 192 = West
- 71 = East-southeast (~100 degrees clockwise from north)

The offset `(-0x80, +0x80)` = `(-128, +128)` leptons pushes the unit one cell
southeast of the building center — the standard refinery exit path.

---

## 5. War Miner Turret While Harvesting

### UnitTypeClass ReadINI (0x00747620)

**Confidence:** 90%

The War Miner has:
- `Harvester=yes` at UnitTypeClass offset `+0xE0E`
- `Turret=yes` (standard TechnoType turret flag)
- Weapons assigned normally (20mm vulcan)

**2026-05-21 correction:** The write below is real, but it is **not** a write
to the normal `ROT=` facing-rate field. [CMIN_RUNTIME_ROT_PARSER_OVERRIDE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CMIN_RUNTIME_ROT_PARSER_OVERRIDE_GHIDRA_REPORT.md)
verifies `ROT=` parses into `TechnoTypeClass+0x71C` and remains the stock value
(`5` for HARV/CMIN). The harvester/weeder branch writes separate
`UnitTypeClass+0x398 = 10`; the exact wider gameplay semantic of `+0x398` is
deferred.

When `Harvester=yes` or `Weeder=yes` is set, `UnitTypeClass::ReadINI` writes:
```c
if (*(char*)(param_1 + 0xE0E) != '\0' || *(char*)(param_1 + 0xE0F) != '\0') {
    *(int*)(param_1 + 0x398) = 10;  // separate harvester/weeder field, not ROT=
}
```

The `NoAutoFire` flag is NOT set for War Miners. The turret/weapon system operates
independently of the harvester mission state machine. The War Miner CAN:
- Fire while moving to ore fields (Mission_Harvest)
- Fire while harvesting (extracting bales)
- Fire while returning to refinery
- Fire while docked at refinery (if threat detected in range)

The only restriction is mission priority: combat missions take precedence over
harvesting, so if the War Miner retaliates, it temporarily interrupts harvesting
until the threat is cleared or it disengages.

---

## 6. Key Struct Offsets Summary

### BuildingTypeClass (param_1 type = `int`, direct byte offsets)
| Offset | Field | Type |
|--------|-------|------|
| 0x16A9 | UnitRepair | bool |
| 0x16AA | UnitReload | bool |
| 0x16AB | Bunker | bool |
| 0x16AC | Cloning | bool |
| 0x16AD | Grinding | bool |
| 0x16AE | UnitAbsorb | bool |
| 0x16AF | InfantryAbsorb | bool |
| 0x16B0 | SecretLab | bool |
| 0x16B3 | DockUnload | bool |
| 0x16B9 | ConstructionYard | bool |
| 0x16BA | NukeSilo | bool |
| 0x16BB | Refinery | bool |
| 0x16BC | Weeder | bool |
| 0x16BD | WeaponsFactory | bool |
| 0x16C1 | Hospital | bool |
| 0x16C2 | Armory | bool |
| 0x16C3 | EMPulseCannon | bool |
| 0xEA0 | FreeUnit | UnitType* |
| 0x1780 | NumberOfDocks | int |
| 0x1788 | DockingOffset data | int* (3 ints per pad) |

### UnitTypeClass (param_1 type = `int`, direct byte offsets)
| Offset | Field | Type |
|--------|-------|------|
| 0xE0E | Harvester | bool |
| 0xE0F | Weeder | bool |
| 0x398 | harvester/weeder auxiliary field | int; default 15, written to 10 for `Harvester=yes`/`Weeder=yes`; not `ROT=` |

### RulesClass (from g_RulesClass_Instance)
| Offset | Field | Type | Default |
|--------|-------|------|---------|
| 0x1520 | HarvesterLoadRate | int | 2 |
| 0x1528 | HarvesterDumpRate | double | 0.016 min/bail |

### TechnoTypeClass
| Offset | Field | Type |
|--------|-------|------|
| 0x71C | ROT/facing rate | parsed from `ROT=`; not overwritten by `Harvester=yes` |
| 0x6B8 (param[0x1AE]) | UnloadingClass | VoxelAnimType index |
| 0x800 (param[0x200]) | Storage | int (max credits capacity) |

### HouseClass
| Offset | Field |
|--------|-------|
| 0x30C | Credits (current money) |
| 0x538C | Base ore value per type (array) |
| 0x54E8 | Displayed credits (visual counter) |

### BuildingClass (instance, param_1 type = `int*`)
| Offset | Field |
|--------|-------|
| 0x2E4 (this[0xB9]) | Docked unit pointer |
| 0x620 | Accumulator (production/repair progress) — used by UnitRepair/Hospital/Armory |
| 0x624 | Tick flag (dumping this tick) |
| 0x628-0x630 | CDTimer (start, interval, step) |
| 0x634 | Step size |
| 0x638 | Increment per tick |

### UnitClass (instance, ESI-relative in dump function)
| Offset | Field |
|--------|-------|
| 0xBC | MissionSubState (0=init, 1=approach, 3=dumping, 4=finish) |
| 0xF8 | StepTimer Steps (auto-increment counter, compared to DumpRate threshold) |
| 0x100 | CDTimer StartFrame (for StepTimer, fires every frame during dump) |
| 0x108 | CDTimer TimeLeft (=1 during dump → fires every frame) |
| 0x10C | CDTimer Rate (=1 during dump) |
| 0x1D0 | Docked building pointer (set by EnterTransport) |
| 0x33C | StorageClass (ore cargo, indexed by ore type) |

---

## 7. Ghidra Functions Labeled

| Address | Name | Purpose |
|---------|------|---------|
| 0x004593A0 | BuildingClass__UndockUnit | Ejects docked unit with facing 0x47 |
| 0x00457CE0 | BuildingClass__CanDock | Validates if unit can dock at building |
| 0x00445F80 | BuildingClass__OnConstructionComplete | Handles post-construction setup, FreeUnit spawn |
| 0x004DA2A0 | FootClass__Is_Mission_Harvest | Returns true if mission == 7 |
| 0x004DFCB0 | FootClass__Find_Nearest_Dock | Finds closest valid dock building |
| 0x004D9290 | UnitClass__Mission_Harvest | Per-tick harvest mission handler (general) |
| 0x0073E5E0 | UnitClass__Mission_Harvest | Harvest state machine (find→gather→return→dock) |
| 0x0073D630 | UnitClass__Mission_Deploy_Building | **Refinery dump + MCV deploy** (3966 B) |
| 0x0073D450 | UnitClass__Harvest_Ore_Tick | Extract one bale from cell |
| 0x004DDF90 | UnitClass__Mission_Unload | Transport unload mission (APCs) |
| 0x00739EC0 | UnitClass__PerCellProcess | Per-cell-crossing handler — historically misnamed `Mission_Enter` in older Ghidra exports; this is the vtable slot +0x18C per-cell hook, not the Mission code-7 handler. Body contains dock-arrival detection (origin of the mislabel). Verified via `get_function_by_address 0x00739EC0`. |
| 0x0070FD70 | BuildingClass__EnterTransport | Links unit to building on dock arrival |
| 0x004F9950 | HouseClass__GiveMoney | Adds credits to house (0x30C) |
| 0x004F9790 | HouseClass__SpendMoney | Deducts credits from house |
| 0x004F9610 | HouseClass__DepositOreCredits | Normal harvester credit deposit + display update |
| 0x004F9700 | HouseClass__DepositWeedCredits | Weeder bulk credit deposit |
| 0x006C9680 | StorageClass__GetAmount | Reads ore amount by type index |
| 0x006C96B0 | StorageClass__Remove | Subtracts ore from storage |
| 0x006C9820 | StorageClass__FindFirstNonEmpty | Finds first occupied ore slot |
| 0x0065A970 | RadioClass__Transmit_Radio_Impl | Radio cmd dispatch (2=DOCK, 3=CLEAR) |
| 0x0044B780 | BuildingClass__MissionRepairAndProduce | Handles UnitRepair/Hospital/Armory/Bunker/CY (NOT Refinery) |
