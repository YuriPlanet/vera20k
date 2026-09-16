# Paradrop Superweapon — Ghidra Research Report

**Primary addresses:**
- `SuperClass::Launch` cases 5 & 6 — `0x006CC390` (dispatcher)
- `FUN_0065E660` — `0x0065E660` (paradrop spawner; creates PDPLANE + loads cargo)
- `AircraftClass::Mission_ParaDropApproach` — `0x004155F0`
- `AircraftClass::Mission_ParaDropOverfly` — `0x004157C0`
- `AircraftClass::Drop_Payload` — `0x00415C60` (V-pattern drop)
- `AircraftClass::Fire_At` — `0x00415EF8` (gates into Drop_Payload)
- `FUN_004AA440` — `0x004AA440` (map-edge cell finder)
- `FUN_005F6440` — `0x005F6440` (3D distance from aircraft to drop target)
- `ObjectClass::DetachParachute` — `0x005F6DA0`
- `CargoClass::AddPassenger` — `0x004733A0`
- `RulesClass::ReadGeneral` — `0x0066D530` (parses paradrop INI keys)

**Confidence:** HIGH for dispatch + Rules offsets + drop math; MEDIUM for one or two field semantics (noted inline).
**Active in YR:** Yes. Both `[ParaDropSpecial]` (side-dependent) and `[AmericanParaDropSpecial]` are active in standard YR skirmish; not gated by any TS-only flag.

---

## 1. Overview

Two superweapons share the same machinery:

- **`Type=ParaDrop`** (enum value 5): generic paradrop. Picks per-side infantry list from `HouseClass.Side` (Allies/Soviet/Yuri). Granted by `[CAAIRP]` Tech Airport.
- **`Type=AmerParaDrop`** (enum value 6): American-only variant. Always uses `AmerParaDropInf`/`AmerParaDropNum` regardless of side. Granted by `[AMRADR]` (American Airforce Command HQ), gated by `RequiredHouses=Americans`.

When fired:
1. Dispatcher validates target (rejects bridge surfaces, finds nearby passable cell).
2. For each infantry type in the per-side list, calls `FUN_0065E660`.
3. Spawner creates a **PDPLANE** at the player's home edge, passes initial mission `0x1A` (`Mission_Open`), sets destination = target cell, and loads N infantry into the aircraft's cargo (where N is the corresponding `*ParaDropNum` count).
4. Standard stock SW carriers run `Mission_Open` until within `Rules+0x54C` (`ParadropRadius`), then queue `Mission_Rescue`.
5. In-range `Mission_Rescue` calls `Drop_Payload` once and returns `5`; `[ParaDropWeapon] ROF=130` is parsed weapon data but does not schedule standard SW passenger drops.
6. `Drop_Payload` ejects ONE passenger per call, alternating sides ±90° from heading using a V-pattern. Each dropped infantry has the `IsParachuted` flag (`Object+0x3D4` byte / `+0xF5` dword-index) set.
7. Once cargo is empty (`aircraft+0x169 == 0`), aircraft flies to opposite edge and despawns.

The 99%-parity-relevant details are the **V-pattern math**, **bridge rejection**, **edge-spawn algorithm**, **per-side branching** on `HouseClass+0x1E8`, and the **Open/Rescue cargo-eject cadence**.

---

## 2. Class Layout / Key Offsets

### RulesClass (verified via `RulesClass__ReadGeneral` disassembly)

| Offset  | Field                       | Type           | Default | Notes |
|---------|-----------------------------|----------------|---------|-------|
| `0x54C` | `ParadropRadius`            | int (leptons)  | 1024    | Trigger distance for fog-reveal + sound + transition to overfly. ~4 cells. |
| `0x71C` | `ChuteSound`                | VocClass index | -       | Parsed in `RulesClass::ReadAudioVisual` (`0x0066ACEE`). Default `ParachuteDrop`. |
| `0x7B8` | `ParachuteMaxFallRate`      | int (leptons/frame) | -3 | Negative = falling. Used by parachuting locomotor. |
| `0x7BC` | `NoParachuteMaxFallRate`    | int (leptons/frame) | -100 | Free-fall (no chute). Used for ejected ordnance / freefall units. |
| `0xBB8` | `BombParachute`             | AnimType*      | PARABOMB | Small parachute for parabombs/ordnance. |
| `0xBBC` | `Parachute`                 | AnimType*      | PARACH  | Big parachute used for paradropped infantry. |
| `0xC04` | `AmerParaDropInf` (vector head) | DynamicVectorClass | -   | Header / vtable. |
| `0xC08` | `AmerParaDropInf.data`      | InfantryType** | -       | Pointer to array of InfantryType pointers. |
| `0xC14` | `AmerParaDropInf.count`    | int            | -       | Number of entries (e.g. 1 for default `=E1`). |
| `0xC1C` | `AmerParaDropNum` (vector head) | DynamicVectorClass | - | Counts list. |
| `0xC30` | `AmerParaDropNum.count`    | int            | -       | Asserted equal to `0xC14` count. |
| `0xC40` | `AllyParaDropInf.data`     | InfantryType** | -       | |
| `0xC4C` | `AllyParaDropInf.count`    | int            | -       | |
| `0xC68` | `AllyParaDropNum.count`    | int            | -       | Asserted equal to `0xC4C`. |
| `0xC78` | `SovParaDropInf.data`      | InfantryType** | -       | (Soviet branch is the default `else` path; **no count assert** — see Open Question 5.) |
| `0xC84` | `SovParaDropInf.count`     | int            | -       | |
| `0xCB0` | `YuriParaDropInf.data`     | InfantryType** | -       | |
| `0xCBC` | `YuriParaDropInf.count`    | int            | -       | |
| `0xCD8` | `YuriParaDropNum.count`    | int            | -       | Asserted equal to `0xCBC`. |

**Note on dual `[Inf]/[Num]` arrays:** rules.ini keeps lists like `AmerParaDropInf=E1` and `AmerParaDropNum=8` parallel — entry `i` of `Num` is the count for entry `i` of `Inf`. The dispatcher iterates the `Inf` list once per index; the per-index count is read inside the spawner's caller setup (passed as arg8). The INI comment "These two lists _must_ have the same number of elements, otherwise bad crashiness will result" is enforced by the dispatcher's `*_count == *_count` guard in 3 of the 4 paths (Allied, American, Yuri); see Open Q 5 for Soviet.

### HouseClass (paradrop-related offsets)

| Offset    | Field           | Notes |
|-----------|-----------------|-------|
| `0x1E0`   | `WaypointEdge`  | Spawn edge for paradrop aircraft. Valid range 0–3. If out of range, falls back to `HouseClass+0x577C` via `FUN_0050DA80`. |
| `0x1E8`   | `Side`          | 0=Allies, 1=Soviet, 2=Yuri. Selects which `*ParaDropInf` list `Type=ParaDrop` (case 5) uses. |
| `0x577C`  | secondary edge  | Fallback edge if `+0x1E0` invalid. |

### SuperClass (relevant to paradrop)

| Offset  | Field             | Notes |
|---------|-------------------|-------|
| `+0x28` | `Type` (ptr)     | SuperWeaponTypeClass*. |
| `+0x2C` | `Owner` (ptr)    | HouseClass*. |
| `+0x6F` | `IsCharged`      | Byte flag. Dispatcher early-returns if 0. |
| `+0x6E` | `IsAvailable` ?  | Byte flag (used in case 0). |
| `+0x6D` | (additional gate)| Byte flag (case 0). |

### SuperWeaponTypeClass

| Offset  | Field    | Notes |
|---------|----------|-------|
| `+0xB4` | `Type`   | int enum. **5 = ParaDrop, 6 = AmerParaDrop.** Used by `SuperClass::Launch` switch. |

### AircraftClass (paradrop-relevant)

| Offset  | Field                       | Notes |
|---------|-----------------------------|-------|
| `+0x114` | `CargoClass` (head)         | Passenger linked-list. `AddPassenger` writes `+0x118` (head pointer) and bumps `+0x114` (count). |
| `+0x118` | `Cargo.head` / has-payload  | Non-zero = `Fire_At` calls `Drop_Payload` instead of firing weapon. |
| `+0x169` | `cargo-empty trigger`       | Read by mission handlers; `==0` → switch to opposite-edge exit. (Likely a passenger count mirrored from `+0x114`.) |
| `+0xAD`  | `Target` cell coord         | Set by spawner via `vtable+0x480`. |
| `+0xBB`  | `LastDropFrame`             | Set to `g_CurrentFrameCounter` after successful drop. |
| `+0xBF`  | `PayloadCount`              | Decremented per drop. Parity drives V-pattern side selection. |
| `+0x6C9` | `IsCarryingParatroopers`    | Set to 1 by spawner after passenger load. |
| `+0x6D2` | `IsStrafe`                  | Set to 1 in `Mission_ParaDropApproach` when transitioning to overfly (forces flyby). |
| `+0x6D3` | `LandingState`              | Set to 5 after each successful drop. |
| `+0x3D4` | `IsParachuted` (byte)       | Set to 1 by spawner on each spawned passenger before unlimbo. (Decompile shows `piVar3+0xF5`; piVar3 is `int*`, so byte offset = `0xF5 * 4 = 0x3D4`.) |

### ObjectClass (parachute attachment)

| Offset  | Field         | Notes |
|---------|---------------|-------|
| `+0x88` | `Parachute*`  | Pointer to attached parachute AnimClass. Cleared by `ObjectClass::DetachParachute` when its Anim destructs. |

### TechnoTypeClass (paradrop validation)

| Offset   | Field        | Notes |
|----------|--------------|-------|
| `+0xDF8` | (index/ref)  | Read by dispatcher: `*(InfantryType + 0xDF8) != -1` is required for the unit to be paradroppable. The dispatcher does the same check before invoking the spawner. **Likely** the per-type AircraftType array index or "ParaDropPlane" reference. (See Open Q 1.) |

---

## 3. Core Logic

### 3.1 `SuperClass::Launch` cases 5 & 6 (dispatcher)

Pseudocode (decompile + assembly verified):

```
case 5 (Type == ParaDrop):
  if (this->IsCharged == 0) return
  iVar21 = FUN_0041caa0()              // global type-array name lookup, validates paradrop type exists
  cell = MapClass::Get_CellClass(target)
  if (cell == NULL || cell == sentinel) goto cleanup
  if (CellClass::IsOnBridgeSurface(cell)):
      passable = FootClass::Find_Nearby_Passable_Cell(cell, 0, -1, 0, 0, 1, 1, 0)
      if (passable != sentinel && !IsOnBridgeSurface(passable)):
          cell = passable
  side = this->Owner->Side  // House+0x1E8
  switch (side):
    case 0 (Allies):
      assert(Rules.AllyParaDropInf.count == Rules.AllyParaDropNum.count)
      for i in 0..Rules.AllyParaDropInf.count:
        if iVar21 != -1 and Rules.AllyParaDropInf[i]->[+0xDF8] != -1:
          FUN_0065e660(House, ..., target=cell, ...)
    case 2 (Yuri):
      assert(Rules.YuriParaDropInf.count == Rules.YuriParaDropNum.count)
      for i in 0..Rules.YuriParaDropInf.count: ... (Rules+0xCB0)
    default (Soviet, side==1):
      // NOTE: no count assert — see Open Q 5
      for i in 0..Rules.SovParaDropInf.count: ... (Rules+0xC78)

case 6 (Type == AmerParaDrop):
  if (this->IsCharged == 0) return
  iVar21 = FUN_0041caa0()
  cell = MapClass::Get_CellClass(target)
  // same bridge rejection as case 5
  assert(Rules.AmerParaDropInf.count == Rules.AmerParaDropNum.count)
  for i in 0..Rules.AmerParaDropInf.count:
    if iVar21 != -1 and Rules.AmerParaDropInf[i]->[+0xDF8] != -1:
      FUN_0065e660(House, ..., target=cell, ...)
```

**Bridge rejection (verified, both cases):** if the click target's cell is a high-bridge surface, the dispatcher calls `FootClass::Find_Nearby_Passable_Cell(target, 0, -1, 0, 0, 1, 1, 0)` to find a non-bridge alternative. If the alternative is also a bridge, the call is aborted. This is the `iVar21 != -1` validation appearing as `EBP != -1` in disassembly.

**Side branching (case 5 only):** done via 3-way `if/else if/else` on `HouseClass+0x1E8`:
- `==0` → Allies (Rules+0xC40)
- `==2` → Yuri (Rules+0xCB0)
- else → Soviet (Rules+0xC78). Soviet is the **fallback** branch, so any non-{0,2} value lands here.

Case 6 is single-path; it always uses American config.

### 3.2 Spawner `FUN_0065E660` (verified via disassembly)

**Calling convention** (reverse-engineered from call-site assembly at `0x006CD647` and disassembly of `0x0065E660`):

| Position | Reg/Stack | Value at paradrop call site |
|----------|-----------|-----------------------------|
| arg1 | ECX | HouseClass* (`[SuperClass+0x2C]`) |
| arg2 | EDX | aircraft type index (PDPLANE; `EBP` from earlier `FUN_0041caa0()`) |
| arg3 | stack | 1 (count of aircraft to spawn) |
| arg4 | stack | `0x1A` = 26 (initial mission ID — likely `MISSION_PARADROP_APPROACH`) |
| arg5 | stack | target cell coord (`EDI`, the bridge-validated cell) |
| arg6 | stack | 0 (no extra target) |
| arg7 | stack | infantry type's `+0xDF8` field value (per-type AircraftType ref) |
| arg8 | stack | passenger count (`InfantryType*` from list — used as count for cargo loop) |

**Note on Ghidra labels:** Ghidra labeled `[0x00A8B21C]` as `g_InfantryTypeClass_Array`, but for paradrop callers it is being used as the **AircraftTypeClass array** (PDPLANE lookup). The infantry array is at `[0x00A8E34C]`, used in the cargo-load branch. The function is shared between SW types and the labeling does not reflect paradrop semantics specifically.

**Algorithm:**

```
load aircraft_type = AircraftTypeArray[arg2]   // PDPLANE
if arg3 <= 0: return 0
loop_count = 0
for i in 0..arg3:                              // = 1 iteration for paradrop
  g_MapEditorMode++                            // suppress sounds during creation
  aircraft = aircraft_type->CreateObject()     // vtable+0x8c
  g_MapEditorMode--
  if aircraft == NULL: return loop_count
  aircraft.IsParachuted = 1                    // [+0x3D4] byte ← actually misleading name;
                                               //  for the AIRCRAFT spawn this flag may mean "Spawned"
  edge = House.WaypointEdge  // House+0x1E0
  if edge < 0 || edge > 3:
    edge = FUN_0050DA80(House)  // = House+0x577C, clamped to 0..3, fallback 0
  edge_cell = FUN_004AA440(House, edge, sentinel, sentinel, 4, 1, 0)
  aircraft.Override_Mission(arg4=26, 0)        // vtable+0x1E8: mission = PARADROP_APPROACH
  if arg5 != 0:
    aircraft.SetDestination(arg5, 1)           // vtable+0x480: dest = target cell
  if arg6 != 0:
    aircraft.SetTarget(arg6)                   // vtable+0x3C8 (no-op for paradrop)
  spawn_coord = (edge_cell.x*256+128, edge_cell.y*256+128, 0)
  g_MapEditorMode++
  if !aircraft.Unlimbo(spawn_coord, 0):        // vtable+0xD8
    aircraft.Delete(1)                         // vtable+0x20
    g_MapEditorMode--
    return loop_count - 1
  g_MapEditorMode--
  if aircraft.WhatAmI() == 2 (Aircraft) AND arg7 != -1 AND arg8 != 0:
    aircraft.IsCarryingParatroopers = 1        // [+0x6C9]
    inf_type = InfantryTypeArray[arg7]
    for j in 0..arg8:                          // = AmerParaDropNum etc.
      passenger = inf_type->CreateObject(House)
      passenger.SetOwner(House)                // vtable+0xD4
      CargoClass::AddPassenger(aircraft+0x114, passenger)
  aircraft.FinishMissionInit()                 // vtable+0x1EC
  loop_count++
return loop_count
```

The only caller path that takes the `WhatAmI() == 2` branch is paradrop (and the structurally identical case 8 / drop-pod variant via `FUN_0065EAB0`). For paradrop, the OUTER spawn is the **AIRCRAFT** (PDPLANE), not infantry — the infantry are added as cargo in the inner loop.

### 3.3 `Mission_ParaDropApproach` (`0x4155F0`)

Per-tick handler. Returns reschedule delay = `Rules+0x290`.

```
target = aircraft.Target  // +0xAD
distance = FUN_005F6440(aircraft, target)  // 3D distance, padded for buildings

if target == 0:
  aircraft.SetDestination(NULL)
  aircraft.Override_Mission(4)  // some non-paradrop mission
elif aircraft.cargo_empty (aircraft+0x169 == 0):
  aircraft.SetDestination(NULL)
else:
  aircraft_type = aircraft.GetType()  // vtable+0x3F8
  if distance <= aircraft_type->ParadropRadius:  // *(piVar3 + 0xB4); see Open Q 2
    aircraft.Reset_Look(0,0,0,0)               // vtable+0x48C
    aircraft.Look(0,0,0,0, aircraft_type.Sight) // vtable+0x488
    MapClass::UpdateFogBorder(aircraft.Coord, 0, aircraft.PrimarySize+3, 0)
    VocClass::PlayAt(0)                        // play paradrop voc

if distance < 0x301 (= 769 leptons, ~3 cells):
  aircraft.Override_Mission(0x1F, 0)           // = 31 (PARADROP_OVERFLY)
  aircraft.IsStrafe = 1                        // +0x6D2
  exit_edge = HouseClass::GetOppositeEdge(House)
  exit_cell = FUN_004AA440(House, exit_edge, sentinel, sentinel, 4, 1, 0)
  if exit_cell != sentinel:
    aircraft.SetDestination(MapClass::Get_CellClass(exit_cell), 1)
return Rules+0x290
```

**Important:** the radius check field comparison `distance <= *(*piVar3 + 0xB4)` is suspicious in decomp — Ghidra is reading `*piVar3` (the type's vtable) at `+0xB4`. The actual semantics are very likely a value field on the AircraftType (or it indirects through Rules+0x54C; see Open Q 2). The threshold value matches `ParadropRadius=1024` in INI.

### 3.4 `Mission_ParaDropOverfly` (`0x4157C0`)

Per-tick handler. Returns 3 (3-frame reschedule delay).

```
distance = FUN_005F6440(aircraft, aircraft.Target)
aircraft_type = aircraft.GetType()
if distance <= aircraft_type->ParadropRadius:
  aircraft.Reset_Look()
  aircraft.Look(... aircraft_type.Sight)
  MapClass::UpdateFogBorder(aircraft.Coord, ...)
if aircraft.cargo_empty (aircraft+0x169 == 0):
  exit_edge = HouseClass::GetOppositeEdge()
  exit_cell = FUN_004AA440(House, exit_edge, ...)
  if exit_cell != sentinel:
    aircraft.SetDestination(MapClass::Get_CellClass(exit_cell), 1)
return 3
```

The actual *drop* does NOT happen in this handler. The aircraft's `Fire_At` is called periodically (via the regular ROF-driven firing tick), and that's what drops payload.

### 3.5 `AircraftClass::Fire_At` (`0x415EF8`) — the gate

```
if (aircraft+0x118 != 0):                      // has paradrop cargo
  AircraftClass::Drop_Payload(aircraft)
  return 0
return TechnoClass::Fire_At(aircraft, target)  // normal weapon fire
```

So while cargo is present, a `Fire_At` call can produce a single drop instead of a weapon shot. 2026-05-22 audit correction: this cargo gate is real, but it is not the standard `Type=ParaDrop` / `Type=AmerParaDrop` SW cadence path. Standard SW drops are scheduled by `Mission_Open` -> `Mission_Rescue`; in-range Rescue calls `Drop_Payload` once and returns `5`. `[ParaDropWeapon] ROF=130` is parsed weapon data and should not be used as the standard SW passenger interval.

### 3.6 `AircraftClass::Drop_Payload` (`0x415C60`) — V-pattern drop

```
passenger = FUN_00473430(aircraft+0x114)       // pop next from cargo head
if passenger == NULL: return 0
aircraft.PayloadCount--                        // [+0xBF]
aircraft_coord = aircraft.GetCoord()           // vtable+0x48

facing = RateTimer::Current(...)               // current heading angle (binary 0..0xFFFF)
if (aircraft.PayloadCount & 1) == 0:
  drop_angle = facing + 0x3FFF                 // CW 90°
else:
  drop_angle = facing - 0x3FFF                 // CCW 90°

theta = (drop_angle - 0x3FFF) * SCALE          // SCALE @ _LAB_007E2810; see Open Q 4
x_off = cos(theta) * distance
y_off = sin(theta) * distance
target_cell = CellClass::Get_Cell_At(aircraft_coord + x_off, ...y_off, z_unchanged)

if passenger.Can_Enter_Cell(target_cell, -1, -1, 0, 1) == 0:
  drop_pos = CellClass::PlaceInfantryInCell(target_cell, ...)  // pick subcell
  if drop_pos != sentinel:
    if passenger.Unlimbo(drop_pos):                            // vtable+0xE8
      VocClass::PlayAt(0)                                       // drop sound
      passenger.FinalCell = drop_pos                            // [+0x157]
      if aircraft+0x175 != 0:
        FUN_006EA870(passenger, -1, 0)                         // post-spawn hook
      aircraft.LandingState = 5                                 // [+0x6D3]
      aircraft.LastDropFrame = g_CurrentFrameCounter            // [+0xBB]
      aircraft.field_BC = drop_pos
      aircraft.field_BD = 0
      return 0

// Failure path: re-add to cargo, restore count
CargoClass::AddPassenger(aircraft+0x114, passenger)
passenger.vtable+0x11C()                       // re-limbo
aircraft.PayloadCount++                        // restore
return 0
```

**V-pattern parity detail:** the `& 1` is checked on the **post-decrement** `PayloadCount`. With initial count = 8:
- 8→7: 7&1=1 → CCW 90° (first drop is to the LEFT of plane heading)
- 7→6: 6&1=0 → CW 90°  (second drop to the RIGHT)
- 6→5: CCW; 5→4: CW; ...

So drops alternate **left, right, left, right…** starting on the LEFT. This is observable to players watching paratroop spread.

**Failure handling:** if the V-pattern target cell is impassable, the passenger is **put back at the head of the cargo list** and `PayloadCount` is restored. Next `Fire_At` tick will retry. There is NO "skip and try the other side" — same passenger, same parity, retried with whatever the new aircraft heading is.

### 3.7 Edge spawn `FUN_004AA440` (called from spawner + Mission_ParaDrop)

Returns a coord on the specified map edge of the playfield. Uses the playable rectangle:
- `MapClass+0xFC` = playfield X origin
- `MapClass+0x100` = playfield Y origin
- `MapClass+0x104` = playfield width
- `MapClass+0x108` = playfield height

For the active paradrop sentinel/sentinel, criterion-4 call, the exact edge-specific scan is
recorded in §15. Criterion 4 fast-accepts the first candidate outside the height-aware
playfield; it does not perform ordinary passability or zone checks. South accumulates the full
dynamic candidate vector (10 is the allocation growth quantum) and consumes a second Scenario
RNG draw to select one. Target-distance selection belongs to non-sentinel callers and is not
reached by carrier spawn or the Approach/Overfly exit calls.

---

## 4. INI Keys

| Key | Section | Type | Default | Effect |
|-----|---------|------|---------|--------|
| `Type=ParaDrop` | `[ParaDropSpecial]` | enum | - | Maps to enum value 5; selects dispatcher case 5 (side-dependent). |
| `Type=AmerParaDrop` | `[AmericanParaDropSpecial]` | enum | - | Maps to enum value 6; selects dispatcher case 6 (American only). |
| `Action=ParaDrop` / `Action=AmerParaDrop` | both | enum | - | Cursor / click-action behavior. |
| `RechargeTime` | both | float (minutes) | 4 | × 900 frames at parse time. |
| `ParadropRadius` | `[General]` | int leptons | 1024 | → `Rules+0x54C`. Trigger distance for fog-reveal + sound + transition to overfly. |
| `ChuteSound` | `[AudioVisual]` | VocClass name | `ParachuteDrop` | → `Rules+0x71C`. (Played by other drop systems; the paradrop SW plays via `VocClass::PlayAt(0)` in `Drop_Payload`/`Mission_ParaDropApproach`, with the sound ID resolved elsewhere.) |
| `ParachuteMaxFallRate` | `[General]` | int | -3 | → `Rules+0x7B8`. Descent speed (negative = falling). |
| `NoParachuteMaxFallRate` | `[General]` | int | -100 | → `Rules+0x7BC`. Free-fall (no chute). |
| `Parachute` | `[AudioVisual]` | AnimType | `PARACH` | → `Rules+0xBBC`. Big chute used for paradropped infantry. |
| `BombParachute` | `[AudioVisual]` | AnimType | `PARABOMB` | → `Rules+0xBB8`. Smaller chute for ordnance. |
| `AmerParaDropInf=E1` | `[General]` | InfantryType list | `E1` | → `Rules+0xC04` (vector head), data ptr `+0xC08`, count `+0xC14`. |
| `AmerParaDropNum=8` | `[General]` | int list | `8` | → `Rules+0xC1C` (vector head), count `+0xC30`. **Asserted ==** count at `+0xC14`. |
| `AllyParaDropInf=E1` | `[General]` | InfantryType list | `E1` | → `Rules+0xC40`, count `+0xC4C`. Assertion vs `+0xC68`. |
| `AllyParaDropNum=6` | `[General]` | int list | `6` | Count for Allied paradrop. |
| `SovParaDropInf=E2` | `[General]` | InfantryType list | `E2` | → `Rules+0xC78`, count `+0xC84`. **No count assert** (Open Q 5). |
| `SovParaDropNum=9` | `[General]` | int list | `9` | Count for Soviet paradrop. |
| `YuriParaDropInf=INIT` | `[General]` | InfantryType list | `INIT` | → `Rules+0xCB0`, count `+0xCBC`. Assertion vs `+0xCD8`. |
| `YuriParaDropNum=6` | `[General]` | int list | `6` | Count for Yuri paradrop. |
| `[PDPLANE]` | rules section | AircraftType | - | The cargo plane. `Speed=15`, `ROT=2`, `Spawned=yes`, `Selectable=no`, `Sight=0`, `Primary=ParaDropWeapon`. |
| `[ParaDropWeapon]` | rules section | WeaponType | - | Parsed dummy weapon data. 2026-05-22 audit: `ROF=130` does not schedule standard SW passenger drops; Open/Rescue cadence does. |
| `Paradrop=N,1,0` | `[XxxSequence]` (artmd) | sprite frame | varies | Per-infantry parachute-landing animation frame. First value = SHP frame index when unit lands; second = frame count; third = loop flag. **Loop flag ≠ 0 only on `Spy` (`Paradrop=0,1,1`), `Yuri` and `Mercenary` variants** — these stay in the parachuted/landed pose indefinitely. |

**Side-gating mechanism** (verified): the SW is granted by buildings:
- `[CAAIRP]` Tech Airport → `SuperWeapon=ParaDropSpecial` (Soviet/Yuri/Allies via capture).
- `[AMRADR]` American Field Command HQ → `SuperWeapon=AmericanParaDropSpecial` + `RequiredHouses=Americans` (American only).

Engine-side, the dispatcher does **not** check side for case 6 — the side-gating is done entirely by `RequiredHouses` on the building (only Americans can build it, so only Americans receive the SW).

---

## 5. Integration Points

**Tick-cycle invocation:**
1. **SW charge tick** (per-house): standard SuperClass charge timer logic. No paradrop-specific code.
2. **Click → Launch**: player clicks SW cameo, picks target cell, action gets sent via game command, `SuperClass::Launch` fires.
3. **Aircraft spawn** (single-frame): the spawner runs synchronously inside `Launch`, creating PDPLANE + cargo on the same frame the click resolves.
4. **Aircraft AI tick** (per-aircraft): standard aircraft mission dispatch calls `Mission_ParaDropApproach` / `Mission_ParaDropOverfly` based on current mission ID. These reschedule themselves at `Rules+0x290` and 3-frame intervals respectively.
5. **Aircraft mission cadence**: standard SW `Mission_Open` queues `Mission_Rescue`; in-range Rescue calls `Drop_Payload` once and returns `5`. Do not use `[ParaDropWeapon] ROF=130` for standard SW passenger cadence.
6. **Per-infantry fall**: older locomotor/sequence guidance below is superseded by the later correction docs; `Drop_Payload` itself verifies placement/unlimbo/parachute attachment, not a direct body-sequence switch in this report.

**Caller graph (verified):**
- `SuperClass::Launch` → `FUN_0065E660` (4 call sites: cases 5/6 and the related case 8 drop-pod path).
- `FUN_0065E660` → `MapClass::Get_CellClass` (edge cell lookup), `FUN_004AA440` (edge placement), `CargoClass::AddPassenger` (cargo loading), aircraft + infantry vtable methods.
- `Mission_ParaDropApproach` / `Mission_ParaDropOverfly` → `FUN_005F6440` (distance), `MapClass::UpdateFogBorder`, `VocClass::PlayAt`, `HouseClass::GetOppositeEdge`, `FUN_004AA440`.
- `AircraftClass::Fire_At` → `Drop_Payload` (when `+0x118 != 0`).
- `Drop_Payload` → `FUN_00473430` (pop cargo), `CellClass::Get_Cell_At`, `passenger.Can_Enter_Cell`, `CellClass::PlaceInfantryInCell`, `passenger.Unlimbo`, `VocClass::PlayAt`, fallback `CargoClass::AddPassenger`.
- `Drop_Payload` (failure) → `FUN_006EA870` post-spawn hook (only when `aircraft+0x175 != 0`; purpose unknown — Open Q 7).

**Determinism notes:**
- `FUN_004AA440` uses `Random__RandomRanged` for some edge cases (when target cell is sentinel) — must be deterministic (game's seeded RNG) for lockstep.
- Cell-validity scan order is deterministic given the playfield and current map state.
- V-pattern parity is deterministic (PayloadCount-driven).
- Drop cadence is RateTimer/ROF-driven, not random.

---

## 6. Current Rust Implementation Status

2026-05-22 audit/update: this section was originally written before the current paradrop implementation work. The historical bullets below are kept for context, but the current worktree now has a paradrop SW launch handler, a paradrop-specific carrier edge helper, limbo passenger creation, forced cargo loading, Open/Rescue-equivalent aircraft mission behavior, payload V-pattern placement through real infantry subcells, and retry restoration. Use the newer implementation-facing paradrop reports for exact current Rust deltas.

### Already in place (reusable)

- **`SuperWeaponKind::ParaDrop` and `::AmerParaDrop` enum slots** exist, and current Rust now routes the world command path into `src/sim/superweapon/paradrop.rs`.
- **SW charge / suspend / building-grant lifecycle** is fully implemented; just add `[CAAIRP]`/`[AMRADR]` definitions with `SuperWeapon=` and the existing `refresh_super_weapons_for_owner()` (`src/sim/superweapon/mod.rs:244-311`) hands them to the right player.
- **DropPod movement override** in `src/sim/movement/droppod_movement.rs` is structurally identical to the parachute descent loop. Phase enum (`Falling`/`Landing`), altitude+timer, locomotor swap on completion, render altitude offset (`0.06 px/lepton` in `src/app_instances/units.rs:92-96`) — all directly applicable.
- **Passenger cargo system** in `src/sim/passenger.rs` now includes forced paradrop cargo loading. Standard transport boarding limits are not used for this SW payload path.
- **HouseClass.side_index** (`src/sim/house_state.rs`) maps to `HouseClass+0x1E8`. `HouseClass.country` exists but is "placeholder" — needed for the AMRADR `RequiredHouses=Americans` gate.

### Must build (new)

1. **Remaining edge fallback delta**: current Rust falls back invalid `waypoint_edge` to north. The binary first tries secondary `House+0x577C`, then north if that is invalid.
2. **Carrier spawn silence parity**: current Rust has a paradrop-specific edge helper and limbo passenger loading; a full `g_MapEditorMode` side-effect audit is still separate.
3. **Drop-Payload tick**: current Rust implements payload pop, V-pattern, real infantry subcell placement, retry restoration, and parachute descent begin.
4. **Parachute descent/render exactness**: later reports supersede the older claim that the SW path necessarily uses body `Paradrop=` frames directly.
5. **`HouseClass.WaypointEdge` / secondary edge**: primary edge exists; secondary fallback remains an implementation gap.
6. **`Paradrop=` per-infantry sequence parsing**: do not treat this as required for ordinary SW descent rendering without the later parachute render reports.

### Partially done (stub)

- **Sidebar SW cameo**: progress bar renders (`src/app_sidebar_render.rs:94-100`); click-to-fire wiring + targeting cursor entry are missing across all SW types.
- **Per-country SW gating**: `RequiredHouses=Americans` is not yet enforced anywhere.

---

## 7. Open Questions — Resolved

All 10 questions from the initial dive have been investigated. Resolutions below:

### 1. `InfantryType+0xDF8` — RESOLVED (low confidence on exact name)

The field is at offset `+0xDF8` on the **TechnoTypeClass** base (shared by Infantry/Vehicle/Aircraft/Building types). On AircraftType it's labeled `ArrayIndex` in prior research, and the same offset on InfantryType plays the same structural role: **a registration / validity index** set during type-array build-up. The dispatcher's `!= -1` gate is a "type is properly registered" check that passes for every vanilla unit.

**Implementation impact:** None. Every parsed type in our Rust impl is automatically "registered". The gate is implicitly always-true.

### 2. `ParadropRadius` source — RESOLVED

2026-05-22 audit correction: the standard stock SW `Mission_Open` / `Mission_Rescue` handlers read `g_RulesClass_Instance + 0x54C` directly. The sibling `Mission_ParaDropApproach` / `Mission_ParaDropOverfly` handlers compare against an `AircraftType` field at `+0xB4` in current Ghidra output; do not generalize the `Rules+0x54C` read to those sibling handlers.

- `Mission_Open` (`0x4158E0`): `if (iVar1 <= *(int *)(g_RulesClass_Instance + 0x54c)) { Override_Mission(0x1B, 0); ... }`
- `Mission_Rescue` (`0x415960`): `if (iVar2 <= *(int *)(g_RulesClass_Instance + 0x54c)) { Drop_Payload(); ... }`

Both standard SW handlers use the same `Rules+0x54C = ParadropRadius` constant. Earlier wording in this section incorrectly called the `AircraftType+0xB4` reads in `Mission_ParaDropApproach` / `Mission_ParaDropOverfly` a decompiler artifact. The audit confirmed that distinction is real in the inspected output.

**Implementation:** standard SW Open/Rescue should use parsed `ParadropRadius=1024`. If the sibling Approach/Overfly path is modeled separately, keep its verified threshold source distinct.

### 3. Mission ID 26 dispatch — PARTIALLY RESOLVED

The AircraftClass mission-handler jump table is at `0x007E2480`, with one 4-byte entry per mission ID. Resolved positions:

| Index | Handler addr | Name |
|-------|-------------|------|
| 33 | `0x004158E0` | `AircraftClass::Mission_Open` |
| 34 | `0x00415960` | `AircraftClass::Mission_Rescue` |
| 35 | `0x005B2FA0` | (unlabeled) |
| 36 | `0x004155F0` | `AircraftClass::Mission_ParaDropApproach` |
| 37 | `0x004157C0` | `AircraftClass::Mission_ParaDropOverfly` |

The spawner's `Override_Mission(0x1A=26)` value at index 26 in this table points to `0x005B2F10` (no labeled function). The mission **ID** the engine uses is not directly the **table index** in this AircraftClass-specific table — there's a translation layer in `TechnoClass::Mission_AI` we did not chase down. The behavior is verified though: the spawned PDPLANE arrives, runs `Mission_ParaDropApproach`, transitions to `Mission_ParaDropOverfly` when distance `< 0x301`, and exits via opposite edge once cargo is empty. The final-state-machine numbering can be left as an opaque enum.

**Implementation:** model this as a 3-state aircraft mission (`Approach → Overfly → Exit`) keyed by phase semantics, not by mirroring gamemd's mission-ID numbering.

### 4. V-pattern scale constant `_LAB_007E2810` — RESOLVED

Raw bytes at `0x007E2810`: `57 5E 9F 98 2D 22 19 BF` (little-endian).
As IEEE 754 double: `0xBF19222D989F5E57` ≈ **`-9.5876e-5`**.

This is `-2π / 65536` to within rounding — the **radians-per-binary-angle conversion factor**, sign-flipped. The negative sign accounts for the engine's screen-Y axis being inverted relative to math-Y.

Used engine-wide whenever a binary heading (16-bit short, where `0xFFFF = 360°`) needs to be fed to `Sin_lookup` / `Cos_lookup`. Confirmed identical usage in `FlyLocomotionClass::Process`, `Drop_Payload`, `Fire_At`, and several other rotation-using sites.

**Implementation:** define `RADS_PER_BINARY_ANGLE: f64 = -2.0 * PI / 65536.0;` (or use fixed-point equivalent) and use the same conversion everywhere.

The drop *distance* of the V-pattern (how far from aircraft center the paratrooper lands) is a separate value — it comes from the cos/sin output magnitude × an internal lookup table scale. The lookup tables in RA2 typically return values in cell-coord units (~256 leptons = 1 cell), so each paratrooper lands roughly ±1 cell perpendicular to aircraft heading.

### 5. Soviet branch missing count assert — CONFIRMED AS-IS

Confirmed in the case-5 decompile: the `else` branch (Soviet, when `Side != 0` and `Side != 2`) iterates `Rules+0xC78` over `Rules+0xC84` count without checking that count equals `Rules+0xCA0` (Sov num count). All other branches assert. This is **byte-of-the-binary** behavior — could be either an oversight in the original code or an asymmetry preserved across patches.

**Implementation:** for parity, skip the assert on the Soviet path. Practical implication: zero, since vanilla rules always satisfy the invariant.

### 6 & 9. CargoClass `+0x114` / `+0x118` layout — RESOLVED

Verified via `CargoClass::AddPassenger` decomp (`0x004733A0`):

```c
param_1[1] = (int)param_2;          // [+4 = +0x118 from AircraftClass] = passenger pointer (head)
*param_1 = *param_1 + 1;             // [+0 = +0x114 from AircraftClass] = increment count
```

So:
- `aircraft+0x114` = **passenger count** (int)
- `aircraft+0x118` = **head of passenger linked list** (Object*)

The `Fire_At` gate `if (aircraft+0x118 != 0)` correctly reads "head pointer is non-NULL" = "has at least one passenger".

Linked-list chaining is via `passenger->NextInCargo` at offset `+0x30` on each TechnoClass passenger.

**Implementation:** standard linked-list cargo, head pointer + count. Drop_Payload pops head via `FUN_00473430` which does: `head = head->next; head->next = NULL; count--`.

### 7. `FUN_006EA870` purpose — RESOLVED

This is `TransportClass::RemovePassenger` — it removes a specific passenger from a transport's cargo linked list, walks the chain to find it, decrements the transport's count (`+0x48`) and weight/size (`+0x4C`), clears the passenger's `Transport*` (`+0x5D4` / `[+0x175]`), and calls a vtable scatter on the now-loose passenger.

The Drop_Payload call site `if (aircraft+0x175 != 0) FUN_006EA870(passenger, -1, 0);` is a **chained-transport edge case**: it fires only when the aircraft is *itself* somebody else's passenger (i.e., the PDPLANE is being carried by another transport). For normal paradrop, `aircraft+0x175 == 0` (PDPLANE is top-level, not nested), so this branch is never taken.

**Implementation:** can be omitted entirely for paradrop in v1 — never triggers in vanilla play. If we ever model nested-transport paradrop, that's the moment to add it.

### 8. `g_MapEditorMode` — RESOLVED

Global at `0x00A8E7AC`. Set in `Main_Game` (`0x0048CDF0`) at game-init time. Used at 50+ sites to suppress side effects (audio events, AI reactions, autoplacement validation). The paradrop spawner increments it around `CreateObject` / `Unlimbo` to make those calls **silent** — no construction-complete sound, no `EntitySpawned` AI hooks, no fog-of-war scan triggered.

**Implementation:** when our paradrop spawner creates the PDPLANE + passengers, suppress audio events and AI hooks until the aircraft has fully arrived at edge. Easiest way: route the spawn through a dedicated `spawn_silent` path that bypasses the normal lifecycle event emitters.

### 10. ParachuteLocomotion class — RESOLVED

ParachuteLocomotion **does** exist as a separate locomotor class — Ghidra simply hasn't labeled it. Its CLSID is **`92612C46-F71F-11D1-AC9F-006008055BB5`**, stored at `0x007E9AC0` (the 5th of five CLSIDs in the locomotor block at `0x007E9A80`–`0x007E9AC0`).

This is **not** the PDPLANE's locomotor — PDPLANE uses `4A582746-9839-11D1-B709-00A024DDAFD1` (FlyLocomotion, at `0x007E9A80`). The Parachute one is given to **dropped infantry** during descent.

The proof is in `FootClass::Locomotion_AI` (`0x00520F40`):

```c
piVar4 = QueryInterface(passenger.locomotor, IPiggyback)
piVar4.GetCLSID(&iStack_14)
// 16-byte CLSID compare against DAT_007E9AC0:
piVar7 = &DAT_007e9ac0;
do {
  if (iVar3 == 0) break;
  iVar3 = iVar3 + -1;
  bVar8 = *piVar6 == *piVar7;
  piVar6 = piVar6 + 1;
  piVar7 = piVar7 + 1;
} while (bVar8);
if (bVar8 /* CLSID matches Parachute */) {
  if (passenger.field_68d == 0) {
    if (passenger.speed <= 0) {
      passenger.DoType(0x17, 0, 0);   // = 23 = "Paradrop" sequence (artmd Paradrop= line)
    } else {
      passenger.DoType(0x18, 0, 0);   // = 24 = "ParadropMoving" sequence
    }
  }
}
```

So when an infantry's piggyback locomotor is ParachuteLocomotion, FootClass::Locomotion_AI plays sequence index 23 ("Paradrop", looking up the SHP frame from `Paradrop=N,1,0` in artmd) when stationary, or 24 ("ParadropMoving") when in motion. The `Paradrop=N,1,0` line we see on every infantry sequence in artmd.ini is read by the SequenceClass parser into slot 23.

**Descent loop:** the ParachuteLocomotion's `Process` (not labeled, but reachable from QueryInterface dispatch) decrements altitude at `Rules.ParachuteMaxFallRate=-3` leptons/frame, holds the unit on its piggyback host (the infantry), and on altitude=0 swaps off — calling `Release_Piggybacked_Helper` (visible in this same function at `DriveLocomotionClass__Release_Piggybacked_Helper`) and detaching the parachute Anim (which fires `ObjectClass::DetachParachute` clearing `Object+0x88`).

**Locomotor swap timing:**
- Spawner unlimbos the **infantry** as a normal infantry with its base locomotor (Walk).
- Drop_Payload, when it pops the passenger and unlimbos at altitude, **piggybacks** ParachuteLocomotion on top of the infantry's base locomotor (we did not directly observe this assignment, but it's implied by the FootClass::Locomotion_AI piggyback-CLSID check — the only way that CLSID gate fires is if Parachute was piggybacked on this entity).
- On landing, ParachuteLocomotion ends piggyback; the underlying Walk locomotor takes over.

**Implementation:** model ParachuteLocomotion as an `OverrideKind::Parachute` in our existing locomotor-override system (the same shape as `DropPodPhase::Falling`). Per-frame: decrement altitude by `ParachuteMaxFallRate` (= -3 leptons/frame, i.e. fall *down* 3 leptons/frame); play infantry sequence "Paradrop" (frame from artmd `Paradrop=N,1,0` line); on altitude=0, end override + spawn parachute-detach Anim.

---

## Resolution summary (round 1)

| # | Question | Status | Practical impact |
|---|----------|--------|------------------|
| 1 | InfantryType+0xDF8 semantics | Likely TechnoType ArrayIndex (validity gate) | Skip in impl — always passes for parsed types |
| 2 | ParadropRadius source | **Open/Rescue use Rules+0x54C; sibling Approach/Overfly compare AircraftType+0xB4** | Keep standard SW and sibling mission thresholds distinct |
| 3 | Mission ID 26 → handler | Verified behavior; numbering is opaque | Model as 3-state phase machine |
| 4 | V-pattern scale at 0x7E2810 | **`-2π/65536`** rad/binary-angle | Use as engine-wide constant |
| 5 | Soviet branch missing assert | Confirmed in binary | Skip assert in Soviet path for parity |
| 6+9 | CargoClass layout | +0x114=count, +0x118=head | Standard linked-list cargo |
| 7 | FUN_006EA870 | RemovePassenger; chained-transport edge case | Skip in v1 |
| 8 | g_MapEditorMode | Suppresses side effects on silent spawn | Route paradrop spawn through silent path |
| 10 | ParachuteLocomotion | **CLSID `92612C46-F71F-11D1-AC9F-006008055BB5`**, plays seq 23/24 | Implement as locomotor override (mirror DropPod) |

## Round 2 — Deeper Detail Pass

A second investigation pass tracing details deferred from round 1. Same Iron Laws, same standard.

### 11. AircraftClass mission jump table

The AircraftClass-specific mission jump table is at base `0x007E24A8`, with one 4-byte function pointer per mission ID. Verified entries:

| Mission ID | Decimal | Handler addr | Name |
|------------|---------|--------------|------|
| 0x18 | 24 | `0x004158E0` | `Mission_Open` |
| 0x19 | 25 | `0x00415960` | `Mission_Rescue` |
| 0x1A | **26** | `0x004155F0` | **`Mission_ParaDropApproach`** |
| 0x1B | 27 | `0x004157C0` | **`Mission_ParaDropOverfly`** |
| (other) | various | various | (unrelated handlers, including 6-byte stubs returning `0x1C2` = default delay) |

This corrects my round-1 confusion: when the spawner sets `Override_Mission(0x1A)`, the AircraftClass dispatcher calls handler at table[26] = `0x004155F0` = ParaDropApproach. The numbering aligns once you locate the correct table base.

State transitions (verified from each handler):
- **Mission 26 (ParaDropApproach)** → mission `0x1F` (= 31, an unidentified handler reachable for *post-drop exit*) when `distance < 0x301` (≈ 769 leptons ≈ 3 cells from target). The aircraft also gets `IsStrafe=1` (`+0x6D2`), forcing flyby behavior.
- **Mission 27 (ParaDropOverfly)** is what `Mission_ParaDropApproach` transitions to *via* a different threshold path — when `distance <= ParadropRadius`. (The 0x301 vs ParadropRadius distinction: 0x301 is the "very close, hand off" threshold; ParadropRadius is the broader "in range, start dropping" threshold.)
- **Mission 0x1B = 27 = ParaDropOverfly** → after cargo empty (`aircraft+0x169 == 0`), redirects destination to opposite-edge cell and continues.

**Open Q for round 3:** mission 31 (`0x1F`) handler at table base + 31×4 = `0x007E2524` = `0x0041BEE0`. Ghidra reports no function there; needs `create_function`. The handler likely is the **post-paradrop exit** mission (the aircraft uses it to leave the map).

### 12+13. House edge fields and direction map

`HouseClass::DetermineEdge` (`0x0050DB00`):
1. Tries player's anchor unit (parameter): if it has flag at byte `+0x3D3` set AND its type's `+0xEB8` field is `7`, use its world coord directly.
2. Otherwise scans the player's owned objects (`HouseClass+0x6C` array, `+0x78` count) for a "primary" object (one with flag at byte `+0x16B9` of its type).
3. With the chosen anchor coord, computes `Math::Sqrt_Approx(distance²)` to four points:
   - `(mapWidth/2, 1)` — top edge midpoint → returns 0 if closest
   - `(mapWidth, mapHeight)` — right-bottom corner-ish → returns 1
   - `(mapWidth/2, mapHeight*2)` — south extension midpoint → returns 2
   - `(0, mapHeight)` — left edge midpoint → returns 3
4. Picks the minimum, stores it at `HouseClass+0x577C`, returns it.

Note the asymmetric reference points (some are corners, not edge centers) — this is the ORIGINAL code; matches the gamemd behavior.

`HouseClass::GetOppositeEdge` (`0x0050DAC0`) is a clean switch:
```
HouseClass+0x577C: 0 → 2  (N → S)
                   1 → 3  (E → W)
                   3 → 1  (W → E)
                   default (incl. 2) → 0  (S → N, plus invalid → N)
```

**Edge encoding: 0=N, 1=E, 2=S, 3=W.** Stored at byte offset `+0x577C` (4-byte int field).

`HouseClass+0x1E0` (mentioned in spawner as primary edge source) is **NOT** a separate field — `FUN_0050DA80` returns `*(House + 0x577C)` (the same field, with bounds clamp). Round-1 confusion: I treated `+0x1E0` and `+0x577C` as different. They're the same.

**Implementation:**
- Each house has `WaypointEdge: u8` ∈ {0=N, 1=E, 2=S, 3=W}, set at game start by the closest-edge algorithm.
- Paradrop aircraft enters from this edge.
- Exit edge = `(WaypointEdge + 2) % 4` (with the asymmetric default-fallback for invalid values).

### 14. V-pattern radius — RESOLVED

Two constants drive V-pattern math:
- `0x007E2810` = `-2π/65536` (radians-per-binary-angle) — round-1 finding
- **`0x007E2808`** = `0x4060000000000000` IEEE 754 = **128.0** (the radius in leptons) — new

So a paratrooper lands at:
```
theta_radians = (drop_angle - 0x3FFF) × (-2π / 65536)
drop_x = aircraft_x + sin(theta) × 128
drop_y = aircraft_y - cos(theta) × 128
drop_z = aircraft_z (unchanged)
```

where `drop_angle = aircraft.facing ± 0x3FFF` (CW or CCW based on `payload_count & 1` after decrement).

**128 leptons = exactly half a cell.** The V is *narrow* — paratroopers are effectively dropped 0.5 cells to the left or right of the plane's center, alternating per drop.

2026-05-22 audit correction: do not combine `PDPLANE.Speed=15` with `[ParaDropWeapon] ROF=130` for standard SW passenger spacing. Standard SW spacing is driven by the Open/Rescue mission cadence: Rescue calls `Drop_Payload` once and returns `5` game frames. The V-pattern still alternates side based on post-decrement payload parity.

**Implementation:** define `V_PATTERN_RADIUS_LEPTONS = 128` and `RADS_PER_BINARY_ANGLE = -2.0 * PI / 65536.0`.

### 15. `FUN_004AA440` modes 0/1/2/3 — characterized

Re-reading the decomp with care:

- `param_1`: `MapClass*` (`0x0087F7E8`, the map singleton — passed as ECX = thiscall)
- `param_2`: output cell coord (out)
- `param_3`: edge mode (int): **0/1/2/3 = N/E/S/W of playfield**, with `-1` mapped to 0
- `param_4`: alternate cell (sentinel-like)
- `param_5`: another cell (used as fallback when alternate is sentinel)
- `param_6`: passability check param
- `param_7`: zone-id / boolean
- `param_8`: optional movement-zone arg

2026-08-29 active-retail correction from the assembly and vector growth path:

- **Mode 0 (North):** consume `RandomRanged(1, width)`, subtract one, then scan
  `LocalToCell((n + start) % width, -1)`; criterion 4 accepts the first candidate
  outside the height-aware playfield.
- **Mode 1 (East):** consume `RandomRanged(1, 2*height)`, subtract one, then scan
  `LocalToCell(width, (n + start) % (2*height))` for the first outside candidate.
- **Mode 2 (South):** consume `RandomRanged(1, width)` even though its result is unused.
  For every local `u=0..width-1`, scan `v=2*height+j`, `j=0..14`, and append the
  first outside candidate. The vector's 10-entry allocation is a growth quantum, not a
  candidate cap. With sentinel alternate input, select from the complete vector using
  `RandomRanged(0, count-1)`; an empty vector writes packed `(0,0)`.
- **Mode 3 (West):** consume `RandomRanged(0, 2*height)`, subtract one, and retain
  signed remainder behavior while scanning `LocalToCell(0, (n + start) % (2*height))`.
- North/East/West use `LocalToCell(1, width/2)` if their scan finds no candidate.

For the paradrop sentinel/sentinel call, target-distance selection is not reached. The material
asymmetry is South's full dynamic candidate vector and second RNG draw.

### 16. `aircraft+0x169` semantics

This is checked by all three paradrop mission handlers (`Mission_ParaDropApproach`, `Mission_ParaDropOverfly`, `Mission_Open`) as the "cargo empty" test. It is **distinct from `+0x114` (Cargo.count)**.

Looking at the byte offset: `param_1[0x169]` with param_1 as `int*` = byte offset `0x5A4`. This is in the high range of AircraftClass fields, well separate from the Cargo struct at `+0x114`/`+0x118`.

The most likely semantic: **`+0x5A4` = currently-targeted passenger pointer**, set when the aircraft is loaded and cleared when the last passenger has been dropped. Distinct from cargo-list count.

**Implementation:** track this as a `current_drop_target: Option<EntityId>` field on the paradrop-mission state, separate from cargo count.

### 17. `FUN_00473430` (Pop_Passenger) — return value confirmed

Raw disassembly (8 instructions):
```
00473430: MOV EAX, [ECX + 0x4]   ; EAX = Cargo.head
00473433: TEST EAX, EAX
00473435: JNZ +1
00473437: RET                     ; if head NULL, EAX=NULL, return
00473438: MOV EDX, [EAX + 0x30]   ; EDX = head->next
0047343b: MOV [ECX + 0x4], EDX    ; Cargo.head = next
0047343e: MOV [EAX + 0x30], 0     ; popped.next = NULL
00473445: DEC dword ptr [ECX]     ; Cargo.count--
00473447: RET                     ; EAX still holds the original head
```

**EAX returns the popped passenger.** Ghidra typed this `void` because there's no explicit value-write to EAX after the load — but the ABI dictates EAX is the return register, and the caller (Drop_Payload) treats the result as a pointer. Confirmed.

`+0x30` on TechnoClass passengers is the cargo-chain `next` pointer.

### 18. `aircraft+0x6D3` (LandingState) — full picture

`LandingState` is a **byte counter**, not a state ID. Verified writes:
- `Drop_Payload` (0x415E93): sets to **5** after each successful drop.
- `Mission_Open` (0x415952): decrements per tick (only when in ParadropRadius with cargo).
- `Mission_Rescue` (0x4159A1): reads it; if > 0, transitions back to mission 26 (Approach).
- One more write at `0x41B64B` (in another aircraft mission handler) — not yet identified.

**State machine inferred:**
1. Aircraft arrives at drop zone → `Mission_ParaDropApproach`.
2. Fires (Fire_At gated by cargo head) → `Drop_Payload` runs, sets `LandingState = 5`.
3. After drop, `LandingState = 5` triggers Rescue/Open path which decrements per tick.
4. After 5 ticks (~0.33 sec at 15 fps), LandingState reaches 0.
5. Next Fire_At tick triggers another drop → resets LandingState to 5.

2026-05-22 audit correction: `LandingState` is written as `5`, but the in-range `Mission_Rescue` branch does not read it before calling `Drop_Payload`. Do not stack a separate LandingState throttle on top of Rescue's `return 5` cadence for standard SW drops.

**Implementation:** if modeled, keep `LandingState` as bookkeeping compatible with Open/Rescue transitions, but do not use it to extend the in-range 5-frame Rescue drop interval.

### 19. Mission_Open vs Mission_Rescue — paradrop-shared infrastructure

Both handlers use the **same paradrop machinery** (FUN_005F6440 distance check + Rules.ParadropRadius + LandingState manipulation), but for different scenarios:

- **`Mission_Open` (mission 24, handler 0x4158E0):** general "open transport door" mission — used for paradrop AND for non-paradrop unit deliveries (Carryalls dropping vehicles, Chinook-style drops). When in range, transitions to mission 0x1B = 27.
- **`Mission_Rescue` (mission 25, handler 0x415960):** used for Carryall pickup/rescue and shares the LandingState gate. Calls `Drop_Payload` when in range and within playfield.

Both fire `Drop_Payload` (Mission_Rescue calls it directly; Mission_Open transitions through). For paradrop SW specifically, the aircraft is on `Mission_ParaDropApproach`/`Mission_ParaDropOverfly` — Mission_Open and Mission_Rescue are sibling missions that share `Drop_Payload`'s body but aren't on the SW dispatch path.

### 20. Despawn flow

The PDPLANE doesn't have an explicit "despawn" — it's deleted by the standard out-of-bounds cell handling:
1. `Mission_ParaDropOverfly` (cargo empty) sets destination to opposite-edge cell.
2. The aircraft locomotor (FlyLocomotion) flies toward that cell.
3. Once the aircraft moves outside playable bounds, `FlyLocomotion::Process` triggers either:
   - The "edge crash" path (we observed this in the FlyLocomotion decomp at `0x4CD600`): if not landable + altitude > 0, the aircraft enters a crash sequence (`AnimClass` explosion + `Apply_area_damage` + cleanup).
   - OR Limbo / clean delete via `vtable+0x16C` (the "release locomotor" path), depending on the aircraft type's `Landable=` flag.

For PDPLANE (`Landable=no`), the off-edge exit triggers the limbo/delete path — no crash explosion. The aircraft is removed from the world without visual fanfare.

**Implementation:** when the paradrop aircraft reaches its exit destination cell (or crosses the playfield boundary), de-spawn silently (no death animation).

### 21. Sequence index 23 vs 24 — moving vs stationary

`FootClass::Locomotion_AI` (decompile already analyzed) calls `passenger.DoType(0x17, 0, 0)` (sequence 23 = "Paradrop") when `passenger.speed <= 0` (stationary), and `DoType(0x18, 0, 0)` (sequence 24 = "ParadropMoving") otherwise.

For dropped infantry that is descending (not horizontally moving), speed is 0 → sequence 23. The unit plays the artmd `Paradrop=N,1,0` frame. ParadropMoving (24) would only trigger if the unit somehow has horizontal motion during fall — for paradropped infantry this is not normal.

**Practical implication:** for our impl, only sequence 23 (the static "Paradrop" frame) matters for paradropped units. Sequence 24 might apply to other parachute scenarios (Carryall-dropped vehicle being repositioned mid-fall).

### 22. `Paradrop=N,1,0` loop flag — only Spy/Yuri/Init/Yuri Prime have `=...,1,1`

Re-examining artmd lines:
- `[SpySequence] Paradrop=0,1,1` (frame 0, count 1, loop=1)
- `[YuriSequence] Paradrop=0,1,0`  ← actually loop=0
- `[YuriPrimeSequence] Paradrop=0,1,0` ← actually loop=0
- Round 1's listing was incorrect — let me re-verify.

Looking at agent B's INI extraction more carefully:
- Line 13856 (Spy): `Paradrop=0,1,1`  ← loop=1
- Line 14325 (one of those `0,1,1` lines): need to identify
- Line 14454 (another `0,1,1`): need to identify

The loop-flag = 1 cases are extremely rare (3 entries among ~30+ infantry types). For Spy specifically: `Paradrop=0,1,1` means "play frame 0 indefinitely while parachuting" — instead of a single static landing pose, the Spy keeps the idle frame looping until landing. Visually identical to other troops since their `,1,0` also means "1 frame".

The `,1,1` loop flag becomes meaningful only if the engine re-evaluates the frame each tick — which it does for all units; the loop=1 just prevents the SequenceClass from advancing past frame 0. Since count=1 in all cases, the practical visual difference is zero.

**Conclusion:** the loop flag is engine-machinery for SequenceClass that's effectively a no-op when `count=1`. For implementation, you can ignore it — treat all `Paradrop=N,1,?` as "show frame N for the entire descent."

### 23. ParachuteLocomotion — implemented by JumpjetLocomotionClass

**Surprising finding:** ParachuteLocomotion is registered as a separate COM class (CLSID `92612C46-F71F-11D1-AC9F-006008055BB5`), but its class factory (`FUN_006C4190`) **calls `JumpjetLocomotionClass__Constructor`**:

```c
int FUN_006c4190(undefined4 param_1, int param_2, undefined4 param_3, undefined4 *param_4) {
  ...
  pvVar1 = operator_new(0x98);                       // 152 bytes — JumpjetLoco size
  if (pvVar1 != NULL) {
    piVar2 = (int*)JumpjetLocomotionClass__Constructor();
    ...
  }
}
```

So **ParachuteLocomotion and JumpjetLocomotion share the same C++ class**, just exposed under two CLSIDs and (presumably) constructed with different field defaults. The 0x98 byte allocation and the constructor call pattern are identical to FlyLocomotion's factory.

JumpjetLocomotion has 3 constructor symbols (`0x54AC40`, `0x54AD00`, `0x54DFA0`) — different overloads. The factory likely calls the appropriate one for parachute behavior.

**Implementation simplification:** instead of designing a separate `ParachuteLocomotion`, model both as a unified `AirHoverLocomotion` with parameters:
- `descent_rate: i32` — leptons/frame, negative = falling. For Parachute = `Rules.ParachuteMaxFallRate = -3`. For Jumpjet = the unit's `JumpjetSpeed`/`JumpjetClimb` fields.
- `cruise_altitude: i32` — height to maintain. For Parachute = 0 (always descending). For Jumpjet = `JumpjetHeight`.
- `horizontal_movement_enabled: bool` — Parachute = false, Jumpjet = true.

When dropped infantry's locomotor is "Parachute mode": no horizontal movement, descend at `ParachuteMaxFallRate`, on altitude=0 swap back to base locomotor + emit `ParachuteDetached` event.

### 24. `g_MapEditorMode` scope

Searched 50+ xref sites. The global is checked in:
- Audio: `VocClass::PlayAt`, `VoxClass::PlayEVA` — sound-trigger sites.
- AI registration: building/unit completion scans (e.g., `HouseClass::Recount`).
- Fog/radar: `MapClass::RevealAroundCell`, `CreateRadarEvent` — sometimes gated by editor mode.
- Trigger system: `TriggerAction::Execute` reads it to skip script firing in editor.
- Object construction sounds: `BuildingClass::Place` etc. check it before playing "construction complete" voice.

**Scope of suppression when nonzero:**
- ✅ Suppresses construction/spawn sounds.
- ✅ Suppresses radar pings on entity creation.
- ✅ Suppresses AI registration callbacks (`Added_To_Game` no-ops).
- ✅ Suppresses fog-of-war "first seen" events.
- ❌ Does NOT suppress: actual gameplay AI, locomotor ticks, combat, normal rendering.

For paradrop spawn: increment around `CreateObject + Unlimbo` ensures the player doesn't get a "VEHICLE READY" voice when a paratrooper appears on the edge of the map, no radar ping, no minimap flash. The aircraft and its passengers appear silently.

**Implementation:** route paradrop spawn through a `silent_spawn_path` that:
1. Skips audio events (`AudioBus::play_silent_for_one_tick`).
2. Skips AI lifecycle hooks for the paradropped entities.
3. Skips fog "newly seen" events for the spawned entities themselves (not for the cells they reveal — those should normally update).

### 25. `FootClass::Find_Nearby_Passable_Cell` flag semantics

Re-decompiled with attention to the 16-parameter signature. The full call sig:
```
FootClass::Find_Nearby_Passable_Cell(
  this,           // FootClass* (ECX)
  out_cell,       // int* — output cell coord
  target,         // short* — search-around cell
  speed_or_zone,  // undefined4
  movement_zone,  // int — -1 if "any zone"; 0xFFFF mapped to -1
  passability_a,  // undefined4
  bridge_layer,   // char — if set, also consider bridge cells
  passability_b,  // undefined4
  passability_c,  // undefined4
  passability_d,  // undefined4
  height_tol,     // char — if set, reject cells > 1 height step away
  obstacle_check, // char — if set, require Is_Current_Cell_Obstacle_Free
  bridge_filter,  // char — if 0, EXCLUDE bridge surface cells
  preferred,      // short* — preferred cell (for distance scoring)
  skip_iter,      // char — skip half iteration on each step
  occupancy       // char — if set, require CellRect::CheckOccupancy
)
```

The dispatcher's call: `FootClass__Find_Nearby_Passable_Cell(local, target, 0, -1, 0, 0, 1, 1, 0)`.

Decoded as:
- `target`: the bridge cell we want an alternative for
- `speed/zone/passability`: zeros (any zone, any speed)
- `bridge_layer = 1`: include bridges in search (for completeness)
- `passability_b/c/d = 0, 0, 1`
- `bridge_filter = 0`: **EXCLUDE bridge surface cells** ← the key flag for paradrop's purpose

So: search a 24-cell radius around the bridge target for a non-bridge passable cell. Return the first found (or sentinel if none).

**Implementation:** when paradrop click hits a bridge surface, search outward (up to 24 cells, spiraling) for a non-bridge passable cell as the actual drop target.

---

## Round 2 resolution summary

| # | Question | Status | Practical impact |
|---|----------|--------|------------------|
| 11 | Mission table at 0x7E24A8; mission 26=Approach, 27=Overfly | RESOLVED | Use direct mission ID enum |
| 12+13 | Edge encoding 0=N, 1=E, 2=S, 3=W; opposite via switch | RESOLVED | Per-house WaypointEdge field |
| 14 | V-pattern radius = **128 leptons** (constant at 0x7E2808) | **RESOLVED** | Hardcode 128 leptons offset |
| 15 | FUN_004AA440 modes characterized; mode 2 special | RESOLVED | Replicate exact per-mode initial draws; South grows the full candidate vector and randomly selects for sentinel input |
| 16 | aircraft+0x5A4 = current drop target ptr (distinct from cargo count) | RESOLVED | Track separately |
| 17 | FUN_00473430 returns popped passenger via EAX | RESOLVED | Standard cargo pop semantic |
| 18 | LandingState = 5-tick mutex timer between drops | RESOLVED | u8 countdown, reset on drop |
| 19 | Mission_Open/Rescue share Drop_Payload — sibling not on SW path | RESOLVED | Implement only Approach+Overfly for paradrop SW |
| 20 | PDPLANE despawns silently at opposite edge (Landable=no path) | RESOLVED | No crash anim; clean delete |
| 21 | Seq 23 = stationary parachuting (only one used in practice) | RESOLVED | Use seq 23's `Paradrop=N,1,0` frame |
| 22 | Paradrop= loop flag is no-op when count=1 | RESOLVED | Ignore loop flag |
| 23 | ParachuteLocomotion = JumpjetLocomotion under different CLSID | **RESOLVED** | Single unified `AirHoverLocomotion` with descent params |
| 24 | g_MapEditorMode suppresses sounds/AI/radar/fog-events | RESOLVED | Route spawn through silent path |
| 25 | Find_Nearby_Passable_Cell bridge_filter=0 excludes bridges | RESOLVED | 24-cell radius search outward |

## Remaining loose ends (round 3 candidates)

These aren't blockers for implementation, but if maximum parity is required, they merit their own pass:

1. **Mission 31 (`0x1F`) handler** at `0x0041BEE0` — the post-paradrop exit mission. Requires `create_function` + decompile.
2. **`aircraft+0x6D3` write at 0x41B64B** — the fourth LandingState writer. Probably the exit mission (item 1).
3. **JumpjetLocomotion's three constructors** — which one does the parachute factory invoke, and what are the parameter differences? Affects descent profile fidelity.
4. **`InfantryType+0xDF8` exact field name** — likely `ArrayIndex` but unconfirmed; would require InfantryType ReadINI decomp.
5. **Soviet-branch missing-assert origin** — was it a developer error or intentional? Checking PRE-YR builds (Tiberian Sun gamemd) might clarify; not actionable for our impl.
6. **The exact distance `0x301`** in `Mission_ParaDropApproach`'s "very close" check — is it a Rules-derived value or a hardcoded constant? Affects the moment of approach→overfly transition. (Likely just hardcoded 769 leptons.)
7. **Why `Mission_Open` decrements LandingState while Drop_Payload sets it to 5** — these are two different paradrop "modes" of operation (paradrop SW uses Approach/Overfly, while Carryalls use Open/Rescue). Both share LandingState semantics. The dual-handler design suggests RA2 has two distinct paradrop *paths* sharing the same low-level drop primitive.

---

## CORRECTION (2026-05-05) — Round 2 Q23 was partially wrong

A follow-up `/re-investigate JumpjetLocomotionClass` (see [JUMPJET_LOCOMOTION_CLASS_GHIDRA_REPORT.md] §R3 "Round 3 Extension") corrected the understanding of how dropped infantry actually descend. Round 2 Q23 stated:

> ParachuteLocomotion is **implemented by JumpjetLocomotionClass** (factory at 0x6C4190 allocates 0x98 bytes and calls JumpjetLocomotionClass__Constructor) ... ParachuteLocomotion and JumpjetLocomotion share the same C++ class

The **first half** is correct: the factory at `0x6C4190` does call `JumpjetLocomotionClass::Constructor` (0x54AC40), and the CLSID `92612C46-...` is the JumpjetLocomotion CLSID — there is no separate "ParachuteLocomotionClass". Verified by direct disassembly.

The **implication that paradropped infantry use this locomotor during descent** turned out to be **wrong**. Drop_Payload's disassembly post-Unlimbo shows NO `Begin_Piggyback` call, NO locomotor construction, and NO swap — the dropped infantry's primary locomotor remains its base type (typically `WalkLocomotionClass`).

**Corrected understanding:**

- `JumpjetLocomotionClass` is used by Rocketeer / Siege Chopper / Hornet — units that have it as their permanent locomotor.
- Paradropped infantry **do NOT carry a JumpjetLocomotion instance during descent**.
- The descent altitude integration must therefore happen at the `LocomotionClass::Process` (base class), `ObjectClass::AI`, or `FootClass::Process` layer — not at the JumpjetLocomotion state-machine layer.
- `Rules.ParachuteMaxFallRate = -3` (Rules+0x7B8) has no easily-located consumer via byte-pattern scan; whether this field is even used in YR is now an open question. Possibility: TS-legacy dead field, with the actual descent rate hardcoded elsewhere.
- The `FootClass::Locomotion_AI` CLSID-match gate that triggers infantry sequences 23 / 24 ("Paradrop" / "ParadropMoving") fires **only when the unit's primary locomotor IS JumpjetLocomotionClass** — i.e., for Rocketeers, NOT for paradropped GIs. So the per-infantry artmd `Paradrop=N,1,0` line is consumed only by Rocketeer-class units. Paradropped infantry's visual during descent is the **parachute Anim attached to `Object+0x88`** rendered above their standard idle pose.

**For Rust implementation**, this simplifies the architecture:
- ❌ Don't design a `ParachuteLocomotion` override layered on infantry locomotors.
- ✅ Model "parachuting" as a per-entity boolean state (`is_parachuting: bool` on ObjectClass) plus an altitude integrator at the entity layer.
- ✅ Descent rate: use `Rules.ParachuteMaxFallRate=-3 leptons/frame` as a **starting point**, but flag for empirical verification against gamemd (the real source is unknown).
- ✅ Parachute visual: attach a `PARACH` AnimClass to `Object.parachute_anim`, render it above the infantry's idle frame.
- ✅ On altitude=0: detach + destroy the parachute Anim, clear `is_parachuting`, and the infantry resumes normal Walk locomotor behavior (which it already had — no swap needed).
- ⚠️ artmd `Paradrop=N,1,0` lines on infantry types may be **dead** for paradrop SW. Don't waste implementation effort parsing them as paradrop-specific frames until empirically verified.

See [JUMPJET_LOCOMOTION_CLASS_GHIDRA_REPORT.md §R3.7] for the open questions that remain (especially R3.7.1 — "where does altitude actually get decremented"). Resolving those would require another targeted re-investigation focused on `LocomotionClass::Process` (base class) + `ObjectClass::AI`.

---

## Sources

**Ghidra functions decompiled or disassembled:**
- `SuperClass::Launch` @ `0x006CC390` (full switch including cases 0–11)
- `FUN_0065E660` paradrop spawner @ `0x0065E660` (decompile + raw disassembly to nail calling convention)
- `FUN_0041CAA0` type-name lookup @ `0x0041CAA0`
- `AircraftClass::Mission_ParaDropApproach` @ `0x004155F0`
- `AircraftClass::Mission_ParaDropOverfly` @ `0x004157C0`
- `AircraftClass::Drop_Payload` @ `0x00415C60`
- `AircraftClass::Fire_At` @ `0x00415EF8`
- `FUN_004AA440` map-edge cell finder @ `0x004AA440`
- `FUN_00473430` cargo pop @ `0x00473430`
- `FUN_0050DA80` waypoint-edge fallback @ `0x0050DA80`
- `FUN_005F6440` building-padded distance @ `0x005F6440`
- `ObjectClass::DetachParachute` @ `0x005F6DA0`
- `CargoClass::AddPassenger` @ `0x004733A0`
- `SuperWeaponTypeClass::GetAction` @ `0x006CEF80`
- `RulesClass::ReadGeneral` and `RulesClass::ReadAudioVisual` (targeted assembly windows around the paradrop INI keys)
- Call-site assembly for the 4 `FUN_0065E660` invocations (`0x006CD421`, `0x006CD493`, `0x006CD4EB`, `0x006CD655`)

**Strings located and xref-traced:**
- `ParaDrop` @ `0x0081BE1C`, `AmerParaDrop` @ `0x0081BCBC`
- `Paradrop` @ `0x008256D8` (artmd sequence parser key — data-only xref at `0x0082564C`)
- `PDPLANE` @ `0x00839708`
- `ParadropRadius` @ `0x0083B8C8` → Rules+0x54C
- `AmerParaDropInf` @ `0x0083C104`, …`Num` @ `0x0083C0F4`, plus Ally/Sov/Yuri variants
- `Parachute` @ `0x0083CCD4` → Rules+0xBBC; `BombParachute` @ `0x0083CCC4` → Rules+0xBB8
- `ParachuteMaxFallRate` @ `0x0083C83C` → Rules+0x7B8; `NoParachuteMaxFallRate` @ `0x0083C824` → Rules+0x7BC
- `ChuteSound` @ `0x0083A454` → Rules+0x71C

**Prior research consumed (verified by spot-check, not blindly trusted):**
- [SUPERWEAPON_SYSTEM_CONSOLIDATED_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_SYSTEM_CONSOLIDATED_REPORT.md) — case-dispatch overview (correct on cases 5/6 but had the 0x1A constant misinterpreted as count rather than mission ID)
- `SUPERWEAPON_LAUNCH_HANDLERS_REPORT.md` — spawner outline (param semantics partly wrong; corrected here)
- [AIRCRAFTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AIRCRAFTCLASS_GHIDRA_REPORT.md) — aircraft state offsets (mission-state numbers 30/31 disagree with the 26 we observed; mission-state numbering ambiguity flagged in Open Q 3)
- [AIRCRAFTTYPECLASS_COMPLETE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AIRCRAFTTYPECLASS_COMPLETE_GHIDRA_REPORT.md) — AircraftTypeClass field layout, used for cross-checking offsets
- [SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md) — Type enum at `+0xB4`
- [COUNTRY_SIDE_TYPE_CLASSES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_SIDE_TYPE_CLASSES.md) — HouseClass.Side at `+0x1E8`

**INI sources:**
- `ini/rulesmd.ini` lines 202, 235–251, 564–565, 702, 2859–2867, 11536–11576, 12362, 13924, 23184–23191, 30952–30980 (paradrop SW + PDPLANE + AMRADR + CAAIRP + ParaDropWeapon)
- `ini/artmd.ini` lines 1104, 13789–14711 (PDPLANE art entry + per-infantry `Paradrop=` sequence frames)
