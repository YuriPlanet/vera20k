# Power System — Ghidra Analysis of gamemd.exe

Source: `D:\ra2mdpost\House.CPP`, `D:\ra2mdpost\Building.CPP`

Comprehensive reverse-engineering of the RA2/YR power system from gamemd.exe binary
via Ghidra MCP live decompilation.

---

## Key Addresses

| Address | Name | Size | Purpose |
|---------|------|------|---------|
| `0x004FCE30` | HouseClass::PowerRatio | ~50 B | Returns float ratio of output/drain |
| `0x0044E7B0` | BuildingClass::GetPowerOutput | ~200 B | Per-building output (health-scaled) |
| `0x0044E880` | BuildingClass::GetPowerDrain | ~120 B | Per-building drain (NOT health-scaled) |
| `0x00508C30` | HouseClass::AI_AssessPower | ~360 B | Sums all buildings, detects transitions |
| `0x00452260` | BuildingClass::GoOnline | ~250 B | Player-initiated power on |
| `0x00452360` | BuildingClass::GoOffline | ~180 B | Player-initiated power off |
| `0x004545D0` | BuildingClass::OnPowerOff | ~300 B | Visual: swap anims for unpowered state |
| `0x004547C0` | BuildingClass::OnPowerOn | ~280 B | Visual: swap anims for powered state |
| `0x004555D0` | BuildingClass::IsOperational | ~240 B | vtable+0x350: central "can this building function" check |
| `0x00447F10` | BuildingClass::GetFireError | ~340 B | vtable+0x3C0: weapon firing gate, calls IsOperational |
| `0x0044ACF0` | BuildingClass::Mission_Attack | ~680 B | Attack mission handler, direct power check for ChargeMode |
| `0x00508DF0` | HouseClass::CheckSuperweaponReady | ~350 B | Enables/disables radar based on power (verified via `get_function_by_address 0x00508DF0`, 2026-05-20) |
| `0x00508F60` | HouseClass::CheckLowPower | ~160 B | SpySat-only reveal/restore scan + EVA; filters exclusively on BuildingTypeClass+0x16A5 (`SpySat=`), has zero PowerOutput/PowerDrain reference (corrected 2026-07-18: was "Triggers low-power shroud/EVA effects" — this function is unrelated to power ratio; decompile_function 0x00508F60 — INFERENCE_HARDENED) |
| `0x0050AF10` | HouseClass::AI_ManageProduction | ~400 B | Updates production sidebar on transition (verified via `get_function_by_address 0x0050AF10`, 2026-05-20) |
| `0x006F47A0` | FactoryClass::GetBuildStepTime | ~150 B | Production speed scaled by PowerRatio (verified via `get_function_by_address 0x006F47A0`, 2026-05-20) |
| `0x004F8440` | HouseClass::Update | ~2700 B | Main tick — orchestrates power recalc |

---

## HouseClass Power Fields

| Offset | Size | Field | Notes |
|--------|------|-------|-------|
| +0x53A4 | 4 | PowerOutput | Sum of all buildings' GetPowerOutput() |
| +0x53A8 | 4 | PowerDrain | Sum of all buildings' GetPowerDrain() |
| +0x5778 | 1 | NeedsPowerRecalc | Set by GoOnline/GoOffline, cleared by AssessPower |
| +0x5779 | 1 | RecheckRadar | Set by AI_AssessPower (and GoOnline), cleared by CheckSuperweaponReady/CheckLowPower (verified via `decompile_function 0x00508C30` + `decompile_function 0x00452260`, 2026-05-20; was "PowerRecalcDone") |
| +0x577A | 1 | SpySatActive | True when a house-owned `SpySat=yes` building is active and the map-wide reveal blackout is applied; set/cleared by CheckLowPower — has no relation to the power ratio (corrected 2026-07-18: was "low-power shroud blackout"; binary `HouseClass::CheckLowPower` has zero PowerOutput/PowerDrain reference, filters solely on BuildingTypeClass+0x16A5 — decompile_function 0x00508F60 — INFERENCE_HARDENED; was "LowPowerShroudActive") |
| +0x577B | 1 | HasOccupiedPowerPlant | True if any building has passengers AND produces power |
| +0x2D8 | 4 | PowerSurplus | PowerOutput minus PowerDrain (used by PoweredUnit= check) |
| +0x2A4 | 4 | SpyBlackoutStartFrame | Frame when spy blackout began (-1 = inactive) |
| +0x2A8 | 4 | (intermediate write in SpyPowerSabotage) | Written by `HouseClass__SpyPowerSabotage` (0x50BC90) — NOT unused; role unclear but field receives a value (verified via `decompile_function 0x0050BC90`, 2026-05-20; was "(padding/unused)") |
| +0x2AC | 4 | SpyBlackoutDuration | Duration in frames |
| +0x578C | 4 | DamageDelayTimerStart | Written in constructor, NEVER READ (vestigial) |
| +0x5794 | 4 | DamageDelayTimerDuration | Written in constructor, NEVER READ (vestigial) |

**Previous doc corrections:** +0x53A4 and +0x53A8 were previously labeled
"AttackPowerSum" and "DefensePowerSum" in [HOUSECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOUSECLASS_GHIDRA_REPORT.md). They are
actually the power output and drain totals used by the power system.

---

## BuildingTypeClass Power Fields

| Offset | Size | Field | INI Key |
|--------|------|-------|---------|
| +0xEE0 | 4 | PowerOutput | `Power=` (positive part) |
| +0xEE4 | 4 | PowerDrain | `Power=` (negative part, stored as positive) |
| +0xEE8 | 4 | ExtraPowerBonus | Upgrade power bonus |
| +0xEEC | 4 | ExtraDrainBonus | Upgrade drain bonus |
| +0x1552 | 1 | NeedsEngineer | `NeedsEngineer=yes` — building inert until engineer enters |
| +0x1573 | 1 | Powered | `Powered=yes` — checked by IsOperational |
| +0x1574 | 1 | PoweredSpecial | `PoweredSpecial=yes` — spy blackout / special power check |
| +0x16A4 | 1 | Radar | `Radar=yes` (used in radar power check) |
| +0x16A5 | 1 | SpySat | `SpySat=yes` — the ONLY building-type filter for `HouseClass::CheckLowPower` (0x508F60)'s full-map-reveal shroud toggle; unrelated to `PoweredSpecial` (+0x1574) or to power ratio (corrected 2026-07-18: was "PoweredSpecialShroud... Used in low-power shroud check"; binary `BuildingTypeClass::ReadINI` at `0x0045ff72` pushes string `"SpySat"` (`0x0081AE58`) into `CCINIClass::ReadBool`, result stored at +0x16A5 — decompile_function 0x0045FE50 — INFERENCE_HARDENED) |
| +0x16B8 | 1 | IsChargeMode | `IsChargeMode=yes` — Tesla Coil charge attack mode |
| +0x16C0 | 1 | NoPowerToggle | **UNVERIFIED** — GoOnline binary (decompile_function 0x00452260, 2026-05-28) has NO check on this offset; the only GoOnline guards are HasPower==false AND EMPLockRemaining==0. Field may exist in TypeClass but is not read by GoOnline — INFERENCE_HARDENED |

## BuildingClass Power Fields

| Offset | Size | Field | Notes |
|--------|------|-------|-------|
| +0x198 | 1 | IsOnlineStatus | General "active" flag (same byte as +0x660 via int* indexing) |
| +0x504 | 4 | EMPLockRemaining | EMP counter; > 0 = EMPed, non-operational. Decrements each tick. |
| +0x660 | 1 | IsOnline | Player-toggled power state (GoOnline/GoOffline) |
| +0x661 | 1 | CanOperate | Set by upgrade/power check at 0x450590 |
| +0x67C | 4 | UpgradeCount | Number of upgrades attached; >= 2 bypasses power check |
| +0x6C8 | 1 | CachedIsActive | Cached IsOperational result from previous tick |
| +0x668 | 1 | IsOverpowered | Set when building has upgrade bonus → adds ExtraPowerBonus to output |
| +0x669 | 1 | IsChargeDraining | Set when GapGenerator is actively charging → adds ExtraDrainBonus to drain |
| +0x6CC | 1 | EngineerCaptured | Set when engineer enters NeedsEngineer building |
| +0x6EA | 1 | PowerAnimState | Toggled by GoOnline (=1) / GoOffline (=0) for anim switching |

---

## Core Algorithm: HouseClass::AI_AssessPower (0x508C30)

Called from `HouseClass::Update` when `NeedsPowerRecalc` (+0x5778) is set.

```
1. Save old low-power state for transition detection
   was_low_power = (PowerOutput < PowerDrain) && (PowerDrain != 0)

2. Clear NeedsPowerRecalc flag (+0x5778)
   Zero PowerOutput (+0x53A4) and PowerDrain (+0x53A8)

3. Iterate all buildings owned by this house:
   for each building:
     skip if: NULL, IsBeingDestroyed, or !IsAlive
     skip if: campaign mode AND not human/player-controlled AND !flag_0x41B

     PowerOutput += BuildingClass::GetPowerOutput()
     PowerDrain  += BuildingClass::GetPowerDrain()

     if building has occupants (0x1D0 != 0) AND produces power:
       HasOccupiedPowerPlant = true

4. Store HasOccupiedPowerPlant at +0x577B

5. Handle spy blackout / occupied reactor:
   PowerOutput = 0 when ANY of these conditions holds:
     a) blackout start == -1 AND duration != 0  (blackout pending/active with no start recorded)
     b) blackout start != -1 AND elapsed < duration  (timer still running)
     c) blackout expired AND HasOccupiedPowerPlant is true  (garrisoned reactor zeroes output even after blackout ends)
   (corrected 2026-05-28: was "if blackout is active"; binary also zeroes output for occupied reactor even when
    blackout has expired — decompile_function 0x00508C30 — OPERATOR_OR_ORDER_DRIFT)

6. Update factory production speeds (FactoryClass::RecalcAllRates at 0x4CA6E0)
   (corrected 2026-05-28: was "FUN_004CA6E0"; Ghidra label confirmed via decompile_function 0x004CA6E0)

7. Detect transition:
   new_low_power = (PowerOutput < PowerDrain) && (PowerDrain != 0)
   if was_low_power != new_low_power:
     AI_ManageProduction()  ← updates sidebar/production queue (was "HandlePowerTransition")

8. Set RecheckRadar flag (+0x5779)  (was "PowerRecalcDone")
(verified via `decompile_function 0x00508C30`, 2026-05-20)
```

---

## GetPowerOutput (0x44E7B0) — Health-Scaled

```
if InLimbo(): return 0

base = TypeClass->PowerOutput (+0xEE0)

if building has been upgraded:
  base += TypeClass->ExtraPowerBonus (+0xEE8)

if (TypeClass has power upgrade flags (+0x16AE, +0x16AF)):
  if (ExtraPowerBonus > 0 AND occupant_count > 0):
    base += ExtraPowerBonus * occupant_count

if building has upgrade slots (3 slots at +0x17B):
  for each occupied upgrade slot:
    base += upgrade_type->PowerOutput

if base > 0 AND building IsOnline (+0x198):
  return ftol(base * GetHealthRatio())

return 0
```

**Key insight:** Output scales linearly with HP. A power plant at 50% HP produces
50% of its rated output.

**Health ratio precision:** `ObjectClass::GetHealthRatio` (0x5F5C60) computes
`(float80)(Health) / (float80)(Strength)` using x87 80-bit extended precision.
The result is multiplied by base power via `FIMUL` (integer × extended float),
then truncated to int via `ftol()`. This means the scaling uses maximum FPU
precision before final truncation — NOT float32 or integer division.

---

## GetPowerDrain (0x44E880) — NOT Health-Scaled

```
if InLimbo() OR NOT IsOnline (+0x198): return 0

drain = TypeClass->PowerDrain (+0xEE4)

if IsChargeDraining (+0x669):   ← GapGenerator actively charging
  drain += TypeClass->ExtraDrainBonus (+0xEEC)

if building has upgrade slots:
  for each occupied upgrade slot:
    drain += upgrade_type->PowerDrain

return drain
```

**Key insight:** Drain is always the full rated value regardless of building health.
A Tesla Coil at 1 HP still drains the full 75 power.

---

## PowerRatio (0x4FCE30) — Continuous Float

```c
int output = this->PowerOutput;  // +0x53A4
int drain  = this->PowerDrain;   // +0x53A8

if (output < drain && drain != 0) {
    if (output != 0)
        return (float)(output) / (float)(drain);  // ratio 0.0 < r < 1.0
    else
        return 0.0f;
}
return 1.0f;  // full power (or no drain)
```

**Constants verified from binary:**
- `_DAT_007e1718` = `3FF0000000000000` (double 1.0) — "full power" threshold
- `_FLOAT_007e2800` = `00000000` (float 0.0) — "no power" value

**PowerRatio consumers (11 call sites verified):**
- `BuildingClass::IsOperational` (0x4555D0) — central building active check (ratio < 1.0 = disabled)
- `BuildingClass::Mission_Attack` (0x44ACF0) — ChargeMode firing check (ratio < 1.0 = can't fire)
- `BuildingClass::Update` (0x43FB20) — upgrade bypass check on state transitions
- `BuildingClass::UpdateUpgradeSlots` (0x450590) — docking availability
- `FactoryClass::GetBuildStepTime` (0x6F47A0) — scales production time (was "GetProductionSpeed"; verified via `get_function_by_address 0x006F47A0`, 2026-05-20)
- `HouseClass::CheckSuperweaponReady` (0x508DF0) — radar enable/disable (inline check) (was "UpdateRadarPowerState"; verified via `get_function_by_address 0x00508DF0`, 2026-05-20)
- AI superweapon launch: 0x6EFC70, 0x6EFE60, 0x6F0130 — hold launches during low power
- AI threat assessment (0x4723B0) — AI strategy selection
- Trigger condition evaluator (0x71E940) — LowPower/HasFullPower conditions

**Two usage patterns:**
1. **Binary threshold** (ratio < 1.0): IsOperational, GetFireError, radar, superweapons.
   ANY deficit disables these features entirely.
2. **Continuous multiplier**: Production speed. Production slows proportionally to
   the power deficit, not in discrete steps.

---

## GoOnline / GoOffline — Player Commands Only

**Critical finding:** GoOnline (0x452260) and GoOffline (0x452360) are **ONLY**
triggered by:

1. **Network commands** (EventClass::Execute at 0x4C6CB0, cases 1 and 2) — when
   the player clicks the power toggle button on a building
2. **Map triggers** (TriggerActionClass::Execute at 0x6DD8B0, cases 0x3D and 0x3E)
   — scripted events in campaign maps

**Buildings are NOT automatically toggled online/offline based on power level.**
The house-level low power state affects buildings through the PowerRatio check
in their various operations, not through explicit GoOnline/GoOffline calls.

### GoOnline conditions:
```
if building is currently offline (+0x660 == 0)
   AND GoOnlineLock (+0x504) == 0:
  set +0x660 = 1 (online)
  mark house NeedsPowerRecalc  (+0x5778 = 1)
  mark house RecheckRadar      (+0x5779 = 1)  ← also set here, same tick
  restart building animations
  recalculate wall connections
  check superweapon availability
```
(verified via `decompile_function 0x00452260`, 2026-05-20; +0x5779 set was missing from prior pseudocode)

### GoOffline conditions:
```
if building is currently online (+0x660 != 0)
   AND (TypeClass->PowerDrain > 0 OR TypeClass->Powered flag):
  set +0x660 = 0 (offline)
  mark house NeedsPowerRecalc  (+0x5778 = 1)
  mark house RecheckRadar      (+0x5779 = 1)  ← corrected 2026-05-28: was missing; binary shows GoOffline sets both +0x5778 and +0x5779 via decompile_function 0x00452360 — OPERATOR_OR_ORDER_DRIFT
  sidebar dirty flag           (+0x1FC = 1)
  call ApplyOfflineEffects (swap animations, play EVA if local player)
```

**GoOffline guard:** Power plants (positive Power=, no drain) can NEVER be taken
offline. Only buildings that drain power or have `Powered=yes` can be toggled off.

---

## How Powered=yes Buildings Are Disabled During Low Power

### The Central Function: BuildingClass::IsOperational (vtable+0x350)

**Address:** 0x4555D0 (mislabeled `CanSellOrUndeploy` in Ghidra)
**Vtable slot:** BuildingClass vtable (at 0x7E3EBC) + 0x350 = 0x7E420C

This is the single most important function for determining whether a `Powered=yes`
building can function. It is called through the vtable (`*param_1 + 0x350`) by
multiple systems. Returns 1 (operational) or 0 (disabled).

```c
bool BuildingClass::IsOperational() {
    // 1. Must be online (player toggle) OR have 2+ upgrades
    if (!IsOnline && UpgradeCount < 2)
        return false;

    // 2. Must not be EMPed
    if (EMPLockRemaining > 0)               // +0x504
        return false;

    // 3. Must be alive (HP > 0)
    if (Health == 0)
        return false;

    // 4. THE POWER CHECK:
    //    If Powered=yes AND building drains power AND house has low power
    //    AND not upgraded with 2+ upgrades => disabled
    if (Type->Powered                          // +0x1573
        && Type->PowerDrain > 0                // +0xEE4
        && House->PowerRatio() < 1.0           // calls 0x4FCE30
        && UpgradeCount < 2)                   // upgrades bypass power check!
        return false;

    // 5. PoweredSpecial check (spy blackout timer + occupied power plant)
    if (Type->PoweredSpecial) {                // +0x1574
        if (House->SpyBlackoutTimer is active) // +0x2A4/+0x2AC
            return false;
        if (House->HasOccupiedPowerPlant)      // +0x577B (true = has garrisoned reactor)
            return false;                      // disables superweapons when reactors have passengers
    }

    // 6. NeedsEngineer check
    if (Type->NeedsEngineer                    // +0x1552
        && !EngineerCaptured)                  // building +0x6CC
        return false;

    // 7. Must not be in Construction (0x12) or Selling (0x13) mission
    //    (Verified from mission string table at 0x816CAC — see POWER_INI_PARSING_AND_LIFECYCLE.md)
    if (GetMission() == 0x12 || GetMission() == 0x13)
        return false;

    return true;
}
```

**Key insight on the power check:** The condition is `PowerRatio() < 1.0`, meaning
ANY power deficit disables the building. This is a binary threshold (full power or
not), NOT a gradual degradation. Even 1 unit of deficit (e.g., 199/200) causes
ALL `Powered=yes` buildings to stop functioning.

**Upgrade bypass:** Buildings with 2 or more upgrades attached bypass BOTH the
IsOnline check AND the power check. This is why upgraded buildings keep working
during low power.

---

### How Each System Uses IsOperational

#### 1. Weapon Firing: GetFireError (vtable+0x3C0, address 0x447F10)

The fire error check is the gatekeeper for all building weapon attacks:

```c
int BuildingClass::GetFireError(target, weapon, canAttack) {
    // ...various checks...
    if (GetMission() != Construction && GetMission() != Selling) {
        if (!this->IsOperational())         // vtable+0x350 call
            return FIRE_CANT;               // returns 6 = cannot fire
        // ...range checks, etc...
    }
}
```

**Call chain:** Mission_Attack -> GetFireError -> IsOperational -> PowerRatio check

A Tesla Coil with `Powered=yes` calls IsOperational through GetFireError every time
it tries to fire. If power is insufficient, GetFireError returns 6 (FIRE_CANT),
preventing the shot entirely.

#### 2. Tesla Coil Charge Mode: Mission_Attack (0x44ACF0)

Buildings with `IsChargeMode=yes` (like the Tesla Coil) have a SECOND, direct
power check in their attack mission handler, independent of GetFireError:

```c
int BuildingClass::Mission_Attack() {
    // ...
    if (Type->IsChargeMode) {
        if (FiringState != 0) {
            // Already firing, continue...
        }
        // Direct power check before even trying to acquire targets:
        if (!Type->Powered                     // Powered=no: always proceed
            || Type->PowerDrain <= 0           // no drain: always proceed
            || House->PowerRatio() >= 1.0) {   // full power: proceed
            // TRY TO FIRE: check target validity, start charge sequence
        }
        // else: Powered=yes AND low power => do NOTHING, return 1
        //       building sits idle, doesn't even look for targets
    }
}
```

This means during low power, the Tesla Coil doesn't just fail to fire -- it
**stops scanning for targets entirely**. The building becomes completely inert.

#### 3. BuildingClass::Update (0x43FB20) — State Change Detection

The main building update function calls IsOperational every tick and compares
the result against a cached previous state:

```c
void BuildingClass::Update() {
    bool is_active = this->IsOperational();      // vtable+0x350

    // Filter out construction/selling states
    if (!is_active || mission == Construction || mission == Selling)
        is_active = false;

    if (is_active != this->CachedIsActive) {     // building +0x6C8
        // State changed! Update animations:
        UpdateGapAndSpecialEffects();             // 0x4549B0
        this->CachedIsActive = is_active;
    }
    // ...rest of update...
}
```

When power drops and IsOperational flips from true to false,
`UpdateGapAndSpecialEffects` triggers:
- Gap Generators stop providing gap
- Spy satellite reveals stop
- Calls `OnPowerOff()` (0x4545D0) to swap building anims to unpowered variants
- Calls `OnPowerOn()` (0x4547C0) when power is restored

#### 4. Radar: HouseClass::CheckSuperweaponReady (0x508DF0) — Ghidra label is a mislabel, function is 100% radar

(corrected 2026-07-18: the Ghidra symbol name "CheckSuperweaponReady" is confirmed WRONG — the function
body has no SuperWeapon-type field, no charge/ready timer, and no `PsychicDetectionRadius` reference
anywhere; it writes `RadarClass+0x14D8` through a callee whose own debug string is
`"Radar/TacticalMap availability is %s"` and compares against `RadarClass::IsTacticalMapAvailable`
(0x00656DE0) — decompile_function 0x00508DF0, decompile_function 0x00656DF0 — RTTI_LABEL_DRIFT.
Function address/entry itself is correct, only the semantic label is wrong; prose below already
described the correct radar behavior, so address/offset citations elsewhere in this doc are unaffected)

Radar does NOT use the per-building IsOperational check. Order of operations, read directly from
the decompile:

```c
void HouseClass::CheckSuperweaponReady() {           // Ghidra label; actually a radar-availability gate
    this->RecheckRadar = false;                        // +0x5779, cleared unconditionally
    if (this != g_PlayerPtr) return;                   // local player only

    // 1. Spy radar blackout timer (+0x2B0 start / +0x2B8 duration) — a DISTINCT timer from the
    //    +0x2A4/+0x2AC SpyPowerBlackout pair used by AI_AssessPower/IsOperational.
    //    While start==-1 && duration!=0, OR elapsed<duration: skip straight to the compare step
    //    below with the result left at its default false (radar forced unavailable).

    // 2. ScenarioClass+0x34A4 = `FreeRadar=` (map [Basic] INI key — NOT SpySat, NOT FogOfWar).
    //    If set: jump straight to "tactical map available = TRUE", skipping steps 3-4 entirely.
    if (ScenarioClass->FreeRadar) {                     // +0x34A4
        result = true;
    } else {
        // 3. House-level power ratio check:
        int output = this->PowerOutput;                 // +0x53A4
        int drain  = this->PowerDrain;                  // +0x53A8
        if (drain <= output || drain == 0 ||
            (output != 0 && (double)output / (double)drain >= 1.0)) {
            // 4. Power sufficient: scan for the FIRST building (list order) passing ALL of:
            for each building in house->BuildingList:
                if (building != NULL
                    && building->IsOnline                  // +0x660
                    && building->Type->Radar                // +0x16A4
                    && !building->IsBeingSold                // +0x81
                    && building->IsAlive                     // +0x74
                    && building->Mission != Selling)         // != 0x13 (checked twice)
                {
                    if (building->EMPLockRemaining == 0      // +0x504
                        && !building->IsWarpingOut())        // vtable+0x1D4 -> reads +0x270
                        result = true;
                    break;   // <-- stops here regardless of pass/fail; does NOT try the next building
                }
        }
    }
    // 5. Compare `result` against RadarClass::IsTacticalMapAvailable(); if changed, call
    //    FUN_00656DF0(result), which writes RadarClass+0x14D8 and calls
    //    RadarClass::ActivateDeactivate / RadarClass::SetRadarMode.
}
```

**Key difference:** Radar checks power at the house level (PowerOutput vs
PowerDrain) and then checks individual building validity separately. It does NOT
call IsOperational. **First-match-only:** the building scan stops at the first
`Radar=yes` building passing the coarse filters (online/alive/not-selling) — if
that one building fails the EMP/warp gate, the loop breaks WITHOUT trying any
other radar building later in list order (corrected 2026-07-18: prior text implied
scanning continues until a match is found — decompile_function 0x00508DF0 —
OPERATOR_OR_ORDER_DRIFT). The vtable+0x1D4 call is `TechnoClass::IsWarpingOut`
(returns `this->field_0x270`), NOT `IsCloaked` (corrected 2026-07-18: was
"!building->IsCloaked()"; `read_memory 0x007E4090` on the BuildingClass vtable
resolves to `0x0070C5B0` = `TechnoClass__IsWarpingOut` — decompile_function
0x0070C5B0 — INFERENCE_HARDENED). The `+0x504` field is `EMPLockRemaining`
(matches this doc's own BuildingClass field table), NOT "IsTemporallyWarped"
as the prior pseudocode labeled it in this one spot (corrected 2026-07-18 —
OFFSET_RETYPED_WRONG). The "PsychicDetectionRadius provides radar regardless
of power" claim and the "RadarClass::SetRadarDisabled" function name are both
REMOVED — neither exists anywhere in the decompiled body (corrected 2026-07-18:
decompile_function 0x00508DF0 shows no `PsychicDetectionRadius` access at all;
the actual callee is `FUN_00656DF0`, which writes `RadarClass+0x14D8` and calls
`RadarClass::ActivateDeactivate`/`RadarClass::SetRadarMode` — decompile_function
0x00656DF0 — INFERENCE_HARDENED).

---

## Low Power Effects Summary

When `PowerOutput < PowerDrain`, the house enters low power. This affects:

### 1. All Powered=yes Buildings (via IsOperational)
- **Weapons:** GetFireError returns FIRE_CANT, preventing all attacks
- **Charge mode:** Tesla Coils stop scanning for targets entirely
- **Gap generators:** Stop providing gap (via UpdateGapAndSpecialEffects)
- **Animations:** Swap to unpowered variants (OnPowerOff/OnPowerOn)
- The check is binary: ANY deficit disables ALL Powered=yes buildings

### 2. Production Speed
`FactoryClass::GetBuildStepTime` (0x6F47A0) calls `PowerRatio()` and uses
the ratio as a multiplier. Production time = base_time / ratio. At 50% power,
production takes 2x longer. (was "GetProductionSpeed"; verified via `get_function_by_address 0x006F47A0`, 2026-05-20)

### 3. Radar / Shroud
`HouseClass::CheckSuperweaponReady` (0x508DF0) — Ghidra label is a mislabel; function is radar/tactical-map
availability, NOT superweapon-related (corrected 2026-07-18: decompile_function 0x00508DF0 has zero
SuperWeapon-type/charge-timer reference — RTTI_LABEL_DRIFT):
- Only runs for the local player
- If `ScenarioClass+0x34A4` (`FreeRadar=`, a map `[Basic]` key) is set, short-circuits straight to
  "radar available" — this is NOT a SpySat check (corrected 2026-07-18: was "Checks house-level power
  ratio first"/implicitly SpySat-gated; binary reads `s_FreeRadar_0083dff8` via
  `ScenarioClass::Read_INI_Basic` at 0x00689E90 — decompile_function 0x00689E90 — INFERENCE_HARDENED)
- Otherwise checks house-level power ratio (must be >= 1.0), then scans for the FIRST (list-order)
  building with `Radar=yes` flag (+0x16A4) passing online/alive/not-selling/EMP/not-warping-out gates
- If power is full AND that first matching building passes: radar enabled
- Otherwise: radar disabled — this is unrelated to SpySat (see below)

`HouseClass::CheckLowPower` (0x508F60) — despite the Ghidra label, this function has NO power-ratio
reference at all; it is a `SpySat=yes`-only reveal/restore scan (corrected 2026-07-18: was "CheckPoweredBuildings";
decompile_function 0x00508F60 shows zero PowerOutput/PowerDrain/PoweredSpecial access — INFERENCE_HARDENED):
- Iterates buildings with the `SpySat` flag (BuildingTypeClass+0x16A5, INI key `SpySat=`) — NOT
  `PoweredSpecial` (+0x1574), which is a separate flag consumed only by `IsOperational` (corrected
  2026-07-18: was "Iterates buildings with PoweredSpecial flag (+0x16A5)" — that offset is `SpySat`,
  not `PoweredSpecial`, and this function doesn't read `PoweredSpecial` at all — OFFSET_RETYPED_WRONG)
- On finding an eligible active `SpySat=yes` building: calls shroud application (0x577D90) + EVA voice
- On transition to full power: calls shroud reset (0x577AB0) + EVA voice

### 4. Sidebar / Production Queue
`HouseClass::AI_ManageProduction` (0x50AF10): (was "HandlePowerTransition"; verified via `get_function_by_address 0x0050AF10`, 2026-05-20)
- Updates production availability in sidebar
- Checks if items can still be built given new power state

### 5. Superweapon Charging (AI)
AI functions at 0x6EFC70, 0x6EFE60, 0x6F0130 check `PowerRatio() >= 1.0` before
launching superweapons. During low power, superweapons are held back.

### 6. Map Triggers
Trigger condition 0x1E (`LowPower`): true when `PowerRatio < 1.0`
Trigger condition 0x3A (`HasFullPower`): true when `PowerRatio >= 1.0`

### 7. DamageDelay Degradation — VESTIGIAL / UNIMPLEMENTED

**DamageDelay (`[General] DamageDelay=1`) is parsed from INI and stored at
RulesClass+0x16B0.** The value is converted to frames (minutes × 900) and written
to `HouseClass+0x578C/+0x5794` during construction. However:

**The timer is NEVER READ by any game code.** No function in gamemd.exe checks
these offsets. This was verified by exhaustive byte-pattern search across the
entire code section.

**Confidence: HIGH** — DamageDelay-based degradation damage (gradual HP loss on
`Powered=yes` buildings during low power) is vestigial/unimplemented in YR.
The INI key exists, the value is parsed, but no gameplay logic uses it.

---

## Production Speed Formula — FactoryClass::GetBuildStepTime (0x6F47A0)

(was "GetProductionSpeed"; verified via `get_function_by_address 0x006F47A0`, 2026-05-20)

### RulesClass Constants

| Offset | INI Key | Type | Default (rulesmd.ini) |
|--------|---------|------|-----------------------|
| +0x570 | `MinLowPowerProductionSpeed` | float | 0.5 |
| +0x574 | `MaxLowPowerProductionSpeed` | float | 0.8 |
| +0x578 | `LowPowerPenaltyModifier` | float | 1.0 |
| +0x57C | `MultipleFactory` | float | 0.8 |
| +0x758 | `WallBuildSpeedCoefficient` | double | 3.0 |
| +0x1748 | `BuildSpeed` | double | 0.7 |

**Note:** +0x57C is `MultipleFactory`, NOT `BuildSpeed`. `BuildSpeed` is a double
at +0x1748.

### Complete Algorithm

```c
int TechnoClass::GetProductionTime() {
    // Step 1: Base build time
    //   TypeClass::GetBuildTime() = ftol(Cost * Rules->BuildSpeed * 0.9)
    //   Cost is int at TypeClass+0x610
    //   BuildSpeed is double at Rules+0x1748
    //   The 0.9 constant is hardcoded — not from INI
    int base_time = type->GetBuildTime();

    // Step 2: Per-faction build time multiplier (from HouseTypeClass)
    //   Infantry(0x10)  → BuildTimeInfantryMult  (+0x134)
    //   Unit(0x28)      → BuildTimeUnitsMult     (+0x138)
    //   Aircraft(3)     → BuildTimeAircraftMult   (+0x13c)
    //   Building(7)     → BuildTimeBuildingsMult  (+0x140)
    //   Naval(type 5)   → BuildTimeNavalMult      (+0x144)
    float house_btm = house->GetBuildTimeMult(type);
    base_time = (int)(house_btm * (float)base_time);

    // Step 3: Per-type build time multiplier
    //   TypeClass+0x608, default 1.0
    base_time = (int)((float)base_time * type->BuildTimeMultiplier);

    // Step 4: Low-power production speed penalty
    float power_ratio = house->PowerRatio();    // 0.0 to 1.0

    float adjusted = 1.0 - (1.0 - power_ratio) * Rules->LowPowerPenaltyModifier;
    float speed = max(adjusted, Rules->MinLowPowerProductionSpeed);

    if (power_ratio < 1.0)
        speed = min(speed, Rules->MaxLowPowerProductionSpeed);
    // When power_ratio >= 1.0, speed = 1.0 (no cap)

    if (speed == 0.0)
        speed = 0.01;       // safety floor — never zero-divide

    int result = (int)((float)base_time / speed);

    // Step 5: Multiple factory bonus
    //   For units: Naval flag (TypeClass+0xCCE) selects naval vs land factory count
    //   Factory counts stored in HouseClass:
    //     Infantry (+0x537C), Unit/non-naval (+0x5380),
    //     Building (+0x5384), Aircraft (+0x5378), Naval (+0x5388)
    int factory_count = house->GetFactoryCount(rtti, is_naval);

    if (Rules->MultipleFactory > 0.0 && factory_count > 1)
        for (int i = 0; i < factory_count - 1; i++)
            result = (int)((float)result * Rules->MultipleFactory);
    // With MultipleFactory=0.8 and 3 factories: result *= 0.8^2 = 0.64

    // Step 6: Wall speed coefficient (buildings only)
    if (this->WhatAmI() == Building && type->IsWall)
        result = (int)((double)result * Rules->WallBuildSpeedCoefficient);
        // 3.0 → walls take 3x longer

    return result;
}
```

### How Factories Consume This

Production completes at step **54** (0x36). `FactoryClass::AI` runs each frame:
- `+0x24` = current step counter (0 to 54)
- `+0x38` = step delay (frames per step) = `GetProductionTime() / 54`, clamped [1, 255]
- `+0x60` = remaining cost
- Each tick: step counter increments. Cost paid per step: `remaining / (54 - current)`.
- If house can't afford it, step is decremented (production stalls).
- When step reaches 54, production completes.

Power changes trigger `FactoryClass::UpdateAllStepDelays` (0x4CA6E0) which
recalculates step delay for all active factories owned by the affected house.

---

## Spy Blackout Mechanism

When a spy infiltrates a power plant, the victim's power output is forced to 0.

### Timer model (frame-based, NOT tick-based):
```
HouseClass+0x2A4 = BlackoutStartFrame (set to g_CurrentFrameCounter)
HouseClass+0x2AC = BlackoutDuration (in frames, from [General] SpyPowerBlackout)

Each HouseClass::Update tick:
  elapsed = g_CurrentFrameCounter - BlackoutStartFrame
  if elapsed < BlackoutDuration:
    blackout is still active
  else:
    blackout has expired
```

In `AI_AssessPower`, if blackout is active: `PowerOutput = 0`, forcing the house
into low power regardless of actual building output.

When the blackout expires, `NeedsPowerRecalc` (+0x5778) is set, which triggers
a fresh power assessment on the next update tick.

### Spy Infiltration Trigger: OnSpyInfiltrate (0x4571E0)

**SpyPowerBlackout** is at **RulesClass+0xD64** (integer, frame count), parsed from
`[General] SpyPowerBlackout=` via ReadInt.

When a spy infiltrates a building, `OnSpyInfiltrate` checks the building type to
determine the effect. For **power plants** (`Power > 0` at TypeClass+0xEE0):

```asm
004572cb: MOV EAX,[ECX + 0xee0]     ; TypeClass->PowerOutput
004572d7: TEST EAX,EAX
004572d9: JLE skip                   ; if Power <= 0, not a power plant
004572db: MOV EAX,[EDX + 0xd64]     ; RulesClass->SpyPowerBlackout
004572e1: MOV ECX,[EBP + 0x21c]     ; victim HouseClass*
004572e7: PUSH EAX                  ; push duration
004572e8: CALL 0x0050bc90           ; HouseClass::SetBlackout(duration)
```

**HouseClass::SetBlackout (0x50BC90)** sets:
- `+0x5778` = 1 (NeedsPowerRecalc)
- `+0x2A4` = g_CurrentFrameCounter (BlackoutStartFrame)
- `+0x2AC` = duration parameter (SpyPowerBlackout frames)

### Complete Spy Infiltration Effects

| TypeClass Check | Offset | Condition | Effect |
|-----------------|--------|-----------|--------|
| PowerOutput | +0xEE0 | > 0 | **Power blackout** via SetBlackout |
| Radar | +0x16A4 | != 0 | Radar/shroud reset on victim |
| SuperWeapon | +0x16F0 | != -1 | Superweapon charge reset |
| Storage | +0x800 | > 0 | **Money steal**: victim balance × SpyMoneyStealPercent (Rules+0xD68) |
| Factory | +0xEB8 | == 0x28 (unit) | Grants veteran infantry to spy owner (+0x2C0) |
| Factory | +0xEB8 | == 0x10 (inf) | Grants veteran units to spy owner (+0x2BF) |
| (spy-reveal list) | Rules+0x920 | type matches | Stolen tech flags (+0x2BD/+0x2BE/+0x2BC by side) |

---

## PowersUpBuilding / Upgrade Power System

### BuildingTypeClass Upgrade Fields

| Offset | Size | INI Key | Purpose |
|--------|------|---------|---------|
| +0xE88 | 24 | `PowersUpBuilding=` | Name of parent building (string, 24 chars) |
| +0xEE0 | 4 | `Power=` (positive) | Power output (added to parent via upgrade slot) |
| +0xEE4 | 4 | `Power=` (negative) | Power drain (added to parent via upgrade slot) |
| +0xEE8 | 4 | `ExtraPower=` (positive) | Extra power bonus when upgraded |
| +0xEEC | 4 | `ExtraPower=` (negative) | Extra drain bonus when upgraded |
| +0x14E0 | 4 | `Upgrades=` | Max upgrade slots on parent (0-3) |
| +0x16FC | 4 | `PowersUpToLevel=` | Upgrade level this provides |

### BuildingClass Upgrade Slots

| Offset | Size | Field |
|--------|------|-------|
| +0x5EC | 4 | UpgradeSlot[0] (BuildingTypeClass pointer) |
| +0x5F0 | 4 | UpgradeSlot[1] |
| +0x5F4 | 4 | UpgradeSlot[2] |
| +0x668 | 1 | IsOverpowered (has upgrade bonus applied) |
| +0x702 | 1 | HasUpgrades (any slot non-NULL) |

### How GetPowerOutput Uses Upgrades

```c
int GetPowerOutput(BuildingClass* this) {
    int base = this->Type->PowerOutput;       // +0xEE0

    if (this->InLimbo()) return 0;

    // 1. Overpowered bonus (upgrade-flagged)
    if (this->IsOverpowered)                  // +0x668
        base += this->Type->ExtraPowerBonus;  // +0xEE8

    // 2. Occupant-based power (UnitAbsorb/InfantryAbsorb buildings)
    if ((Type->UnitAbsorb || Type->InfantryAbsorb)  // +0x16AE, +0x16AF
        && Type->ExtraPowerBonus > 0
        && this->OccupantCount > 0)           // +0x114
        base += ExtraPowerBonus * OccupantCount;

    // 3. Upgrade slots (up to 3)
    if (this->HasUpgrades)                    // +0x702
        for i in 0..3:
            slot = this->UpgradeSlots[i]      // +0x5EC + i*4
            if slot != NULL:
                base += slot->PowerOutput     // upgrade type's +0xEE0

    // 4. Health scaling
    if base > 0 && this->IsOnline:            // +0x660
        return base * GetHealthRatio()        // integer math, rounds down

    return 0;
}
```

### Upgrade Bypass in IsOperational

**Critical gameplay mechanic:** Buildings with `UpgradeCount >= 2` (+0x67C) bypass
BOTH the IsOnline check AND the power check in `IsOperational`. This means:
- A building with 2+ upgrades **keeps working during low power**
- A building with 2+ upgrades **works even if manually powered off**

This is verified at address 0x4555D0 in the IsOperational function.

---

## PoweredUnit= Flag (Vehicles)

**TechnoTypeClass+0x410** (bool, 1 byte). Parsed from `[UnitType] PoweredUnit=yes`.

Unlike `Powered=yes` on buildings (which is a binary threshold via IsOperational),
`PoweredUnit=yes` affects **vehicles** and uses a different mechanism:

- Checked in locomotor/movement code (0x718887) and mission handler (0x739F27)
- Calls `FUN_0050e1b0(HouseClass*)` which returns `HouseClass+0x2D8 > 0`
- `+0x2D8` = **PowerSurplus** (PowerOutput minus PowerDrain)
- When surplus <= 0, powered units are **immobilized** and cannot act
- There is NO manual toggle — fully automatic based on house power surplus
- Different check from buildings: uses surplus > 0, NOT PowerRatio < 1.0

**Example:** Robot Tank (`PoweredUnit=yes`) stops moving when the owner has any
power deficit. Unlike buildings which use the ratio check, this is a simple
surplus > 0 test.

---

## Production Speed Formula (Fully Verified from Assembly)

### RulesClass Constants

| Offset | Type | INI Key | Default |
|--------|------|---------|---------|
| +0x570 | float | `MinLowPowerProductionSpeed` | 0.5 |
| +0x574 | float | `MaxLowPowerProductionSpeed` | 0.8 |
| +0x578 | float | `LowPowerPenaltyModifier` | 1.0 |
| +0x57C | float | `MultipleFactory` | 0.8 |
| +0x758 | double | `WallBuildSpeedCoefficient` | 3.0 |
| +0x1748 | double | `BuildSpeed` | 0.7 |

**CORRECTION:** Previous documentation claimed +0x57C was BuildSpeed. It is
**MultipleFactory**. BuildSpeed is at +0x1748 as a **double**, not a float.

### GetBuildTime (0x711EE0)

```c
int TechnoTypeClass::GetBuildTime() {
    return ftol(this->Cost * Rules->BuildSpeed * 0.9);
}
```
- `Cost` at TechnoTypeClass+0x610 (int32), loaded with FILD
- `BuildSpeed` at RulesClass+0x1748 (double)
- `0.9` hardcoded constant at 0x007F4E80 (double `3FECCCCCCCCCCCCD`)

### Complete Formula (annotated from FPU instructions)

```c
int GetProductionTime(TechnoClass* techno) {
    TechnoTypeClass* type = techno->GetType();
    HouseClass* house = techno->Owner;  // +0x21C

    // Phase 1: Base build time
    int base = ftol(type->Cost * Rules->BuildSpeed * 0.9);

    // Phase 2: House-type category multiplier
    float house_mult = house->GetBuildTimeMult(type);
    //   Infantry(0x10) → HouseTypeClass+0x134
    //   Unit(0x28)     → +0x138 (naval check: type 5 → +0x144)
    //   Aircraft(3)    → +0x13C
    //   Building(7)    → +0x140 (naval=type 5 → +0x144)
    //   default        → 1.0
    int step1 = ftol(house_mult * (float)base);

    // Phase 3: Per-type BuildTimeMultiplier
    int step2 = ftol((float)step1 * type->BuildTimeMultiplier);  // +0x608

    // Phase 4: Low power penalty (DIVISION, not multiplication)
    float power_ratio = house->PowerRatio();  // 0.0..1.0
    float deficit = 1.0f - power_ratio;
    float speed = 1.0f - deficit * Rules->LowPowerPenaltyModifier;  // +0x578
    speed = max(speed, Rules->MinLowPowerProductionSpeed);           // +0x570
    if (power_ratio < 1.0f)
        speed = min(speed, Rules->MaxLowPowerProductionSpeed);       // +0x574
    if (speed == 0.0f)
        speed = 0.01f;  // prevent divide-by-zero (const at 0x7F4E34)
    int step3 = ftol((float)step2 / speed);

    // Phase 5: MultipleFactory bonus
    if (Rules->MultipleFactory > 0.0f) {
        int factory_count = house->GetFactoryCount(whatAmI, is_naval);
        for (int i = 0; i < factory_count - 1; i++)
            step3 = ftol((float)step3 * Rules->MultipleFactory);   // +0x57C
    }
    // With MultipleFactory=0.8 and 3 factories: step3 *= 0.8^2 = 0.64

    // Phase 6: Wall special case
    if (whatAmI == BUILDING && type->IsWall)    // +0x1571
        return ftol((float)step3 * (float)Rules->WallBuildSpeedCoefficient);  // +0x758

    return step3;
}
```

### Caller: FactoryClass Step Delay

All three callers divide by **54** (0x36) and clamp to [1, 255]:
```c
int step_delay = GetProductionTime() / 54;
step_delay = clamp(step_delay, 1, 255);
```

Production completes when step counter reaches 54. The step delay (frames between
steps) determines how fast the bar fills. A lower delay = faster production.

---

## Power Recalculation Triggers

`NeedsPowerRecalc` (+0x5778) is the dirty flag that triggers `AI_AssessPower`.
It is set (= 1) by these events:

| Address | Function | Trigger |
|---------|----------|---------|
| `0x43BF11` | BuildingClass destructor | Building destroyed |
| `0x449344` | Building placement | Building placed / captured / upgraded |
| `0x44AB0E` | Building state change | Attack/mission state transition |
| `0x4F846C` | HouseClass::Update | Spy blackout timer expires |
| `0x50BC9C` | HouseClass::SetBlackout | Spy infiltrates power plant |
| `0x70FDAC` | TechnoClass::EnterTransport | Unit enters building (passenger enters reactor) |
| `0x71B126` | Occupant linkage | Unit docks with building |
| GoOnline/GoOffline | Player commands | Manual power toggle |

**Recalculation flow:**
1. Event sets `+0x5778 = 1`
2. `HouseClass::Update` checks `+0x5778` each tick
3. If set, calls `AI_AssessPower` (0x508C30)
4. AI_AssessPower sums all buildings, handles blackout, recalculates factory speeds
5. Detects low-power transition → calls `AI_ManageProduction` (0x50AF10) (was "HandlePowerTransition"; verified via `get_function_by_address 0x0050AF10`, 2026-05-20)
6. Sets `+0x5779 = 1` (RecheckRadar) (was "PowerRecalcDone"; verified via `decompile_function 0x00508C30`, 2026-05-20)
7. `HouseClass::Update` checks `+0x5779` for radar/EVA updates (calls CheckSuperweaponReady + CheckLowPower)

---

## Temporal Warp and Power (Chrono Legionnaire)

When a Chrono Legionnaire's temporal eraser targets a building:

1. Temporal attach (`0x71AF20`) sets `building+0x270 = 1` (IsBeingWarped)
   and sets `HouseClass+0x5778 = 1` (NeedsPowerRecalc)
2. `GetPowerOutput` (0x44E7B0) checks `building+0x270` via vtable+0x1D4
   as its **very first check**. If IsBeingWarped → returns **0**
3. `GetPowerDrain` (0x44E880) has the **identical** first check → returns **0**
4. When warp completes or is cancelled, detach functions (`0x71ABC0`, `0x71ACD0`)
   again set `NeedsPowerRecalc = 1`

**A building being temporally erased contributes ZERO output AND ZERO drain.**
The power system immediately recalculates when a warp begins or ends.

**Note:** The function at `0x70C5B0` (6 bytes, returns `this+0x270`) was previously
labeled `HasPower` in our ADDRESS_MAP. It is actually `IsBeingWarped` / `InLimbo` —
a general "is this object in an inactive state" check.

---

## Trigger Actions Affecting Power

Two map trigger actions directly manipulate building power state:

| Action ID | Name | Behavior |
|-----------|------|----------|
| 0x3D | Disable Power | Iterates all buildings of trigger's house, calls `GoOffline` on each with `HasPower == true` |
| 0x3E | Enable Power | Iterates all buildings of trigger's house, calls `GoOnline` on each with `HasPower == false` |

These are campaign/map-editor actions for scripted power events. They use
`FUN_006E5380` for house matching.

---

## Verified Non-Interactions

The following systems have **zero interaction** with power (verified from binary):

- **Bridges** — purely cell/tile manipulation, no power references
- **Cloaked buildings** — cloaking is visual/detection only, power unaffected
- **Score screen** — no power statistics tracked
- **Allied power sharing** — does not exist; each house is completely independent
- **Network sync CRC** — power state is NOT in the sync hash (derived deterministically from building state which IS synced)

---

## Campaign Mode: Building Registration Flag (+0x41B)

In `AI_AssessPower` and `CheckPoweredBuildings`, a campaign-only check skips
buildings where `building+0x41B == 0` when `GameMode == 0` (single-player).
This is a "has been registered/activated" flag for preplaced campaign buildings.
Unactivated preplaced buildings do not contribute to power calculations.

---

## DamageDelay Fields — Confirmed Dead Code

`HouseClass+0x578C` and `+0x5794` (DamageDelay timer fields) are written ONLY
in `HouseClass::Constructor` (0x4F54A0). Byte-pattern search confirms **zero
reads** anywhere in the entire binary. These are vestigial with no functional
impact. Our Rust implementation's `degradation_accum_ms` should be removed.

---

## Discrepancies with Rust Implementation

Comparing gamemd.exe behavior with the actual codebase (last audited 2026-03-21).

### Bugs — Actively Wrong Behavior

#### 1. DamageDelay Degradation — RESOLVED, no longer a discrepancy

(corrected 2026-07-18: this section was stale. `src/sim/power_system.rs` has no
`apply_degradation_damage` function and no `degradation_accum_ms` field — grep of `src/`
confirms zero matches. `tick_power_states`'s doc comment (power_system.rs:141-145) and an
explicit regression test, `test_low_power_does_not_damage_buildings` (power_system.rs:534-540),
both pin the current Rust behavior to "no HP loss during low power," matching gamemd's vestigial
DamageDelay timer. No code change needed; this line item is closed.)

#### 2. Buildings Under Construction Excluded — Should Be Included
**Our code skips buildings with `building_up.is_some()`**, producing 0 power.
**The original INCLUDES them:** buildings under construction (mission 0x12) DO
contribute health-scaled output and full-rated drain. They are NOT operational
(can't fire, produce, etc.) but they DO affect the power balance.
**Severity: MEDIUM** — power balance is slightly wrong during early game building.

### Correct — No Changes Needed

- **Health-scaled output, full-rated drain** — matches original
- **Power plants never deactivated** — matches original (power > 0 → return true)
- **PowerOutput/PowerDrain single-field storage** — functionally equivalent
- **Binary power threshold** (`is_low_power = produced < drained`) — matches
  original's `PowerRatio() < 1.0` check in `IsOperational`
- **Spy blackout** — functionally equivalent (countdown vs start_frame+duration)
- **Production speed formula** — `production_tech.rs::owner_power_speed_multiplier_ppm()`
  already implements the exact continuous ratio formula:
  `speed = 1.0 - (1.0 - ratio) * LowPowerPenaltyModifier`, clamped to
  `[MinLowPowerProductionSpeed, MaxLowPowerProductionSpeed]`. Verified correct.
- **is_building_powered()** — correct for current scope. The checks it's "missing"
  (EMP, NeedsEngineer, upgrade bypass, PoweredSpecial) require features that don't
  exist in our sim yet. When those features are added, `is_building_powered` should
  be expanded into a full `is_operational` function.

### Future Features — Blocked on Prerequisites

These require sim features that don't exist yet. Document for when they're added:

| Discrepancy | Blocked On | Notes |
|-------------|-----------|-------|
| IsOperational full check (EMP, NeedsEngineer, mission state) | EMP system, engineer capture | Add checks when features exist |
| Upgrade bypass (UpgradeCount >= 2) | Building upgrade system | Bypasses both IsOnline and power check |
| Upgrade slot power contribution | Building upgrade system | GetPowerOutput iterates 3 upgrade slots |
| PoweredSpecial + HasOccupiedPowerPlant | Garrison system + superweapons | Anti-exploit: garrisoned reactor disables SWs |
| IsChargeDraining extra drain | GapGenerator charge system | +0x669 adds ExtraDrainBonus during charge |
| Super weapon pause/resume | Superweapon system | HandlePowerTransition suspends/resumes charging |
| ChargeMode double-check | Tesla Coil charge attack | Mission_Attack has second PowerRatio check |

### New Features — Can Implement Now

| Feature | Effort | Notes |
|---------|--------|-------|
| GoOnline/GoOffline toggle | Medium | `toggle_power` already parsed in ObjectType; needs per-building flag + command + guard logic |
| PoweredUnit= for vehicles | Small | Parse flag, check `power_surplus > 0` in movement code |
| Dirty-flag recalculation | Small | Performance optimization; set flag on build/destroy/capture, only recalc when set |

---

## Priority Fixes

1. ~~Remove degradation damage~~ — RESOLVED (corrected 2026-07-18: already absent from `src/sim/power_system.rs`, see Discrepancies §1 above)
2. **Include buildings under construction in power calc** — wrong power balance
3. **Add GoOnline/GoOffline per-building toggle** — feature already parsed, needs sim wiring
4. **Add PoweredUnit= for vehicles** — small, improves gameplay accuracy
5. **Add dirty-flag recalculation** — performance optimization

---

## Power Bar Rendering (PowerClass)

### Addresses

| Address | Name | Size | Purpose |
|---------|------|------|---------|
| `0x0063F7C0` | PowerClass::LoadGraphics | — | Loads POWERP.SHP |
| `0x0063F850` | PowerClass::CalcSegments | — | Calculates segment count from total power |
| `0x0063F960` | PowerClass::SplitPowerDisplay | — | Splits into drain/output/surplus portions |
| `0x0063FEA0` | PowerClass::AnimationTick | 1279 B | Per-frame bar animation |
| `0x0063FB20` | PowerClass::Draw | 664 B | Renders the power bar |
| `0x00640450` | PowerClass::Tooltip | — | Tooltip: "Power: drain/output" |

### POWERP.SHP Frames

| Frame | Color | Meaning |
|-------|-------|---------|
| 0 | Dark/empty | Unfilled segments (top of bar) |
| 1 | Green | Surplus power (top of filled area) |
| 2 | Yellow | Power output matching drain |
| 3 | Red | Power drain (bottom of bar) |
| 4 | Blink | Transition flash at empty/filled boundary |

### Segment Calculation

```c
total_segments = (bar_height_px + 3) / 3;
// bar_height is always a multiple of 50px

// Scale factor: inverse-proportional curve
// Bar is half-full when total_power = 400
filled = total_segments - ftol(total_segments * 400.0 / (total_power + 400.0));
// Always at least 1 filled segment
```

The segments represent the MAXIMUM power capacity (sum of all owned buildings'
theoretical power output and drain values), not the current operational values.

### Drain / Output / Surplus Split

The filled segments are divided into three color zones:
- **Drain (red):** Proportional to total power consumption
- **Output (yellow):** Power generation matching drain (capped at drain level)
- **Surplus (green):** Excess generation above drain

### Animation System

1. **Flash phase:** When power values change, a 10-count flash at 3-tick intervals
   (30 ticks total). Frame 4 (transition blink) drawn at the empty/filled boundary.
2. **Interpolation phase:** After flash, segments lerp ±1 per tick toward targets.
   Priority: surplus first, then drain, then output. Compensating adjustments
   maintain total segment count.
3. **Change detection:** Two cached power values compared each tick. Any change
   triggers a new flash sequence.

### Drawing

- Draws **top-to-bottom**: empty → blink → surplus (green) → output (yellow) → drain (red)
- Allied sidebar: X offset = 5px; Soviet/Yuri: X offset = 0px
- Y starts at sidebar_top + 69px (0x45), each segment = 3px height
- Uses `CC_Draw_Shape` with brightness 1000, flags 0x400, sidebar palette
- Tooltip (ID 999): `sprintf(buffer, TXT_POWER_DRAIN, drain_value, output_value)`
- No numeric values drawn on the bar itself — only in tooltip

---

## Building Animation Slot System (Power State Switching)

### 21 Animation Slots

Buildings have 21 animation slots. Each slot is 0x44 (68) bytes in
BuildingTypeClass and holds anim name strings + flags.

| Slot | TypeClass Offset | art.ini Key | Purpose |
|------|-----------------|-------------|---------|
| 0–2 | +0xF4C..+0xFD4 | PowerUp1/2/3Anim | Upgrade animations |
| 3–6 | +0x1018..+0x10E4 | ActiveAnim/Two/Three/Four | Active state anims |
| 7 | +0x1128 | PreProductionAnim | Pre-production |
| 8 | +0x116C | ProductionAnim | While producing |
| 9 | +0x11B0 | TurretAnim | Turret overlay |
| 10–13 | +0x11F4..+0x12C0 | SpecialAnim/Two/Three/Four | Special effect anims |
| 14–17 | +0x1304..+0x13D0 | SuperAnim/Two/Three/Four | Superweapon charge anims |
| 18 | +0x1414 | IdleAnim | Idle state |
| 19 | +0x1458 | LowPower | Shown during low power |
| 20 | +0x149C | SuperLowPower | Shown during low power (super variant) |

### Per-Slot Flags (4 bytes per slot at offset +0x40..+0x43 within slot)

Parsed from art.ini as `XXXPowered`, `XXXPoweredLight`, `XXXPoweredEffect`,
`XXXPoweredSpecial`:

| Flag | Offset | art.ini Suffix | Default | Meaning |
|------|--------|---------------|---------|---------|
| A | +0x40 | `XXXPowered` | 1 | Visibility toggle: destroy anim when entering this power state |
| B | +0x41 | `XXXPoweredLight` | 0 | Idle replacement: create alternative anim when entering state |
| C | +0x42 | `XXXPoweredEffect` | 0 | Charge latch: toggle +0x5B0 byte array flag per slot |
| D | +0x43 | `XXXPoweredSpecial` | 0 | (reserved/unused in most cases) |

### OnPowerOff (0x4545D0) — Entering Unpowered State

Iterates all 21 slots:
- **Flag A set:** Destroys the existing anim in that slot
- **Flag B set:** If slot is empty and building has `IsPoweredOn` flag (+0x6E4),
  creates a replacement anim (healthy or damaged variant based on `GetHealthRatio`
  vs `ConditionYellow` threshold at Rules+0x1700)
- **Flag C set:** Toggles charge flag in `+0x5B0` array, creates anim
- **Slot 20 always cleared** first
- **Slot 10 ↔ Slot 3 linking:** When SpecialAnim has `IsAnimDelayedFire` flag
  (+0x16A7), destroying slot 10 creates ActiveAnim in slot 3

### OnPowerOn (0x4547C0) — Entering Powered State

Iterates all 21 slots:
- **Flag C set:** Sets charge flag in +0x5B0, clears and recreates anim
- **Flag B set:** Destroys the idle replacement anim
- **Slot 16 ↔ Slot 20 linking:** SuperAnimThree charge flag creates SuperLowPower
  anim in slot 20
- Uses damage state to select healthy vs damaged anim variant

### State Change Detection (BuildingClass::Update at 0x43FB20)

Each tick: `IsOperational()` result compared to cached `+0x6C8`. On change,
`UpdateGapAndSpecialEffects` (0x4549B0) is called, which delegates to
`OnPowerOff` or `OnPowerOn` for the anim swap.

---

## Special Building Power Interactions

### Gap Generator

**Function:** `UpdateGapGenerator_Tick` (0x454DB0, 2076 bytes)

Uses a 4-state machine:
1. **Off** (state 0): Building unpowered or just placed
2. **Expanding** (state 1): Gap radius growing, counter increments 0→15
3. **Active** (state 2): Full gap coverage, maintaining shroud
4. **Contracting** (state 3): Power lost, gap shrinking, counter decrements 15→0

Does NOT check power independently — relies on `IsOperational()` via the
building update loop. When power drops, state transitions from Active→Contracting.
The 21 overlay cells get translucency updated each frame during transitions.
Gap radius comes from `GapRadiusInCells` in rules.ini.

### Cloak Generator

Uses the same `IsOperational()` check. Cloak state tracked at building+0x6EB.
When power is lost, `UpdateGapAndSpecialEffects` sets cloak state to 0xFF
(deactivating). The cloak radius contracts each tick. Nearby CloakGenerator
buildings coordinate to fill coverage gaps — if one goes offline, others
compensate by expanding.

### Laser Fence Posts

**Aggressive behavior:** When power drops, ALL laser fence beams are completely
**destroyed** (FUN_00472140), linked fence objects released (FUN_0070FEE0), and
fence lines go down. This is not a visual hide — the beam objects are deallocated.
When power returns, beams are rebuilt from scratch. Gated by `IsOperational()`.

### Psychic Sensor / Sensor Array

Uses `SensorArray=yes` flag (TypeClass+0x16C8) with `PsychicDetectionRadius`
(TypeClass+0x170C). Activated/deactivated through virtual calls at vtable+0x414
and vtable+0x418. Gated by `IsOperational()`.

### Super Weapons During Low Power

**Charging is PAUSED, not reset.** `SuperWeaponTypeClass->IsPowered` (offset 0xE6)
controls this. When entering low power:
- Remaining charge duration is saved
- `StartFrame` set to -1 (suspended)

When power restores:
- Charging picks up from where it left off

This is handled in `AI_ManageProduction` (0x50AF10), which checks the house
power ratio directly. Superweapons with `IsPowered=no` charge regardless of power.

### EVA Voice Lines

Two EVA systems for power events:

| Trigger | Function | Mechanism |
|---------|----------|-----------|
| Power drops | `FUN_00752700("EVA_LowPower", -1)` | String-based, from `HouseClass::Update` (0x4F8D0F) |
| SpySat shroud lost | `FUN_00750920` with Rules+0x220 | Index-based, from `CheckLowPower` (0x508F60) |
| SpySat shroud restored | `FUN_00750920` with Rules+0x224 | Index-based, from `CheckLowPower` |
| Spy infiltration | `"EVA_PowerSabotaged"` | String-based, from `OnSpyInfiltrate` (0x457309) |

EVA lines are only played for the **local player** (checked via `param_1 == DAT_00a83d4c`).

---

## AI_ManageProduction (0x50AF10) — Superweapon Pause/Resume

(was "HandlePowerTransition"; verified via `get_function_by_address 0x0050AF10`, 2026-05-20)

Called from `AI_AssessPower` when low-power state changes. Iterates all superweapons
owned by the house:

- **Entering low power:** For each `IsPowered=yes` superweapon, calls `SuperClass::Suspend(true)`
  at `0x6CB4D0`. Saves remaining charge time, sets `StartFrame = -1`. Sidebar tab invalidated.
- **Recovering from low power:** Calls `SuperClass::Suspend(false)`. Charging resumes from
  saved time. Sidebar tab invalidated.
- **Building destroyed / no producer:** Calls `SuperClass::Deactivate` at `0x6CB7B0`.
- **Does NOT** play EVA sounds (those come from `CheckPoweredBuildings`).
- **Does NOT** affect building animations (those happen via `IsOperational` per-tick checks).
- Sets `house+0x1FC = 1` to flag sidebar for full update.

---

## CheckLowPower (0x508F60) — SpySat Reveal/Restore Scan (NOT a low-power handler)

(was "CheckPoweredBuildings"; verified via `get_function_by_address 0x00508F60`, 2026-05-20)

**Corrected 2026-07-18:** Despite the Ghidra label `HouseClass__CheckLowPower`, this function has
**zero** references to `PowerOutput`/`PowerDrain`/`PoweredSpecial` anywhere in its body — it is scoped
exclusively to the `SpySat=yes` flag family and is unrelated to the house's power ratio
(decompile_function 0x00508F60 — INFERENCE_HARDENED). Called from `HouseClass::Update` when
`RecheckRadar` (+0x5779) is set. Drives the full-map-reveal shroud toggle for `SpySat` buildings
(TypeClass+0x16A5, NOT `PoweredSpecialShroud` — corrected 2026-07-18: binary INI reader at
`0x0045ff72` pushes string `"SpySat"` — decompile_function 0x0045FE50 — INFERENCE_HARDENED):

```
for each owned building with SpySat flag (TypeClass+0x16A5), alive, not selling, not IsWarpingOut:
  // first eligible building decides the outcome (list-order scan, breaks after first match/fail)
  if IsWarpingOut() (vtable+0x1D4) → break out of loop (no active SpySat found this tick)
  else (eligible building found, not warping):
    if SpySatActive (+0x577A) → return (already applied, no-op)
    MapClass::BlackoutShroud(0x00577D90) → reveals map (SpySatActive shroud toggle)
    SpySatActive = 1
    play EVA sound (SpySatActivationSound) if this == local player
    return

if loop exits (no eligible SpySat building found):
  if SpySatActive:
    MapClass::RestoreShroud(0x00577AB0) → restores normal shroud
    SpySatActive = 0
    play EVA sound (SpySatDeactivationSound) if this == local player
```

(corrected 2026-07-18: pseudocode rewritten from `decompile_function 0x00508F60`; prior version's
"if building has power → break (shroud should be OFF)" framing was wrong — the loop condition is
never a power check, it is the `SpySat` type flag plus alive/not-selling/not-warping-out —
INFERENCE_HARDENED)

---

## CheckSuperweaponReady (0x508DF0) — Ghidra label is a mislabel; function is Radar/Tactical-Map Availability

(was "UpdateRadarPowerState"; verified via `get_function_by_address 0x00508DF0`, 2026-05-20)

**Corrected 2026-07-18:** The Ghidra symbol name `HouseClass__CheckSuperweaponReady` does not match
the function's behavior — it never reads a `SuperWeapon`-type field or a charge/ready timer anywhere
in its body (decompile_function 0x00508DF0 — RTTI_LABEL_DRIFT). It is the local-player
tactical-map/radar-availability gate. Called after power recalc, only for the local player:

1. Check the `+0x2B0`(start)/`+0x2B8`(duration) spy radar blackout timer — a storage location
   **distinct** from the `+0x2A4/+0x2AC` `SpyPowerBlackout` pair used by `AI_AssessPower`/
   `IsOperational` (corrected 2026-07-18: these are two separate timers, not the same one —
   decompile_function 0x00508DF0 — OFFSET_RETYPED_WRONG). While active, skip straight to the
   compare step with the result left at its default `false` (radar forced unavailable).
2. If `ScenarioClass+0x34A4` (`FreeRadar=`, a map `[Basic]` INI key) is set → radar availability
   result is forced to `TRUE` unconditionally, skipping the power/building scan entirely (corrected
   2026-07-18: was "If SpySat active (`ScenarioClass+0x34A4`) → radar DISABLED"; binary
   `ScenarioClass::Read_INI_Basic` at `0x00689E90` reads this byte from the string `"FreeRadar"`
   (`s_FreeRadar_0083dff8`) in section `[Basic]`, and `0x00508DF0`'s `JNZ` on this byte jumps to
   the "available=true" branch, the OPPOSITE polarity of the prior claim, and the field has no
   relationship to `SpySat` — decompile_function 0x00689E90, decompile_function 0x00508DF0 —
   INFERENCE_HARDENED)
3. Otherwise, inline power ratio check: `drain <= output || drain == 0 || (double)output / (double)drain >= 1.0`
4. If power sufficient → scan buildings for the FIRST (list-order) one that is:
   - `Radar=yes` (+0x16A4), online (+0x660), alive (+0x74), not selling (mission != 0x13)
   - Then gated on `EMPLockRemaining == 0` (+0x504) and `!IsWarpingOut()` (vtable+0x1D4, reads
     +0x270 — corrected 2026-07-18: was "`HasPower` check (+0x270)"/"`IsCloaked`"; `read_memory
     0x007E4090` on the BuildingClass vtable resolves to `TechnoClass::IsWarpingOut` —
     decompile_function 0x0070C5B0 — INFERENCE_HARDENED). No `PsychicDetectionRadius` reference
     exists anywhere in the function (corrected 2026-07-18: removed the prior "Buildings with
     `PsychicDetectionRadius > 0` provide radar regardless of power" claim — not present in
     `decompile_function 0x00508DF0` — INFERENCE_HARDENED).
   - **First-match-only:** the loop `break`s after this first candidate regardless of whether it
     passes the EMP/warp gate — a second working `Radar=yes` building later in list order does
     NOT rescue availability if the first one is EMPed/warping (corrected 2026-07-18:
     OPERATOR_OR_ORDER_DRIFT).
5. Compares result against `RadarClass::IsTacticalMapAvailable` (0x00656DE0) and, if changed, calls
   `FUN_00656DF0`, which writes `RadarClass+0x14D8` and calls `RadarClass::ActivateDeactivate` /
   `RadarClass::SetRadarMode` (corrected 2026-07-18: was "`RadarClass::SetRadarDisabled` toggles
   minimap state and triggers radar animation" — no such function is called; `decompile_function
   0x00656DF0` shows the actual callees — INFERENCE_HARDENED).

---

## FactoryClass::RecalcAllRates (0x4CA6E0)

Iterates the **global** factory array, filters by house ownership:

```c
for each FactoryClass in global array:
  if factory->OwnerHouse != house: continue
  if factory->CurrentObject == NULL:
    speed = 0
  else:
    speed = GetProductionTime(factory->CurrentObject)
  step_delay = clamp(speed / 54, 1, 255)
  factory->StepDelay = step_delay
```

---

## Power= INI Parsing

A single `Power=` value is split into output and drain:

```c
if (Power >= 0):
    TypeClass->PowerOutput (+0xEE0) = Power
    TypeClass->PowerDrain  (+0xEE4) = 0
if (Power < 0):
    TypeClass->PowerOutput (+0xEE0) = 0
    TypeClass->PowerDrain  (+0xEE4) = -Power   // stored as positive
```

Same logic for `ExtraPower=` → +0xEE8 / +0xEEC. Verified at 0x461060.

---

## Power During Building Lifecycle

| State | Produces Power? | Drains Power? | IsOperational? |
|-------|----------------|---------------|----------------|
| In factory (limbo) | No | No | N/A |
| Under construction (mission 0x12) | Yes (health-scaled, low) | Yes (full rated) | No |
| Operational | Yes (health-scaled) | Yes (full rated) | Yes (if powered) |
| Being sold (mission 0x13) | Yes (health dropping) | Yes (full rated) | No |
| Manually offline (+0x660=0) | No | No | No |
| EMPed (+0x504 > 0) | **Yes** | **Yes** | **No** |
| Destroyed | Removed from house list | Removed | N/A |
| Captured | Transferred to new owner | Transferred | Recalc triggered on new owner |

**Key insight:** EMPed buildings still produce and drain power. EMP only affects
`IsOperational` (which gates weapons, production, etc.), NOT power contribution.
The EMP counter at +0x504 decrements each tick until it reaches 0.

---

## AI Power Management

- **AI prioritizes power plants** when in deficit. `AI_PickBuildItem` (0x4FE3E0)
  checks if placing a new building would cause `Drain > Output`. If so, it inserts
  a power plant build order. Plant selection per side: Allied (+0x89C), Soviet
  base/advanced (+0x8A0/+0x8A4), Yuri (+0x8A8) from RulesClass.
- **AI does NOT sell buildings** to reduce power deficit.
- **AI does NOT toggle buildings offline** (GoOffline). Only player commands do this.
- `BuildConst` buildings (e.g., Construction Yard) are exempt from the power budget check.

---

## Edge Cases (Verified)

1. **Powered=yes + positive Power=** → Building NEVER disabled by low power. The
   IsOperational check requires `Type->PowerDrain > 0` alongside `Powered=yes`.
   A power plant with Power=200, drain=0, and Powered=yes won't be affected.
   Prevents circular dependency.

2. **Iron Curtain** has NO power interaction. Iron Curtained buildings remain fully
   operational. No power checks reference Iron Curtain state.

3. **Chronoshift** cannot target buildings. The question of chronoshifted buildings
   and power is moot.

4. **No power crate** exists. The crate effect list has no power bonus.

5. **ForceShield** uses the **same blackout mechanism** as spy infiltration
   (`FUN_0050BC90`). It zeroes PowerOutput for its duration rather than adding drain.

---

## Companion Documents

Detailed deep-dives are in separate files:

- [POWER_BAR_RENDERING.md](POWER_BAR_RENDERING.md) — POWERP.SHP frames, segment
  calculation, flash/interpolation animation, drawing coordinates
- [SPECIAL_BUILDINGS_POWER_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPECIAL_BUILDINGS_POWER_SYSTEM.md) — Gap
  Generator state machine, Cloak Generator, Laser Fence, Psychic Sensor, super
  weapon pause/resume details
- [POWER_INI_PARSING_AND_LIFECYCLE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/POWER_INI_PARSING_AND_LIFECYCLE.md) — Power=
  parsing, ExtraPower=, building lifecycle (construction, sell, capture), mission IDs
- [POWER_EDGE_CASES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/POWER_EDGE_CASES.md) — AI power management, EMP, Iron
  Curtain, Chronoshift, crates, ForceShield, Powered=yes edge cases
