# SuperClass / SuperWeaponTypeClass — Ghidra Research Report

**Primary Addresses:**
- `SuperWeaponTypeClass::Constructor` — `0x006CE5B0`
- `SuperWeaponTypeClass::ReadINI` — `0x006CEA20`
- `SuperClass::Constructor` — `0x006CAF90` (2-param), `0x006CAEC0` (0-param)
- `SuperClass::Activate` — `0x006CB560`
- `SuperClass::Suspend` — `0x006CB4D0`
- `SuperClass::Deactivate` — `0x006CB7B0`
- `SuperClass::AI_Charging` — `0x006CC080`
- `SuperClass::AI_Ready` — `0x006CBCA0`
- `SuperClass::AnimStage` — `0x006CBEE0`
- `SuperClass::NameReadiness` — `0x006CC2B0`
- `SuperClass::Launch` — `0x006CC390`
- `HouseClass::AI_ResumeProduction` — `0x0050AF10` (superweapon grant/enable loop)
- `AI::SuperLaunchCheck_SingleSW` — `0x006EFC70` (AI Iron Curtain fire logic)
- `AI::SuperLaunchCheck_DualSW` — `0x006EFE60` (AI dual-SW fire logic)

**Confidence:** HIGH (all functions decompiled from binary)
**Active in YR:** Yes — core gameplay system, all 12 types active

---

## 1. Overview

The superweapon system consists of two classes:
- **SuperWeaponTypeClass** — static type definitions loaded from INI (one per `[SWType]` section)
- **SuperClass** — runtime instances (one per SuperWeaponType per HouseClass)

Each HouseClass creates a SuperClass array at construction time. Buildings with `SuperWeapon=` or `SuperWeapon2=` keys grant/enable superweapons on their owning house when completed. Superweapons charge over time, can be suspended by low power, and dispatch type-specific effects when launched.

---

## 2. Type Enum

String table at `0x008425C0` (12 entries, 4 bytes each = pointer array ending at `0x008425F0`):

| Index | String | Superweapon | Active in YR |
|-------|--------|-------------|--------------|
| 0 | `MultiMissile` | Nuclear Missile | Yes |
| 1 | `IronCurtain` | Iron Curtain | Yes |
| 2 | `LightningStorm` | Lightning Storm | Yes |
| 3 | `ChronoSphere` | Chronosphere (1st click — select source) | Yes |
| 4 | `ChronoWarp` | Chrono Warp (2nd click — select destination) | Yes |
| 5 | `ParaDrop` | Paradrop (side-dependent) | Yes |
| 6 | `AmerParaDrop` | American Paradrop | Yes |
| 7 | `PsychicDominator` | Psychic Dominator | Yes (YR only) |
| 8 | `SpyPlane` | Spy Plane Flyby | Yes |
| 9 | `GeneticConverter` | Genetic Mutator | Yes (YR only) |
| 10 | `ForceShield` | Force Shield | Yes (YR only) |
| 11 | `PsychicReveal` | Psychic Reveal | Yes (YR only) |

The Type enum is stored at `SuperWeaponTypeClass+0xB4` and controls the switch dispatch in `Launch()`.

---

## 3. SuperWeaponTypeClass Layout

Inherits from AbstractTypeClass. Constructor at `0x006CE5B0`.
`param_1` in constructor is `undefined4 *` (pointer-indexed), so multiply by 4 for byte offsets.
`param_1` in ReadINI is `int` (direct byte offsets).

| Byte Offset | Type | Name | Default | INI Key | Notes |
|-------------|------|------|---------|---------|-------|
| 0x00–0x23 | — | AbstractTypeClass base | — | — | Includes vtable ptrs, ID |
| 0x24 | char[] | SectionName | from base | `[section]` | Used as INI section key |
| 0x9C | ptr | WeaponType | NULL | `WeaponType=` | WeaponTypeClass* for Nuke payload |
| 0xA0 | int | Unknown_A0 | -1 | — | Not read in ReadINI |
| 0xA4 | int | Unknown_A4 | -1 | — | Not read in ReadINI |
| 0xA8 | int | Unknown_A8 | -1 | — | Not read in ReadINI |
| 0xAC | int | Unknown_AC | -1 | — | Not read in ReadINI |
| 0xB0 | int | RechargeTime | 0x1194 (4500) | `RechargeTime=` | In game frames. 4500 = 5 min @ 15fps. INI value is in minutes, converted via `ftol(minutes * 900)` |
| 0xB4 | int | Type | -1 | `Type=` | Index into Type enum table |
| 0xB8 | ptr | SidebarImageSHP | NULL | `SidebarImage=` | Loaded from MIX after name parsed |
| 0xBC | int | Action | 0 | `Action=` | Cursor/targeting action enum (table at `0x7E4C50`) |
| 0xC0 | int | SpecialSound | -1 | `SpecialSound=` | VocClass index |
| 0xC4 | int | StartSound | -1 | `StartSound=` | VocClass index |
| 0xC8 | ptr | AuxBuilding | NULL | `AuxBuilding=` | BuildingTypeClass* (e.g., Nuke Silo building needed to fire) |
| 0xCC | char[25] | SidebarImageName | `""` | `SidebarImage=` | Raw string, used to load SHP |
| 0xE5 | bool | UseChargeDrain | false | `UseChargeDrain=` | Enables charge-drain cycle (charge→ready→active→recharge) |
| 0xE6 | bool | IsPowered | **true** | `IsPowered=` | Charging pauses when house has insufficient power |
| 0xE7 | bool | DisableableFromShell | false | `DisableableFromShell=` | Can be toggled off in game lobby |
| 0xE8 | int | FlashSidebarTabFrames | -1 | `FlashSidebarTabFrames=` | Sidebar tab flash duration on activation |
| 0xEC | bool | AIDefendAgainst | false | `AIDefendAgainst=` | AI will attempt to defend against this SW |
| 0xED | bool | PreClick | false | `PreClick=` | Requires first click (ChronoSphere source selection) |
| 0xEE | bool | PostClick | false | `PostClick=` | Requires second click (ChronoWarp destination) |
| 0xF0 | int | PreDependent | -1 | `PreDependent=` | Type enum index of required prerequisite SW |
| 0xF4 | bool | ShowTimer | false | `ShowTimer=` | Display charge timer in sidebar |
| 0xF5 | bool | ManualControl | false | `ManualControl=` | If true, suspend doesn't auto-resume on power restore |
| 0xF8 | float | Range | 0.0 | `Range=` | Targeting range in cells |
| 0xFC | int | LineMultiplier | 0 | `LineMultiplier=` | Line drawing multiplier for targeting cursor |

### Notes on INI keys NOT in the binary

`RechargeVoice=`, `ChargingVoice=`, `ImpatientVoice=`, `SuspendVoice=` — these strings do **NOT** exist in gamemd.exe. They are likely mod-extension features (Ares/Phobos), not original YR. The EVA voices per superweapon type appear to be hardcoded in `AI_Charging` and `AI_Ready` via switch/case per Type enum.

---

## 4. SuperClass Layout

Runtime instance, one per SuperWeaponType per HouseClass. Constructor at `0x006CAF90`.
`param_1` in constructor is `undefined4 *` (multiply index by 4 for byte offset).
`param_1` in all other functions is `int` (direct byte offsets).

| Byte Offset | Type | Name | Default | Notes |
|-------------|------|------|---------|-------|
| 0x00–0x23 | — | Base class | — | 4 vtable pointers + INoticeSink base |
| 0x24 | int | TimerOverride | -1 | Custom recharge time; -1 = use SWType default |
| 0x28 | ptr | SWType | param | SuperWeaponTypeClass* |
| 0x2C | ptr | OwnerHouse | param | HouseClass* |
| 0x30 | int | ChargeStartFrame | currentFrame | Frame when charging began; -1 = inactive |
| 0x34 | int | TimerSnapshot | 0 | CDTimerClass internal data |
| 0x38 | int | RemainingFrames | 0 | Frames until ready |
| 0x3C | ptr | UIDataPtr | from SWType+0x60 | Copied from SWType during Activate |
| 0x40 | byte | field_40 | 0 | Cleared during Activate |
| 0x48 | int | field_48 | 0 | |
| 0x4C | int | field_4C | 0 | |
| 0x50 | int | ReadyCountdown | -1 | Sound countdown timer; -1 = done |
| 0x60 | byte | IsPresent | 1 | Guard flag for Suspend; 1 = can operate |
| 0x62 | uint32 | TargetCell | InvalidCell | Packed cell coordinate (low 16 = X, high 16 = Y) |
| 0x68 | ptr | AssociatedBuilding | NULL | BuildingClass* that provides this SW |
| 0x6C | byte | field_6C | 0 | |
| 0x6D | byte | IsEnabled | 0 | Set to 1 by Activate when building grants SW |
| 0x6E | byte | IsPostClicked | 0 | Set to 1 for two-phase SWs (Nuke targeting, etc.) |
| 0x6F | byte | IsReady | 0 | Set to 1 when charging complete |
| 0x70 | byte | IsSuspended | 0 | Set to 1 when power insufficient |
| 0x74 | int | LastProgressFrame | -100 | Frame of last sidebar progress update |
| 0x78 | int | LastAnimStage | -1 | Cached sidebar pip stage for change detection |
| 0x7C | int | OneTimeState | -1 | ChargeDrain state machine: 0=Charging, 1=Ready, 2=Active |

---

## 5. Superweapon Lifecycle

### 5.1 Initialization (HouseClass Constructor)

`HouseClass::Constructor` (`0x004F54A0`) creates one SuperClass instance for each SuperWeaponTypeClass registered in the global array. These are stored in:
- `HouseClass+0x258` — pointer to SuperClass* array data
- `HouseClass+0x264` — count of superweapons

### 5.2 Grant / Enable (HouseClass::AI_ResumeProduction)

`HouseClass::AI_ResumeProduction` (`0x0050AF10`) iterates all superweapons each tick:

1. For each SuperClass not yet enabled (`+0x6D == 0`):
2. Scan the house's building list (`HouseClass+0x6C` array, `+0x78` count)
3. For each building that is alive and operational:
   - Check up to 3 upgrade slots at `BuildingClass+0x5EC`
   - Check if `BuildingTypeClass+0x16F0` (SuperWeapon) or `+0x16F4` (SuperWeapon2) matches this SW index
4. If a matching building exists AND (`DisableableFromShell==false` OR `SuperWeaponsAllowed` game option):
   - Check power ratio (PowerOutput / PowerDrain)
   - Call `SuperClass::Activate` (`0x006CB560`) with power-low flag
   - If player house: add cameo to sidebar

### 5.3 Activate (SuperClass::Activate = FUN_006CB560)

Called when a building grants this superweapon. Key logic:

```
Guard: if (IsEnabled) return false  // already active
Set IsEnabled = 1
Set IsPostClicked = param_PostClick
Clear field_40
Copy SWType->field_0x60 to UIDataPtr
```

Then:
- If **ManualControl is false** AND was previously suspended: resume charging (set ChargeStartFrame = currentFrame)
- If **ManualControl is true**: start timer from scratch with RechargeTime
- If **ShowTimer is set** and this is the player: add to timer display list
- If SW is charging (enabled, not ready, not suspended, not PreClick): call `AI_Charging`

### 5.4 Charging (SuperClass::AI_Charging)

Called each tick while enabled and not ready:

```pseudocode
if not IsEnabled: return

rechargeTime = (TimerOverride != -1) ? TimerOverride : SWType.RechargeTime
elapsed = ftol(currentFrame - ChargeStartFrame)
remaining = rechargeTime - elapsed

if remaining == 0:
    IsReady = 1

ChargeStartFrame = currentFrame  // Reset for next tick
RemainingFrames = remaining

if firstTimeReady:  // param_2
    PlayEVA per type (switch on SWType.Type)

LastProgressFrame = currentFrame

if UseChargeDrain:
    OneTimeState = 1  // Transition to "Ready"
```

### 5.5 Ready Check (SuperClass::AI_Ready)

Called each tick from `HouseClass::Update` (`0x004F8440`, ~line 451):

```pseudocode
// Ready countdown sound
if ReadyCountdown > 0: ReadyCountdown -= 1
if ReadyCountdown == 0:
    ReadyCountdown = -1
    PlayReadySound()

// Flash associated building
if cursorState != 4 AND AssociatedBuilding != NULL:
    AssociatedBuilding.IsFlashing = 1
    UpdateBuildingAnim()

// Guard checks
if not IsEnabled: return 0
if IsReady AND not UseChargeDrain: return 0  // Already ready, no drain cycle
if IsSuspended: return 0

// Timer not started
if ChargeStartFrame == -1:
    if LastAnimStage == -1: return 0
    else: LastAnimStage = -1; return STAGE_CHANGE

// Calculate remaining
elapsed = currentFrame - ChargeStartFrame
if elapsed < RemainingFrames:
    // Still charging — check for anim stage change
    remaining = RemainingFrames - elapsed
    stage = AnimStage()
    if stage == LastAnimStage: return 0
    LastAnimStage = stage
    return STAGE_CHANGED

// Fully charged!
if UseChargeDrain:
    if OneTimeState != 2:  // Not yet "Active"
        OneTimeState = 1   // "Ready"
        IsReady = 1
        return CHANGED
    else:
        OneTimeState = 0   // Reset to "Charging"
        // Restart timer with full recharge duration
        ChargeStartFrame = currentFrame
        RemainingFrames = rechargeTime
        return CHANGED
else:
    IsReady = 1
    PlayEVA per type
    LastProgressFrame = currentFrame
    return CHANGED
```

### 5.6 Suspend / Resume (SuperClass::Suspend)

`SuperClass::Suspend` (`0x006CB4D0`) pauses/resumes charging based on power:

```pseudocode
Guard: IsEnabled AND not IsPostClicked AND IsSuspended != newState AND IsPresent

if resuming (newState == false):
    if ManualControl is false:
        if ChargeStartFrame == -1:
            ChargeStartFrame = currentFrame  // Start fresh
        IsSuspended = false
        return true
    // ManualControl: save remaining time and pause
    else:
        remaining = CalculateRemaining(ChargeStartFrame, RemainingFrames)
        RemainingFrames = remaining
        ChargeStartFrame = -1  // Pause
else: // suspending
    remaining = CalculateRemaining()
    RemainingFrames = remaining
    ChargeStartFrame = -1  // Pause

IsSuspended = newState
return true
```

**Key detail:** When charging is suspended, the remaining time is **saved** (not reset). When power is restored, charging continues from where it left off.

### 5.7 Deactivate (SuperClass::Deactivate)

`SuperClass::Deactivate` (`0x006CB7B0`): Called when the granting building is destroyed/sold.

```pseudocode
if not IsEnabled: return false
IsReady = false
IsEnabled = false
Remove from active superweapon display list
return true
```

### 5.8 AnimStage (SuperClass::AnimStage)

`SuperClass::AnimStage` (`0x006CBEE0`): Returns 0–54 for sidebar charge progress display.

```pseudocode
if not IsEnabled: return 0
if UseChargeDrain == false AND IsReady: return 54  // Fully charged
stage = ftol(chargeRatio * 52)  // 0..52
if stage > 52: return 53
return stage
```

Range: 0 = empty, 53 = almost full, 54 = fully charged.

### 5.9 NameReadiness (SuperClass::NameReadiness)

`SuperClass::NameReadiness` (`0x006CC2B0`): Returns status text string.

```pseudocode
if IsSuspended: return "Offline" (CSF 0x3B6)
if UseChargeDrain == false:
    if IsReady: return "Ready" (CSF 0x3B0)
else:  // ChargeDrain cycle
    switch OneTimeState:
        0: return "Charging" (CSF 0x397)
        1: return "Ready"    (CSF 0x39A)
        2: return "Active"   (CSF 0x39D)
return NULL  // Still charging, no status text
```

---

## 6. Launch Dispatch (SuperClass::Launch)

`SuperClass::Launch` (`0x006CC390`) — the master fire handler. Switches on `SWType->Type` (+0xB4):

### Case 0: MultiMissile (Nuke)

Two-phase launch:
1. **Phase 1 (IsPostClicked == false):** Find building with matching SuperWeapon type. Set building's fire animation. Store target cell at `HouseClass+0x5784`. Set building's pending SW type. Play launch sound + EVA.
2. **Phase 2 (IsPostClicked == true, IsEnabled == true):** Get ground height at target cell (including bridge offset `DAT_00b0c07c`). Look up WeaponType from SWType+0x9C (linked to NukePayload section). Spawn projectile (BulletClass) with trajectory calculated via Sin/Cos lookup tables. Create building fire animation. Play EVA + sound. Set `HouseClass+0x1FC = 1` (nuke launched flag).

### Case 1: IronCurtain

- Get target cell center coordinates (including bridge height)
- Create `IRONBLST` animation (`Rules+0x348 = IronCurtainInvokeAnim`)
- Iterate 3×3 cell grid around target (table at `0x00B0C038`, 9 entries of `short[2]`)
- For each cell: get occupants from cell linked list (`CellClass+0xE4` ground / `+0xE8` bridge)
- For each unit: if not in air AND not already iron-curtained:
  - Apply iron curtain effect with duration from `Rules+0xFE8` (`IronCurtainDuration`)
- Play EVA + create radar event

### Case 2: LightningStorm

- Call `LightningStorm::Start` (`0x00539EB0`)
- Play EVA

**LightningStorm::Start:**
- If target cell is the invalid sentinel, pick a random valid cell
- Store target cell, owner house in globals (`DAT_00A9F9CC`, `DAT_00A9FACC`)
- Set storm active flag (`DAT_00A9FAB4 = 1`)
- Record start frame (`DAT_00827FC0`)
- For each non-allied house: play EVA warning
- Set `PlayerPtr+0x5779 = 1` (NeedsPowerRecalc)
- Call weather overlay function (`0x0053C280`)
- If `Rules+0x17B0` flag set: play ion storm sound, show text message

### Case 3: ChronoSphere (First Click)

- Store target cell at `SuperClass+0x62`
- Get cell center coordinates
- Set cursor state to 4 (prompts for second click destination)
- Call `SuperClass::SetTargetData` (`0x006CB3A0`)
- If `AssociatedBuilding` exists: set building flash flag

### Case 4: ChronoWarp (Second Click)

- Create radar events at both source and destination
- Create `ChronoBlast` animation at source (`Rules+0x32C`) and `ChronoBlastDest` at destination (`Rules+0x328`)
- Iterate 3×3 cell grid around **source cell** (from `SuperClass+0x62`):
  - For each unit in cell:
    - **Non-chronoshiftable check:** `TypeClass+0xD97 (Chronoshiftable)` — if false AND not already immune (`TypeClass+0xCD4`), kill with full HP damage (C4 warhead from `Rules+0xFA8`)
    - **Chronoshiftable units:**
      - If warping and in a building, detach the Temporal weapon
      - Disable any active docking/parasiting
      - Create TeleportLocomotion (`CLSID_TeleportLocomotion`)
      - Calculate destination cell: offset = unit position - source cell center. Destination = dest cell center + offset (preserves sub-cell position)
      - Handle bridge height at destination
      - Set `ChronoInTransit` flag (`+0x6B6 = 1`)
      - Store destination coordinates at `+0xA2/0xA3/0xA4` (0x288/0x28C/0x290)
      - Set `PendingWarpPhase` to 3 (`vtable+0x280`)
      - Store owner house index at `+0x10B` (0x42C)
      - Piggyback the existing locomotor onto the new TeleportLocomotion
- Play EVA

### Case 5: ParaDrop

- Get country/side ID from `FUN_0041CAA0`
- Get target cell (handle bridge → find nearby passable cell)
- Select infantry array based on side:
  - Side 0 (Allied): `Rules+0xC40/0xC4C/0xC68` (AllyParaDropInf/Num)
  - Side 2 (Yuri): `Rules+0xCB0/0xCBC/0xCD8` (YuriParaDropInf/Num)
  - Default (Soviet): `Rules+0xC78/0xC84` (SovParaDropInf/Num)
- For each infantry type in the array: call `FUN_0065E660` (spawn paradrop aircraft)

### Case 6: AmerParaDrop

- Same structure as ParaDrop but uses `Rules+0xC08/0xC14/0xC30` (AmerParaDropInf/Num)

### Case 7: PsychicDominator

- Call `PsychicDominator::Start` (`0x0053AE50`)
  - Creates PsychicDominator animation (`Rules+0x2FC`)
  - Stores target cell, owner house in globals
  - Sets active flag, start frame
  - Calls shared weather overlay function (`0x0053C280`)
- Create radar event + play EVA + sound

### Case 8: SpyPlane

- Gets country/side, target cell
- Uses side-dependent arrays (same as AllyParaDrop at `Rules+0xC4C`)
- Calls `FUN_0065EAB0` (distinct from paradrop — spawns spy plane aircraft for flyover)

### Case 9: GeneticConverter (Genetic Mutator)

- Get target coordinates (including bridge height)
- Create `GeneticMutator` animation (`Rules+0x298`)
- Play EVA + sound
- Create radar event
- If `Rules+0x17C8` flag (`PurifyMode`?) is false:
  - Iterate 3×3 cell grid, for each infantry unit (`RTTIType == 0xF`):
    - Kill with `Rules+0xF98` warhead (GeneticMutator damage)
- Else:
  - Apply area damage via `Apply_area_damage()`

### Case 10: ForceShield

- Get target coordinates (including bridge height)
- Create `ForceShield` animation (`Rules+0x34C`)
- Set countdown timer: `SuperClass+0x50 = Rules+0x17BC (ForceShieldDuration) - Rules+0x17C4 (?)`
- Store target coordinates at `SuperClass+0x54/0x58/0x5C`
- Play StartSound if set
- **Call `HouseClass::SpyPowerSabotage`** — causes power blackout on the activating player!
  - `Rules+0x17B8` = ForceShieldRadius
- Iterate all buildings in game:
  - If allied with the shield owner:
    - Calculate distance from target center
    - If within `ForceShieldRadius * 256` leptons:
      - Apply invulnerability (IronCurtain effect) with `ForceShieldDuration` from `Rules+0x17BC`

### Case 11: PsychicReveal

- Get target cell coordinates
- Call `FUN_005678E0` twice (reveal area) with:
  - `Rules+0xFEC` (PsychicRevealRadius)
  - Owner HouseClass
- Play sound at target

---

## 7. AI Superweapon Usage

### AI::SuperLaunchCheck_SingleSW (`0x006EFC70`)

AI logic for firing the Iron Curtain (searches for Type == 1):

1. Iterates all units belonging to the AI, finding the most threatened one (highest `TechnoClass+0x5FC` threat score)
2. Searches the AI house's superweapon array for IronCurtain type
3. If ready and power sufficient: launch at the most threatened unit
4. If not ready: calculate charge ratio. If close enough to a threshold (`DAT_007E2AC8 - Rules+0xD70`), mark as pending

### AI::SuperLaunchCheck_DualSW (`0x006EFE60`)

Similar logic for other superweapons with two-phase targeting.

---

## 8. INI Keys Summary

### SuperWeaponTypeClass Section Keys

| Key | Type | Default | Offset | Description |
|-----|------|---------|--------|-------------|
| `Type=` | string→enum | — | 0xB4 | Superweapon type (see Type Enum table) |
| `Action=` | string→enum | 0 | 0xBC | Cursor/targeting action |
| `RechargeTime=` | float (minutes) | 5.0 | 0xB0 | Stored as frames (minutes × 900) |
| `WeaponType=` | string | — | 0x9C | WeaponTypeClass for projectile-based SWs |
| `IsPowered=` | bool | **true** | 0xE6 | Charges pause when house is low-power |
| `DisableableFromShell=` | bool | false | 0xE7 | Can be toggled in game lobby |
| `ShowTimer=` | bool | false | 0xF4 | Display charge timer on sidebar |
| `SidebarImage=` | string | — | 0xCC | SHP filename for sidebar cameo |
| `Range=` | float | 0.0 | 0xF8 | Targeting range in cells |
| `LineMultiplier=` | int | 0 | 0xFC | Line drawing multiplier |
| `FlashSidebarTabFrames=` | int | -1 | 0xE8 | Sidebar tab flash frames on activation |
| `PreClick=` | bool | false | 0xED | Requires first click (source selection) |
| `PostClick=` | bool | false | 0xEE | Requires second click (destination) |
| `PreDependent=` | string→enum | -1 | 0xF0 | Type of prerequisite SW (e.g., ChronoWarp needs ChronoSphere) |
| `SpecialSound=` | string | -1 | 0xC0 | VocClass sound during effect |
| `StartSound=` | string | -1 | 0xC4 | VocClass sound on activation |
| `AIDefendAgainst=` | bool | false | 0xEC | AI will try to defend against this |
| `AuxBuilding=` | string | — | 0xC8 | Required building type to fire (e.g., Nuke Silo) |
| `UseChargeDrain=` | bool | false | 0xE5 | Charge-drain cycle instead of one-shot |
| `ManualControl=` | bool | false | 0xF5 | Suspend doesn't auto-resume on power restore |

### Key [General] Section Values

| Key | Rules Offset | Default | Used By |
|-----|-------------|---------|---------|
| `IronCurtainDuration` | +0xFE8 | 750 frames (50s) | Case 1 (IronCurtain), Case 10 (ForceShield) |
| `ForceShieldDuration` | +0x17BC | 500 frames | Case 10 (ForceShield) |
| `ForceShieldRadius` | +0x17B8 | 4 cells | Case 10 (ForceShield) |
| `ForceShieldBlackoutDuration` | +0x17C4 | — | Case 10 (timer offset) |
| `LightningStormDuration` | — | 180 frames | Case 2 (global DAT) |
| `PsychicRevealRadius` | +0xFEC | 15 cells | Case 11 (PsychicReveal) |
| `ChronoDelay` | +0xBEC | 60 frames | Case 4 (ChronoWarp) |
| `ChronoBlast` | +0x32C | anim | Case 4 (source anim) |
| `ChronoBlastDest` | +0x328 | anim | Case 4 (destination anim) |
| `IronCurtainInvokeAnim` | +0x348 | IRONBLST | Case 1 (IronCurtain) |

### Building-Side Keys

| Key | BldgType Offset | Type |
|-----|----------------|------|
| `SuperWeapon=` | 0x16F0 | int (SuperWeaponTypeClass index) |
| `SuperWeapon2=` | 0x16F4 | int (SuperWeaponTypeClass index) |

---

## 9. Integration Points

### Who Calls What

| Caller | Callee | When |
|--------|--------|------|
| `HouseClass::Constructor` | `SuperClass::Constructor` | House init — creates one per SWType |
| `HouseClass::AI_ResumeProduction` | `SuperClass::Activate` | Building grants SW |
| `HouseClass::Update` | `SuperClass::AI_Ready` | Every tick per SW |
| `SuperClass::Activate` | `SuperClass::AI_Charging` | First charge tick |
| `HouseClass::Update_Power` | `SuperClass::Suspend` | Power goes low/restores |
| Player input / AI decision | `SuperClass::Launch` | Fire the superweapon |
| Building destroyed/sold | `SuperClass::Deactivate` | SW lost |
| `TriggerAction::Execute` | `SuperClass::Activate` | Map trigger can force-activate |

### Tick Order Context

Superweapon ticking (`AI_Ready`) happens during `HouseClass::Update`, which runs after movement/combat systems and before defeat detection.

### 3×3 Cell Grid

Cases 1, 4, 9 iterate a 3×3 cell grid defined as a `short[2][9]` table at `0x00B0C038`. This is the standard adjacency pattern:
```
(-1,-1) (0,-1) (1,-1)
(-1, 0) (0, 0) (1, 0)
(-1, 1) (0, 1) (1, 1)
```
The table ends at `0x00B0C05C` (9 entries × 4 bytes = 36 bytes).

---

## 10. Current Rust Implementation Status

**Implemented:**
- `GameOptions::super_weapons` flag (on/off toggle)
- Cursor enum variants for all superweapon types
- Cursor atlas sprite definitions
- Chrono miner teleportation (separate from Chronosphere SW)
- Power system (would support IsPowered suspend logic)
- `SimSoundEvent::ChronoTeleport` sound event

**NOT Implemented:**
- SuperWeaponTypeClass INI parsing
- SuperClass runtime state (charging, timers, ready flag)
- Superweapon grant/enable from buildings
- All 12 Launch dispatch handlers
- Sidebar cameo integration for superweapons
- AI auto-fire logic
- Power suspend/resume for charging
- ForceShield power blackout mechanic
- Lightning Storm weather overlay system
- PsychicDominator mind control area effect
- Genetic Mutator infantry conversion
- Paradrop aircraft spawning (SW-triggered)

---

## 11. Open Questions (RESOLVED)

All major open questions from the initial investigation have been resolved by deep-dive subagents.
See companion reports for full details. Remaining minor unknowns noted inline below.

### Resolved: Offsets 0xA0–0xAC in SuperWeaponTypeClass
**Vestigial/unused EVA voice fields.** Never read by any code path. EVA voices are hardcoded
per-Type in switch statements inside `AI_Charging` and `AI_Ready`. Included in `ComputeChecksum`
(0x006CE910) but otherwise dead. **Confidence: 85%.**
*(Source: [SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md))*

### Resolved: RechargeTime Conversion Factor
**Confirmed: 900.0f** (float constant at `0x007F4100` = `0x44610000`).
Formula: `frames = (int)(minutes_from_ini * 900.0f)`. Default 4500 = 5 min @ 15fps.
*(Source: [SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md))*

### Resolved: CDTimerClass Layout (SuperClass+0x30)
12-byte struct: `{ int StartFrame, int Reserved, int Duration }`.
`GetTimeRemaining()` returns `Duration - (CurrentFrame - StartFrame)` or 0 if expired.
*(Source: [SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md))*

### Resolved: Action Enum (73 entries at 0x7E4C50)
Key superweapon actions: None(0), Move(1), Attack(5), **Nuke(20)**, **IronCurtain(37)**,
**LightningStorm(38)**, **ChronoSphere(39)**, **ChronoWarp(40)**, **ParaDrop(41)**,
**PsychicDominator(59)**, **ForceShield(62)**, **Airstrike(64)**.
Several DontUse and TibSunBug entries are TS legacy placeholders.
*(Source: [SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md))*

### Resolved: Lightning Storm Per-Tick Logic
`LightningStorm::Process` at `0x0053A6C0`, called from `LogicClass::PerTickUpdate`.
- Center bolt every `LightningHitDelay` (10) frames at storm center
- Scatter bolt every `LightningScatterDelay` (5) frames at random cell within `LightningCellSpread/2`
- Min manhattan distance between bolts = `LightningSeparation` (3), up to 3 retries/tick
- Two-phase bolt: cloud anim spawns → at halfway frame, bolt + damage fires
- Duration check: storm ends when `currentFrame > startFrame + duration`, then waits for anims
- Rules: LightningDamage=250 (+0x1798), LightningWarhead=IonWH (+0x17B4), Duration=180 (+0x179C),
  Deferment=250 (+0x1794), CellSpread=10 (+0x17A8)
*(Source: SUPERCLASS_SYSTEM_GHIDRA_REPORT.md lightning-storm agent)*

### Resolved: PsychicDominator Area Effect
5-phase state machine in `FUN_0053AF40`. Core effect `FUN_0053B080` fires at state 2→3:
1. Area damage with `DominatorWarhead` (Rules+0x2F8, CellSpread=7)
2. Iterate cells within `DominatorCaptureRange` (Rules+0x308, default 1, max 10)
3. Filter out buildings, ImmuneToPsionics, BalloonHover, iron-curtained, in-limbo
4. **Permanent ownership transfer** via `SetOwner(house, 1)` — NOT CaptureManager MC
5. Creates `PermaControlledAnimationType` (MINDANIMR, red ring)
*(Source: [PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md))*

### Resolved: ForceShield Blackout + IronCurtain Storage
- `HouseClass::SpyPowerSabotage` (0x0050BC90): forces `PowerOutput=0` for duration
- Uses `ForceShieldBlackoutDuration` at **Rules+0x17C0** (NOT 0x17C4 as initially assumed)
- Default: 1000 frames (twice the shield's 500-frame duration)
- Rules+0x17C4 = `ForceShieldPlayFadeSoundTime` (75 frames before expiry, plays warning sound)
- IronCurtain stored on TechnoClass: StartFrame +0x18C, Duration +0x194, IsForceShield +0x1C4
- No early removal — effect times out
*(Source: IRONCURTAIN_FORCESHIELD_GHIDRA_REPORT.md)*

### Resolved: Nuke Full Chain
Carrier missile → NukeMaker warhead (WarheadTypeClass+0x176) → `NukeMaker__SpawnDownwardNuke`
(0x0046B310) → GiantNukeDown bullet with NUKE warhead → screen flash (30 frames hardcoded) →
NUKEBALL anim → `NukeGroundZero__ApplyDamage` (0x004251F0) with Rules.NukeWarhead (Rules+0xF8C)
→ RadSite with RadLevel=500.
*(Source: NUKE_SUPERWEAPON_GHIDRA_REPORT.md)*

### Resolved: Genetic Mutator Mechanism
Mutation is **indirect** through death animation system:
SW kills infantry with MutateWarhead (Rules+0xF98, InfDeath=9) → plays GENDEATH anim →
`MakeInfantry=0` in artmd.ini (AnimTypeClass+0x34C) → AnimClass::AI spawns
`AnimToInfantry[0]` = BRUTE owned by firing player.
*(Source: SUPERWEAPON_LAUNCH_HANDLERS_REPORT.md)*

### Minor Remaining Unknowns
- SuperClass offsets 0x44–0x5F: likely CDTimerClass padding + internal state. Not gameplay-critical.
- Global SWType array at 0x00A8E328: confirmed but per-entry size 0x100 needs cross-check.
- AI::SuperLaunchCheck_DualSW (0x006EFE60): not fully decompiled yet.

---

## Sources

### Ghidra Addresses Decompiled (Core Report)
- `0x006CE5B0` — SuperWeaponTypeClass::Constructor
- `0x006CEA20` — SuperWeaponTypeClass::ReadINI
- `0x006CE800` — SuperWeaponTypeClass::Load (save-game constructor)
- `0x006CE910` — SuperWeaponTypeClass::ComputeChecksum
- `0x006CEEF0` — SuperWeaponTypeClass::FindOrAllocate
- `0x006CAEC0` — SuperClass::Constructor (0-param)
- `0x006CAF90` — SuperClass::Constructor (2-param)
- `0x006CB560` — SuperClass::Activate
- `0x006CB4D0` — SuperClass::Suspend
- `0x006CB7B0` — SuperClass::Deactivate
- `0x006CC080` — SuperClass::AI_Charging
- `0x006CBCA0` — SuperClass::AI_Ready
- `0x006CBEE0` — SuperClass::AnimStage
- `0x006CC2B0` — SuperClass::NameReadiness
- `0x006CC390` — SuperClass::Launch (all 12 cases)
- `0x0050AF10` — HouseClass::AI_ResumeProduction
- `0x004F8440` — HouseClass::Update (SW tick loop)
- `0x00508DF0` — HouseClass::CheckSuperweaponReady
- `0x006EFC70` — AI::SuperLaunchCheck_SingleSW

### Ghidra Addresses Decompiled (Deep Dive Reports)

**Lightning Storm:**
- `0x00539EB0` — LightningStorm::Start
- `0x0053A6C0` — LightningStorm::Process (per-tick)
- `0x0053A140` — LightningStorm::SpawnCloudAnim
- `0x0053A300` — LightningStorm::StrikeBolt (damage + visual)
- `0x0053A090` — LightningStorm::RequestEnd
- `0x00539760` — LightningStorm::Clear/Reset
- `0x0053C280` — SetWeatherOverlay (shared lighting controller)

**Psychic Dominator:**
- `0x0053AE50` — PsychicDominator::Start
- `0x0053AF40` — PsychicDominator::StateMachine (5-phase tick)
- `0x0053B080` — PsychicDominator::Fire (area MC application)

**Nuke:**
- `0x0046B050` — BulletClass::Allocate (COM CoCreateInstance)
- `0x004664C0` — BulletClass::Init
- `0x00468670` — BulletClass::Fire
- `0x0046B310` — NukeMaker::SpawnDownwardNuke
- `0x0053AB70` — ScreenNukeFlash (30 frames hardcoded)
- `0x004251F0` — NukeGroundZero::ApplyDamage
- `0x004690B0` — WarheadTypeClass::Detonate (NukeMaker check at +0x176)

**IronCurtain / ForceShield:**
- `0x0070E2B0` — TechnoClass::IronCurtain (vtable+0x154)
- `0x00457C90` — BuildingClass::IronCurtain (override)
- `0x0041BF40` — TechnoClass::IsIronCurtainActive (vtable+0x160)
- `0x0050BC90` — HouseClass::SpyPowerSabotage

**ParaDrop / SpyPlane / GeneticMutator:**
- `0x0065E660` — ParaDrop::SpawnAircraft
- `0x0065EAB0` — SpyPlane::SpawnAircraft
- `0x00423AC0` — AnimClass::AI (MakeInfantry spawn for mutation)

### Memory Inspected
- `0x008425C0–0x008425F0` — Type enum pointer table (12 entries)
- `0x007E4C50–0x007E4D74` — Action enum pointer table (73 entries)
- `0x007F4100` — RechargeTime conversion factor (900.0f)
- `0x00A8E328` — Global SuperWeaponTypeClass DynamicVectorClass
- `0x00B0C038–0x00B0C05C` — 3×3 cell adjacency table

### Companion Research Documents (created by deep-dive agents)
- `NUKE_SUPERWEAPON_GHIDRA_REPORT.md` — Full nuke chain from carrier to radiation
- [PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md) — 5-phase state machine + area MC
- `IRONCURTAIN_FORCESHIELD_GHIDRA_REPORT.md` — Invulnerability storage + blackout mechanic
- [SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md) — Unknown offsets, Action enum, CDTimerClass
- `SUPERWEAPON_LAUNCH_HANDLERS_REPORT.md` — GeneticMutator, ParaDrop, SpyPlane handlers

### Prior Research Documents Referenced
- [CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md) — Chrono-specific details (verified, consistent)
- [SPECIAL_BUILDINGS_POWER_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPECIAL_BUILDINGS_POWER_SYSTEM.md) — Power suspend logic (verified, consistent)
- [TECHNOCLASS_CHRONO_OFFSETS_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_CHRONO_OFFSETS_VERIFIED.md) — Warp field offsets (verified, consistent)
- [HOUSECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOUSECLASS_GHIDRA_REPORT.md) — Partial SuperClass field map (extended here)
- [TEMPORAL_WARP_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TEMPORAL_WARP_PIPELINE_GHIDRA_REPORT.md) — TemporalClass linked list (referenced)
- [TELEPORT_LOCOMOTION_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TELEPORT_LOCOMOTION_DEEP_DIVE.md) — TeleportLocomotion fields (referenced)

### INI Files Checked
- `ini/rulesmd.ini` — All 12 SW type sections, [General] keys, building SuperWeapon= keys
- `ini/rules.ini` — Base RA2 SW definitions
