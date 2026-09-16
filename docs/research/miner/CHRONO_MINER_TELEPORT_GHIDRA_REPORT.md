# Chrono Miner / TeleportLocomotionClass -- Ghidra Research Report (v3)

## Overview

This report covers the complete TeleportLocomotionClass implementation in gamemd.exe,
including the 8-phase warp state machine, IPiggyback COM interface for locomotor switching,
chrono delay calculation, warp animation spawning, and Chronosphere superweapon interaction.

All addresses are from gamemd.exe (YR 1.001).

This is v3 of the report. v3 corrects the teleport trigger mechanism in Section 14 as
originally written below -- **that v3 framing is itself REFUTED as of 2026-07-19** (see the
correction boxes atop Sections 14 and 21): the function at 0x47EBA0 is `CellClass::FindFirstUnit`
(checks `WhatAmI()==1`), not `FindFirstBuilding`, and "destination cell has no unit" is
necessary but NOT sufficient for a warp -- the warp additionally requires Teleport's own
`HeadToCoord` to be invoked while Teleport is the ACTIVE locomotor. See
[docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) for the current
understanding and its open item.

---

## 1. TeleportLocomotionClass Struct Layout

**Constructor: 0x00718000**

From the constructor assembly, the struct is laid out as follows. The class inherits from
LocomotionClass (constructor at 0x0055A6C0).

### LocomotionClass Base (0x0055A6C0)
```
+0x00: IUnknown vtable ptr
+0x04: ILocomotion vtable ptr
+0x08: (dword) refcount / internal field = 0
+0x0C: (dword) LinkedTo FootClass* = 0
+0x10: (byte)  Powered = 1
+0x11: (byte)  field_11 = 1
+0x14: (dword) field_14 = 0
```

### TeleportLocomotionClass (extends LocomotionClass)

Vtable pointers overwritten by constructor:
```
+0x00: 0x7F50CC  (IUnknown vtable)
+0x04: 0x7F5000  (ILocomotion vtable)
+0x18: 0x7F4FDC  (IPiggyback vtable)
```

Constructor assembly at 0x718000 (verified instruction-by-instruction):
```asm
MOV [ESI+0x1C], g_NullCoord_X     ; HeadToCoord.X
MOV [ESI+0x20], g_NullCoord_Y     ; HeadToCoord.Y
MOV [ESI+0x24], g_NullCoord_Z     ; HeadToCoord.Z
MOV [ESI+0x28], g_NullCoord_X     ; DestCoord.X
MOV [ESI+0x2C], g_NullCoord_Y     ; DestCoord.Y
MOV [ESI+0x30], g_NullCoord_Z     ; DestCoord.Z
MOV byte [ESI+0x34], 0            ; IsMoving = false
MOV byte [ESI+0x35], 0            ; field_35 = 0
MOV byte [ESI+0x36], 0            ; field_36 = 0
MOV [ESI+0x38], 0                 ; WarpPhase = 0
MOV [ESI+0x3C], g_CurrentFrame    ; Timer.StartFrame
MOV [ESI+0x44], 0                 ; Timer.Duration = 0
MOV [ESI+0x48], 0                 ; field_48 = 0
```

### Complete Field Map

| Base Offset | Size | Field | Init | Notes |
|-------------|------|-------|------|-------|
| +0x00 | 4 | IUnknown_vtable | 0x7F50CC | |
| +0x04 | 4 | ILocomotion_vtable | 0x7F5000 | All ILocomotion calls go through this |
| +0x08 | 4 | Refcount | 0 | From LocomotionClass |
| +0x0C | 4 | LinkedTo (FootClass*) | 0 | The TechnoClass/FootClass this locomotor moves |
| +0x10 | 1 | Powered | 1 | From LocomotionClass |
| +0x11 | 1 | field_11 | 1 | From LocomotionClass |
| +0x14 | 4 | field_14 | 0 | From LocomotionClass |
| +0x18 | 4 | IPiggyback_vtable | 0x7F4FDC | |
| +0x1C | 4 | HeadToCoord.X | NullCoord | Where the unit is heading |
| +0x20 | 4 | HeadToCoord.Y | NullCoord | |
| +0x24 | 4 | HeadToCoord.Z | NullCoord | |
| +0x28 | 4 | DestCoord.X | NullCoord | Process destination |
| +0x2C | 4 | DestCoord.Y | NullCoord | |
| +0x30 | 4 | DestCoord.Z | NullCoord | |
| +0x34 | 1 | IsMoving | 0 | 1 while warp is in progress |
| +0x35 | 1 | field_35 | 0 | Checked by Is_Ok_To_End |
| +0x36 | 1 | field_36 | 0 | Unknown |
| +0x38 | 4 | WarpPhase | 0 | State machine phase (0-7) |
| +0x3C | 4 | Timer.StartFrame | CurrentFrame | CDTimerClass start frame |
| +0x40 | 4 | Timer.field_4 | (uninitialized) | Middle field of CDTimerClass, not used in countdown |
| +0x44 | 4 | Timer.Duration | 0 | CDTimerClass countdown duration in frames |
| +0x48 | 4 | field_48 | 0 | Unknown |

**Total struct size: ~0x4C bytes (76 bytes)**

**NOTE on calling conventions:** When functions are called through the ILocomotion vtable,
the `this` pointer points to base+0x04 (the ILocomotion vtable). So `[this+0x08]` in the
decompilation actually refers to base+0x0C = LinkedTo. Similarly, `[this+0x34]` = base+0x38
= WarpPhase, `[this+0x30]` = base+0x34 = IsMoving, etc.

When called through IPiggyback vtable, `this` = base+0x18. So `[this+0x30]` = base+0x48.
The IPiggyback stores a piggybacked locomotor at IPiggyback_this+0x30 (= base+0x48), but
wait -- looking at Begin_Piggyback(0x719E90), it checks `[param_1+0x30]` which from the
IPiggyback this (base+0x18) would be base+0x48. But the constructor only inits up to 0x48.
So the piggyback locomotor pointer is stored at what was called `field_48` -- it doubles
as the IPiggyback stored locomotor.

Actually, re-examining: In the constructor, `field_48 = 0` and Begin_Piggyback stores
the piggybacked locomotor at IPiggyback_this+0x30. Since IPiggyback vtable is at base+0x18,
that's base+0x18+0x30 = base+0x48. So field_48 IS the PiggybackedLocomotor pointer.

**Confidence: 95%** -- Verified from constructor assembly and state machine cross-references.

---

## 2. CDTimerClass Pattern (Timer Triple at +0x3C/+0x40/+0x44)

The timer at locomotor offsets +0x3C/+0x40/+0x44 follows the CDTimerClass pattern used
throughout gamemd.exe:

```
struct CDTimerClass {
    int StartFrame;   // +0x00: Frame when timer was set (or -1 for expired/invalid)
    int field_4;      // +0x04: Middle field, NOT used in countdown logic
    int Duration;     // +0x08: Duration in frames
};
```

### Countdown Check (from 0x719BF0 and 0x7194FD-0x7194F7):
```c
int remaining = timer.Duration;
if (timer.StartFrame != -1) {
    int elapsed = g_CurrentFrameCounter - timer.StartFrame;
    if (elapsed >= timer.Duration) {
        remaining = 0;  // timer expired
    } else {
        remaining = timer.Duration - elapsed;
    }
}
// Timer has expired when remaining == 0
```

### Timer Set (from Phase 0 at 0x7194BE):
```c
timer.StartFrame = g_CurrentFrameCounter;
timer.field_4 = <value from stack>;  // uninitialized / carried from prior state
timer.Duration = <calculated chrono delay>;
```

The middle field (+0x40) is written when the timer is set but NEVER read during the
countdown check. It appears to be vestigial padding in CDTimerClass.

**Confidence: 95%** -- Verified from assembly at 0x7194FD (phase 0 timer check) and
0x719BF0 (phase 1/6 timer expiry function).

---

## 3. The State Machine (0x007192F0) -- Complete Decompilation

**ILocomotion vtable slot 16 (offset 0x40)**. This is the main state machine function.
Called every tick via virtual dispatch from the game loop.

Parameter: `this` = ILocomotion interface pointer (base+0x04).
Notation: `techno` = LinkedTo TechnoClass* at this+0x08 (= base+0x0C).

### Pre-Phase Checks (before the switch)

```c
// 1. If techno is BeingWarped (+0x271), phase==0, and techno->PendingWarpPhase (+0x280)==0:
//    Call End_Piggyback timer check and return. Unit is idle-warped.
if (techno->BeingWarped && phase == 0 && techno->PendingWarpPhase == 0) {
    return this->IUnknown_vtable->TimerCheck(this);
}

// 2. If phase==0 and techno->PendingWarpPhase != 0:
//    Pick up externally-set phase (from ChronoSphere superweapon)
if (phase == 0 && techno->PendingWarpPhase != 0) {
    this->WarpPhase = techno->PendingWarpPhase;
    return;
}

// 3. If techno->ChronoInTransit (+0x27C, accessed as byte of [techno+0x9F*4]) != 0:
//    Jump to the phase-specific Chrono-in-transit handler (see below)
if (techno->ChronoInTransit != 0 && phase == 0) {
    // Phase 0 special: chrono-in-transit initialization
    techno->WarpingOut (+0x270) = 1;
    timer.StartFrame = g_CurrentFrameCounter;
    timer.Duration = 0x3C;  // 60 frames fixed delay
    WarpPhase++;  // -> phase 1
    return;
}
```

### Phase 0: WARP_START (Self-Teleport Initiation)

This phase handles the chrono legionnaire / chrono miner self-teleport. Only entered
when `ChronoInTransit == 0` and `WarpPhase < 1` and `Is_Moving()` returns true.

```c
// Check if current position != destination (both not NullCoord)
CoordStruct currentPos = techno->Location;   // techno+0x9C/0xA0/0xA4
CoordStruct destCoord = this->DestCoord;     // this+0x24/0x28/0x2C (= base+0x28-0x30)

if (currentPos == destCoord || destCoord == NullCoord) {
    // No movement needed: call SetOccupation(1, 0) and End_Piggyback
    techno->vtable->SetOccupation(1, 0);   // vtable+0x480
    this->IUnknown_vtable->TimerCheck(this);
    return;
}

// === BEGIN WARP SEQUENCE ===

// 1. Stop all units targeting this one
FUN_0070D4A0(techno);  // Clears all targeting/pursuit of this unit

// 2. Detach all anim effects linked to this unit
for (each anim in g_AnimArray where anim->OwnerTechno == techno) {
    FUN_00468430(anim);  // Detach anim
}

// 3. Spawn WarpOut anim at unit's current location
AnimClass::Constructor(
    Rules->WarpAway,        // Rules+0x33C (AnimType*)
    &techno->Location,      // position
    0,                      // delay
    1,                      // loop count
    0x600,                  // flags (AnimFlag_600)
    0, 0                    // owner, etc.
);

// 4. Calculate Euclidean distance
int dx = techno->GetCoord()->X - destCoord.X;
int dy = techno->GetCoord()->Y - destCoord.Y;
int dz = techno->GetCoord()->Z - destCoord.Z;
double distSq = (double)dx*dx + (double)dy*dy + (double)dz*dz;
int distance = (int)sqrt(distSq);  // sqrt at 0x4CAC40, ftol at 0x7C5F00

// 5. Calculate chrono delay (timer duration)
timer.StartFrame = g_CurrentFrameCounter;
timer.Duration = 0;  // default: no delay

if (Rules->ChronoTrigger) {                          // Rules+0xBF8 (bool)
    timer.Duration = distance / Rules->ChronoDistanceFactor;  // Rules+0xBF4 (int)
}

// 6. Clamp timer: compute remaining time
int remaining = timer.Duration;
if (timer.StartFrame != -1) {
    int elapsed = g_CurrentFrameCounter - timer.StartFrame;
    remaining = (elapsed >= timer.Duration) ? 0 : timer.Duration - elapsed;
}

// 7. If remaining <= ChronoMinimumDelay, use minimum
if (remaining <= Rules->ChronoMinimumDelay) {        // Rules+0xBFC (int)
    timer.StartFrame = g_CurrentFrameCounter;
    timer.Duration = Rules->ChronoMinimumDelay;
}

// 8. If distance < ChronoRangeMinimum, force minimum delay
if (distance < Rules->ChronoRangeMinimum) {          // Rules+0xC00 (int)
    timer.StartFrame = g_CurrentFrameCounter;
    timer.Duration = Rules->ChronoMinimumDelay;
}

// 9. Set BeingWarped flag
techno->BeingWarped (+0x271) = 1;

// 10. Infantry chrono-kill check
int abstractType = techno->vtable->WhatAmI();  // vtable+0x2C
if (abstractType == 1  /* InfantryClass */
    // [corrected 2026-07-18: decompile_function 0x7192F0 reads this in ONE dereference,
    //  `*(char*)(*(int*)(techno+0x6c4) + 0xe0e)` -- techno->OwnerHouseType (+0x6C4, see
    //  Section 12) directly, not a two-hop techno->OwnerHouse->TypeClass chain.]
    && techno->OwnerHouseType->ChronoKillInfantry (+0xE0E)) {
    // Infantry killed by chrono: zero the timer and clear warp
    timer.StartFrame = g_CurrentFrameCounter;
    timer.Duration = 0;
    techno->BeingWarped = 0;
}

// 11. Detach flash anim if present
if (techno->FlashAnim (+0x694) != NULL) {
    FUN_0062A4A0(techno->FlashAnim->field_69C);  // Complex flash detach
}

// 12. Unmark from map, then mark at destination
techno->vtable->Unmark(0);                      // vtable+0x124, param=0

// 13. Play ChronoOutSound
TypeClass* type = techno->vtable->GetType();     // vtable+0x84
if (type->ChronoOutSound (+0x578) != -1 || Rules->ChronoOutSound (+0x21C) != -1) {
    FUN_007509E0(sound_index, 0, &techno->Location);
}

// 14. Set destination on TechnoClass, update bridge flag
techno->vtable->SetDestination(destCoord);       // vtable+0x1B4
CellClass* destCell = MapClass::GetCellAt(destCoord);
if (destCell->Flags (+0x140) & 0x100) {          // bridge flag
    techno->IsOnBridge (+0x8C) = 1;
} else {
    techno->IsOnBridge = 0;
}

// 15. Clear destination, mark at new position
techno->vtable->ClearDestination(0);             // vtable+0x1CC
techno->vtable->Mark(1);                         // vtable+0x124 with param=1

// 16. Play ChronoInSound at destination
if (type->ChronoInSound (+0x574) != -1 || Rules->ChronoInSound (+0x218) != -1) {
    FUN_007509E0(sound_index, 0, &destCoord);
}

// 17. Set mission to GUARD_AREA (2)
techno->vtable->SetMission(2);                   // vtable+0x18C

// 18. Stop_Moving / Clear_Coords -- NOT "UpdateLayer"
// [corrected 2026-07-18: vtable+0x48 resolves to TeleportLocomotionClass::Stop_Moving
//  (0x718230), verified by decompile_function 0x718230 (plate comment: PROOFED confidence
//  92, chronominer-locomotion/fn-accessors.md). It clears HeadToCoord to NullCoord and sets
//  IsMoving=0 (base+0x34) and field_36=0 (base+0x36). There is no "UpdateLayer" method on
//  this vtable -- Section 19's own vtable table (independently read via read_memory
//  0x7F5000, 76 bytes) already lists slot 0x48 as Stop_Moving; this Phase-0 narrative had
//  not been cross-checked against it.]
this->ILocomotion_vtable->Stop_Moving(this);     // vtable+0x48 (0x718230)

// 19. Handle crate pickup at destination
FUN_00481A00(destCell, techno);

// 20. Call SetOccupation(0, 1)
techno->vtable->SetOccupation(0, 1);             // vtable+0x480

// 21. Spawn WarpIn anim at unit location (same WarpAway type!)
AnimClass::Constructor(
    Rules->WarpAway,        // Rules+0x33C
    &techno->Location,
    0, 1, 0x600, 0, 0
);

// 22. Clear PendingWarpPhase
techno->PendingWarpPhase (+0x280) = 0;

// [corrected 2026-07-18: the claim "IsMoving is now 1" is WRONG. Step 18 (Stop_Moving,
//  called earlier in this same Phase-0 body) already reset IsMoving to 0 and HeadToCoord
//  to NullCoord -- verified by decompile_function 0x7192F0 (the raw Phase-0 body calls
//  `(**(code**)(*param_1 + 0x48))(param_1)` before this point, where *param_1 is the
//  ILocomotion vtable 0x7F5000 and slot 0x48 = 0x718230 = Stop_Moving) and
//  decompile_function 0x718230 directly. Consequently WarpPhase does NOT advance to 1 via
//  TimerCheck either: TimerCheck (0x719BF0) only does `if (WarpPhase > 0) WarpPhase++`, and
//  WarpPhase is never incremented anywhere in the Phase-0 (ChronoInTransit==0) path, so it
//  stays parked at 0 for self-teleport. "Phase 1" (WARP_OUT_WAIT) is only a real WarpPhase
//  value for the OTHER entry path (ChronoInTransit!=0, e.g. ChronoSphere), which has its own
//  explicit `WarpPhase++` in the pre-phase check. For self-teleport, the translucency wait
//  is implemented entirely by the pre-phase check (BeingWarped==1 && WarpPhase==0 &&
//  PendingWarpPhase==0 -> call TimerCheck every tick) while WarpPhase remains 0 throughout --
//  this matches Section 21's later "Definitive" call-chain description, which does NOT
//  claim WarpPhase reaches 1 for self-teleport. The two sections were never reconciled.]
// Phase remains 0. IsMoving is 0 again by the end of this tick. BeingWarped stays 1 until
// TimerCheck's countdown (armed in steps 5-8) expires on a later tick.
return;
```

### Phase 1: WARP_OUT_WAIT

```c
// Call IUnknown vtable[0x28] = FUN_00719BF0 (timer expiry check)
// This function:
//   1. Checks if timer (StartFrame/Duration) has expired
//   2. If expired: sets techno->BeingWarped (+0x271) = 0
//   3. If expired AND WarpPhase > 0: WarpPhase++
// So phase 1 simply waits for the warp-out timer to expire, then advances to phase 2.
this->IUnknown_vtable->TimerCheck(this);
```

**Timer expiry function (0x719BF0) -- full decompilation:**
```c
void TimerCheck(LocomotionBase* this) {
    int remaining = this->Timer.Duration;        // base+0x44
    if (this->Timer.StartFrame != -1) {          // base+0x3C
        int elapsed = g_CurrentFrameCounter - this->Timer.StartFrame;
        if (elapsed >= remaining) {
            goto expired;
        }
        remaining -= elapsed;
    }
    if (remaining != 0) return;  // still counting down

expired:
    // Timer expired!
    LinkedTo->BeingWarped (+0x271) = 0;

    // Check garrison: if unit has garrison (+0x2B4 != 0),
    // call FUN_0070F770 and check FUN_00709480
    if (LinkedTo->Garrison (+0x2B4) == 0) {
        FUN_0070F770();
        if (!FUN_00709480()) {
            LinkedTo->vtable->SetOccupation(0, 1);
        }
    }

    // Advance phase if > 0
    if (this->WarpPhase > 0) {
        this->WarpPhase++;
    }
}
```

### Phase 2: IN_TRANSIT_START

```c
// Spawn WarpAway anim at unit's current location (warp-in sparkle at source)
AnimClass::Constructor(Rules->WarpAway, &techno->Location, 0, 1, 0x600, 0, 0);

// Unmark from current cell
techno->vtable->Unmark(0);                   // vtable+0x124

// Play ChronoOutSound (same logic as Phase 0)
if (type->ChronoOutSound != -1 || Rules->ChronoOutSound != -1) {
    FUN_007509E0(...);
}

// Set visual flags on TechnoClass
techno->BeingWarped (+0x271) = 1;
techno->ChronoInTransit (+0x27C) = 0;
techno->WarpingOut (+0x270) = 0;
techno->IsOnBridge (+0x8C) = 0;

// Read the externally-set destination from TechnoClass
CoordStruct chronoDest = {
    techno->ChronoDestCoord_X (+0x288),
    techno->ChronoDestCoord_Y (+0x28C),
    techno->ChronoDestCoord_Z (+0x290)
};

// Call Update_Position to teleport the unit
bool arrived = TeleportLocomotionClass::Update_Position(this, chronoDest, 0);

// Advance phase
WarpPhase++;         // -> 3
if (arrived) {
    WarpPhase++;     // -> 4 (skip phase 3 if already at dest)
}
```

### Phase 3: IN_TRANSIT_CONTINUE

```c
// Continue moving toward destination
CoordStruct chronoDest = { techno+0x288, +0x28C, +0x290 };
bool arrived = Update_Position(this, chronoDest, 0);

if (arrived) {
    WarpPhase++;  // -> 4
}

// Store ChronoDelay into TechnoClass for later use
techno->ChronoLockDuration (+0x284) = Rules->ChronoDelay;  // Rules+0xBEC
```

### Phase 4: WARP_IN_RELOCATE

```c
// Final position update with flag=1 (apply occupancy)
CoordStruct chronoDest = { techno+0x288, +0x28C, +0x290 };
Update_Position(this, chronoDest, 1);  // param_5 = 1 means "apply"

// Update map marking
techno->vtable->SetDestination(destCoord);   // vtable+0x1B4
techno->vtable->ClearDestination(0);         // vtable+0x1CC
techno->vtable->Mark(1);                     // vtable+0x124

// Advance phase
WarpPhase++;  // -> 5
```

### Phase 5: WARP_IN_COMPLETE

```c
// Final destination marking
techno->vtable->SetDestination(destCoord);   // vtable+0x1B4
techno->vtable->ClearDestination(0);         // vtable+0x1CC
techno->vtable->Mark(1);                     // vtable+0x124 with param=1

// Play ChronoInSound
if (type->ChronoInSound (+0x574) != -1 || Rules->ChronoInSound (+0x218) != -1) {
    FUN_007509E0(sound_index, 0, &techno->Location);
}

// Check if destination cell is in playfield
CellStruct destCell = techno->vtable->GetMapCoords(1);
if (!MapClass::Is_Cell_In_Playfield(destCell)) {
    techno->field_3D5 = 0;  // clear "discovered" flag
}

// Post-warp validation (only if NOT externally-warped, i.e., PendingWarpPhase == 0)
if (techno->PendingWarpPhase (+0x280) == 0) {
    FUN_007187A0(this, destCoord);  // validate destination, handle water/occupied
}

// Check if unit is alive after validation
if (!techno->IsAlive (+0x90)) {
    return;  // unit was killed by validation (water death, etc.)
}

// === Unit survived! Complete the warp ===

// Set mission to GUARD_AREA
techno->vtable->SetMission(2);               // vtable+0x18C

// Stop_Moving / Clear_Coords -- NOT "UpdateLayer"
// [corrected 2026-07-18: same mislabel as Phase 0 step 18 -- vtable+0x48 is
//  TeleportLocomotionClass::Stop_Moving (0x718230), verified by decompile_function
//  0x718230. Clears HeadToCoord to NullCoord, IsMoving=0, field_36=0.]
this->ILocomotion_vtable->Stop_Moving(this); // vtable+0x48 (0x718230)

// Clear chrono source references
techno->ChronoSourceBuilding (+0x428) = 0;
techno->ChronoSourceHouse (+0x42C) = 0;

// Clear visual warp state
FUN_0070C610(techno, 0);  // sets techno+0x218 = 0

// Update occupation
techno->vtable->SetOccupation(0, 1);         // vtable+0x480

// Set warp-in timer from stored ChronoLockDuration
int lockDuration = techno->ChronoLockDuration (+0x284);
timer.StartFrame = g_CurrentFrameCounter;
timer.Duration = lockDuration;

// Spawn WarpIn anim
AnimClass::Constructor(Rules->WarpAway, &techno->Location, 0, 1, 0x600, 0, 0);

// Advance phase
WarpPhase++;  // -> 6
```

### Phase 6: CHRONO_LOCK_WAIT

```c
// Same as Phase 1: call timer expiry check
// Wait for the chrono lock timer (set in Phase 5) to expire
// When it expires, WarpPhase advances to 7
this->IUnknown_vtable->TimerCheck(this);
```

### Phase 7: WARP_DONE (Reset)

```c
// Clear all warp flags
techno->BeingWarped (+0x271) = 0;

// Clear visual warp state
FUN_0070C610(techno, 0);                     // sets techno+0x218 = 0

// Final occupation update
techno->vtable->SetOccupation(0, 1);         // vtable+0x480

// Reset locomotor state
this->IsMoving = 0;                          // base+0x34
techno->PendingWarpPhase (+0x280) = 0;
this->WarpPhase = 0;                         // back to idle

// State machine returns to idle. The piggybacked locomotor
// will be swapped back via Is_Ok_To_End -> End_Piggyback
// on the next tick when all conditions are met.
```

**Confidence: 95%** -- Every branch verified from both decompilation and assembly.

---

## 4. Chrono Delay Calculation (Phase 0 Detail)

The chrono delay determines how long the warp-out/warp-in visual effect lasts.

### RulesClass Fields

| Rules Offset | INI Key | Type | Purpose |
|-------------|---------|------|---------|
| +0xBEC | ChronoDelay | int | Post-warp lock duration (stored into techno+0x284 in Phase 3) |
| +0xBF0 | ChronoReinfDelay | int | Delay for ChronoSphere reinforcement warp |
| +0xBF4 | ChronoDistanceFactor | int | Divisor: delay = distance / factor |
| +0xBF8 | ChronoTrigger | bool | If true, compute distance-based delay |
| +0xBFC | ChronoMinimumDelay | int | Floor for warp timer duration |
| +0xC00 | ChronoRangeMinimum | int | If distance < this value, force minimum delay |

### Algorithm (from assembly at 0x7194AC-0x719573)

```
Step 1: distance = (int)sqrt((double)(dx*dx + dy*dy + dz*dz))
        where dx,dy,dz are differences between current position and destination in leptons

Step 2: Initialize timer
        timer.StartFrame = g_CurrentFrameCounter
        timer.Duration = 0

Step 3: If ChronoTrigger (Rules+0xBF8):
        timer.Duration = distance / ChronoDistanceFactor (Rules+0xBF4)
        (integer division via IDIV instruction at 0x7194E3)

Step 4: Compute remaining time (same formula as countdown check):
        elapsed = CurrentFrame - timer.StartFrame
        remaining = max(0, timer.Duration - elapsed)
        Note: Since StartFrame was JUST set to CurrentFrame, elapsed=0,
        so remaining = timer.Duration. This step is a no-op on initial set
        but exists because the code reuses the CDTimerClass check pattern.

Step 5: Clamp to minimum:
        if (remaining <= ChronoMinimumDelay (Rules+0xBFC)):
            timer = { CurrentFrame, ?, ChronoMinimumDelay }

Step 6: Force minimum for short distances:
        if (distance < ChronoRangeMinimum (Rules+0xC00)):
            timer = { CurrentFrame, ?, ChronoMinimumDelay }
```

### WarpFactor Ramp -- CORRECTED UNDERSTANDING

**There is no floating-point WarpFactor ramp in the TeleportLocomotionClass.**

Previous reports suggested TechnoClass+0x244 was a float "WarpFactor" that ramped 0.0->1.0.
This is INCORRECT. The actual mechanism is:

1. TechnoClass+0x280 (`PendingWarpPhase`) is an INTEGER that stores the phase to jump to.
   - Set to 0 by the state machine when warp completes
   - Set to 3 by the ChronoSphere superweapon to start units at Phase 3
   - The state machine reads it at startup and copies to WarpPhase

2. The visual warp effect (alpha fade) is driven by the TIMER at locomotor +0x3C/+0x44:
   - The rendering code reads TechnoClass+0x271 (BeingWarped flag)
   - If set, the unit is drawn with a warp visual effect
   - The alpha/intensity is derived from `remaining = timer.Duration - (CurrentFrame - timer.StartFrame)`
   - This produces a linear ramp from full effect (when timer starts) to zero (when timer expires)

3. TechnoClass+0x428 and +0x42C store pointers to the chrono source building and house
   for the "return to sender" mechanic (ChronoSphere only). Cleared by End_Piggyback
   and in Phase 5.

**Confidence: 95%** -- Verified by searching all writes to +0x280 across the entire binary.
Only 4 locations write it: state machine phases 0/7 (clear to 0), ChronoSphere handler
(set to 3), and the init function at 0x720440 (clear to 0).

---

## 5. Process Function (0x718B70) -- Full Decompilation

> **Naming caution (confirmed 2026-07-19):** despite the Ghidra function name
> `TeleportLocomotionClass__Process`, this is NOT the per-tick `ILocomotion::Process` vtable
> slot. `get_function_by_address 0x00718b70` confirms its body is 00718b70-007192bd, ending
> exactly where `TeleportLocomotionClass__StateMachineTick` (0x007192F0, the real per-tick
> `ILocomotion::Process` at vtable+0x40 -- see Section 19) begins. Per
> [CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) §1.2, this function's only caller is
> `HeadToCoord` (0x718100) via a direct call, and it is absent from the ILocomotion vtable
> entirely. Do not cite 0x718B70 as "the per-tick Process."

**Called from:** FUN_00718100 (ILocomotion vtable slot 17, "Head_To_Coord")

This function handles the initial movement request and infantry placement logic.
It does NOT contain the state machine -- it prepares the destination and validates
movement, then the state machine at 0x7192F0 handles the tick-by-tick warp.

### Relationship to State Machine

```
Movement Order (player click / Mission_Harvest / etc.)
  -> TechnoClass::Set_Destination (0x741970)
     -> Teleporter block (0x7423CD): checks dest cell for buildings
        -> If dest cell EMPTY + CLSID is TeleportLoco: NO piggyback (teleport path)
        -> If dest cell HAS building OR CLSID is already Drive: Drive piggyback (drive path)
     -> FootClass::Set_Destination_Internal (0x4D94B0)
        -> Stores NavCom target at FootClass+0x5A4
        -> Calls ILocomotion::Head_To_Coord (vtable+0x44) on ACTIVE locomotor
           -> If TeleportLocomotionClass (0x718100):
              -> Calls Process (0x718B70) to validate destination
              -> Sets IsMoving=1 and HeadToCoord=DestCoord
           -> If DriveLocomotionClass:
              -> Drive handles normal pathfinding

Game Loop (every tick)
  -> FootClass::AI (0x4DA530)
     -> ILocomotion::Process (vtable+0x40)
        -> If TeleportLoco active: StateMachineTick (0x7192F0)
           -> Phase 0: Is_Moving()? -> Initiates warp
        -> If Drive active: normal drive movement
     -> IPiggyback::Is_Ok_To_End check
        -> If Drive stopped + has piggyback: swaps back to Teleport
```

### Process Function Logic (0x718B70)

```c
bool Process(TeleportLoco* this, CoordStruct destCoord) {
    // Determine the effective destination
    CoordStruct* dest;
    if (this->DestCoord == NullCoord) {
        dest = &techno->Location;  // use current position
    } else {
        dest = &this->DestCoord;
    }

    // Set destination on TechnoClass
    techno->vtable->SetCoord(dest);              // vtable+0xF4

    // If resolved dest is NullCoord: set to current position, return 0
    if (resolvedDest == NullCoord) {
        this->DestCoord = resolvedDest;
        goto finalize;
    }

    // Check if destination cell has a bridge
    CellClass* destCell = CellClass::Get_Cell_At(&resolvedDest);
    bool isOnBridge;
    if (!(destCell->Flags & 0x100)) {
        isOnBridge = false;
    } else {
        // Check if unit Z is above bridge level
        int groundHeight = CellClass::GetGroundHeight(&resolvedDest);
        isOnBridge = (techno->Location.Z > groundHeight + g_BridgeZOffset * 3);
    }

    // Infantry special handling
    int abstractType = techno->vtable->WhatAmI();
    if (abstractType == 0xF) {  // InfantryClass
        // Handle infantry already in the cell (check subcell availability)
        // ... complex infantry placement logic ...
        // Check building occupancy (types 1, 2, 6) at destination
        // Use Pathfinding_validate_alternate for valid cell search
        CellClass::PlaceInfantryInCell(...)
        // Check if resulting cell allows movement
        techno->vtable->CanEnterCell(cell, -1, -1, 0, 1);
        // If blocked, set dest to NullCoord
    }
    // Non-infantry: regular pathfinding validation
    else if (techno->ChronoInTransit (+0x27C) == 0) {
        // Validate destination is passable
        TypeClass* type = techno->vtable->GetType();
        int speedType = type->SpeedType (+0x5B4);
        // Check cell passability with zone validation
        // Use Pathfinding_validate_alternate if needed
        // Snap destination to cell center
        this->DestCoord.X = cellCoord.X * 256 + 128;
        this->DestCoord.Y = cellCoord.Y * 256 + 128;
        this->DestCoord.Z = GetGroundHeight(dest);
        techno->vtable->SetCoord(dest);
    }
    // ChronoInTransit path: just use the destination as-is
    else {
        techno->vtable->SetCoord(dest);
    }

finalize:
    // If dest is still NullCoord, use unit's current location
    if (this->DestCoord == NullCoord) {
        techno->vtable->SetCoord(&techno->Location);
        return 0;  // no movement
    }
    techno->vtable->SetCoord(&this->DestCoord);
    return 1;  // movement initiated
}
```

**Confidence: 80%** -- Complex function with heavy stack manipulation and infantry
placement logic. Core flow is verified but some details in the infantry path may
have decompilation artifacts.

---

## 6. Update_Position (0x718260) -- Full Decompilation

Called in Phases 2, 3, 4 of the state machine to move the unit toward the warp destination.

### Parameters
```c
bool Update_Position(
    TeleportLoco* this,      // ECX (thiscall)
    CoordStruct dest,        // on stack (X, Y, Z)
    bool applyOccupancy      // param_5: if true, check cell passability and do zone validation
);
```

### Logic

```c
bool Update_Position(this, dest, applyOccupancy) {
    if (!applyOccupancy) {
        // === Mode 0: Simple teleport ===

        // Check destination cell for objects to damage
        CellClass* destCell = CellClass::Get_Cell_At(&dest);
        ObjectClass* obj;

        if (destCell->Flags & 0x100) {  // bridge cell
            obj = destCell->BridgeObjects (+0xE8);  // objects on bridge
        } else {
            obj = destCell->FirstObject (+0xE4);    // ground objects
        }

        // Iterate objects at destination
        for (; obj != NULL; obj = obj->NextObject (+0x30)) {
            if (obj->IsInAir()) {
                // Flying unit at destination: skip other checks
                // Deal damage to this unit using ChronoWarpDamagePercent
                TypeClass* myType = techno->vtable->GetType();
                int damage = myType->Strength (+0xA0);
                techno->vtable->TakeDamage(
                    &damage, 0, Rules->C4Warhead (+0xFA8)  /* corrected 2026-05-19: stock gamemd reads INI key "C4Warhead" (section [CombatDamage]) into Rules+0xFA8; prior label "ChronoWarpDamagePercent" is fictional */, 0, 1, 0, 0);
            }
            else if (!obj->IsInAir() && obj->WhatAmI() == 0xF  // infantry
                     && techno->WhatAmI() == 0xF) {
                // Both are infantry at same subcell: deal chrono damage to THEM
                TypeClass* theirType = obj->vtable->GetType();
                int damage = theirType->Strength;
                obj->vtable->TakeDamage(
                    &damage, 0, Rules->C4Warhead, 0, 1, 0, 0);
            }
            else if (!obj->IsInAir() && obj->IsTechno) {
                // Techno at destination: deal chrono damage to them
                TypeClass* theirType = obj->vtable->GetType();
                int damage = theirType->Strength;
                obj->vtable->TakeDamage(
                    &damage, 0, Rules->C4Warhead, 0, 1, 0, 0);
            }
        }

        // Check for water cell on bridge
        if ((destCell->Flags & 0x100) && !(destCell->Flags & 0x200)) {
            applyOccupancy = true;  // force occupancy check
        }

        // Snap to cell center
        CellStruct myCell = CellStruct::FromCoord(techno->GetCoord());
        // ... cell validation logic ...

    } else {
        // === Mode 1: Teleport with relative offset ===

        // Determine current effective position
        CoordStruct* curPos;
        if (this->DestCoord == NullCoord) {
            curPos = &techno->Location;
        } else {
            curPos = &this->DestCoord;
        }
        techno->vtable->SetCoord(curPos);

        // Apply ChronoDestCoord offset
        this->DestCoord.X = techno->ChronoDestCoord_X (+0x288);
        this->DestCoord.Y = techno->ChronoDestCoord_Y (+0x28C);
        this->DestCoord.Z = techno->ChronoDestCoord_Z (+0x290);

        // Get ground height at destination
        this->DestCoord.Z = CellClass::GetGroundHeight(&this->DestCoord);

        // Handle bridge detection and Z adjustment
        CellClass* destCell = CellClass::Get_Cell_At(&this->DestCoord);
        if ((destCell->Flags & 0x100) == 0 || techno->IsOnBridge) {
            techno->IsOnBridge = 0;
        } else {
            techno->IsOnBridge = 1;
            this->DestCoord.Z += g_BridgeZOffset_Teleport;  // 0xB0EC38
        }

        // Place at destination
        techno->vtable->SetCoord(&this->DestCoord);
    }

    // Final position sync
    if (this->DestCoord == NullCoord) {
        // Use techno's current location
        techno->vtable->SetCoord(&techno->Location);
        return 0;
    }
    techno->vtable->SetCoord(&this->DestCoord);
    return 1;
}
```

**Confidence: 85%** -- The mode 0 (damage dealing) logic is complex with many object
iteration paths. The mode 1 (bridge Z adjustment) is clearly verified from assembly.

---

## 7. Post-Warp Validation (0x7187A0) -- Full Decompilation

Called in Phase 5 when `PendingWarpPhase == 0` (self-teleport, not ChronoSphere).
Validates the destination and handles edge cases.

### Full Logic

```c
void PostWarpValidation(TeleportLoco* this, CoordStruct dest) {
    // 1. Damage all flying objects at destination cell
    CellClass* destCell = CellClass::Get_Cell_At(&dest);
    for (ObjectClass* obj = destCell->FirstObject; obj; obj = obj->Next) {
        if (obj->IsInAir()) {
            TypeClass* myType = techno->vtable->GetType();
            int damage = myType->Strength;
            techno->vtable->TakeDamage(
                &damage, 0, Rules->C4Warhead (+0xFA8)  /* corrected 2026-05-19: stock gamemd reads INI key "C4Warhead" (section [CombatDamage]) into Rules+0xFA8; prior label "ChronoWarpDamagePercent" is fictional */, 0, 1, 0);
        }
    }

    // 2. Check if TypeClass has Chronoshiftable flag
    TypeClass* type = techno->vtable->GetType();
    bool chronoshiftable = type->Chronoshiftable (+0xCCE);

    // 3. Check if unit needs power validation (vehicles on powered buildings)
    bool needsPower = false;
    if (type->SpeedType (+0x67C) == 3) {  // SPEED_TRACK
        needsPower = true;
        if (type->NeedsEngineer (+0x410)) {
            HouseClass* house = techno->Owner (+0x21C);
            if (!HouseClass::HasPowerSurplus(house)) {
                needsPower = false;
            }
        }
    }

    // 4. Check destination land type
    CellStruct destCellCoord = CellStruct::FromCoord(dest);
    CellClass* cell = MapClass::Get_CellClass(&destCellCoord);

    if (cell->LandType (+0xEC) == 2  /* LAND_WATER */  && !needsPower) {
        // Unit warped onto water!
        if (!chronoshiftable && techno->WhatAmI() != 0xF  /* not infantry */) {
            // Check if cell truly has water (not a bridge over water)
            CellClass* destCellObj = CellClass::Get_Cell_At(&dest);
            if (!(destCellObj->Flags & 0x100)) {  // no bridge
                CellClass* deepCheck = CellClass::Get_Cell_At(&dest);
                if (deepCheck->LandType != 1  /* LAND_CLEAR */) {
                    // === KILL THE UNIT ===
                    techno->ShouldSelfDestruct (+0x3CD) = 1;
                    techno->vtable->KillSelf();                    // vtable+0x3A0

                    // Handle linked building release
                    if (techno->LinkedBuilding (+0x2D8) != 0) {
                        FUN_006B0AE0(
                            techno->ChronoSourceBuilding (+0x428),
                            techno->ChronoSourceHouse (+0x42C));
                        BuildingClass* bld = techno->LinkedBuilding;
                        if (bld != NULL) {
                            bld->vtable->Remove(1);  // vtable+0x20
                        }
                        techno->LinkedBuilding = 0;
                    }

                    // Scatter to nearby valid cell if possible
                    FootClass* linked = techno;
                    if (linked->TargetA (+0x10A * 4) != 0) {
                        linked->vtable->ScatterFrom(linked->TargetA);
                    } else if (linked->TargetB (+0x10B * 4) != 0) {
                        linked->vtable->ScatterFromB(linked->TargetB);
                    }
                    return;
                }
            }
        }
    }

    // 5. Check cell passability
    CellStruct cellCoord = CellStruct::FromCoord(dest);
    CellClass* passCell = MapClass::Get_CellClass(&cellCoord);
    int canEnter = techno->vtable->CanEnterCell(passCell, -1, -1, 0, 1);

    if (canEnter == 7  /* MOVE_NO */  || /* other failure */) {
        // Cell is blocked!
        // Check if there's a bridge
        CellStruct origCell = CellStruct::FromCoord(techno->OriginalLocation);
        MapClass::Get_CellClass(&origCell);
        bool hasBridge = FUN_004865D0(cell);  // bridge overlay check

        if (!hasBridge || techno->WhatAmI() == 0xF) {
            // No bridge or infantry: deal damage to self
            TypeClass* myType = techno->vtable->GetType();
            int damage = myType->Strength;
            techno->vtable->TakeDamage(
                &damage, 0, Rules->C4Warhead, 0, 1, 0, 0);
        } else {
            // On a bridge and blocked: kill self
            techno->ShouldSelfDestruct = 1;
            techno->vtable->KillSelf();
            // ... same building detach logic as water death ...
        }
    }
}
```

### Key Edge Cases:
1. **Water death**: Unit warped onto water (LandType==2) with no bridge -> killed
2. **Occupied cell**: Unit warped onto blocked cell -> takes C4Warhead damage (Rules+0xFA8, corrected 2026-05-19)
3. **Bridge handling**: If cell has bridge overlay (FUN_004865D0), unit survives on bridge
4. **Power check**: Vehicles with SPEED_TRACK need power surplus or lose protection

**Confidence: 85%** -- Complex function with many nested branches. Core kill/damage
paths are verified. The bridge detection at 0x4865D0 checks overlay indices against
known bridge overlay ranges.

---

## 8. Is_Moving (0x718080), Destination (0x7180A0), and Stop_Moving/Clear_Coords (0x718230)
<!-- corrected 2026-07-18: header previously called both 0x7180A0 and 0x718230
     "Stop_Moving" while the body below correctly names them Destination and Clear_Coords
     respectively; verified via decompile_function 0x718230 (returns
     TeleportLocomotionClass__Stop_Moving) and 0x7180A0 (Destination accessor). -->


### Is_Moving (0x718080)
```asm
00718080: MOV EAX, [ESP+4]        ; EAX = ILocomotion this (base+4)
00718084: CMP byte [EAX+0x30], 1  ; check base+0x34 = IsMoving
00718088: SETZ AL
0071808B: RET 4
```
Returns `true` if IsMoving (base+0x34) == 1.

### Destination (0x7180A0)
Returns current destination coordinates. If Is_Moving is true, returns DestCoord.
Otherwise returns techno->Location.

```c
CoordStruct Destination(ILocomotion* this, CoordStruct* out) {
    if (Is_Moving(this)) {
        *out = this->HeadToCoord;  // base+0x1C/0x20/0x24
    } else {
        *out = techno->Location;   // techno+0x9C/0xA0/0xA4
    }
}
```

### Clear_Coords (0x718230)
```asm
MOV EAX, [ESP+4]
MOV [EAX+0x18], g_NullCoord_X   ; HeadToCoord = NullCoord
MOV [EAX+0x1C], g_NullCoord_Y
MOV [EAX+0x20], g_NullCoord_Z
MOV byte [EAX+0x30], 0          ; IsMoving = 0
MOV byte [EAX+0x32], 0          ; field_36 = 0 (note: +0x32 from ILoco = base+0x36)
RET 4
```

**Confidence: 99%** -- Trivial functions, fully verified from assembly.

---

## 9. IPiggyback COM Interface

**Vtable: 0x7F4FDC** (set at TeleportLocomotionClass+0x18)

| Index | Address | Method |
|-------|---------|--------|
| 0 | 0x71A190 | QueryInterface |
| 1 | 0x71A1A0 | AddRef |
| 2 | 0x71A1B0 | Release |
| 3 | 0x719E90 | Begin_Piggyback |
| 4 | 0x719EE0 | End_Piggyback |
| 5 | 0x719F30 | Is_Ok_To_End |
| 6 | 0x719F80 | Piggyback_CLSID |
| 7 | 0x71A100 | Is_Piggybacking |

### Begin_Piggyback (0x719E90)
```c
HRESULT Begin_Piggyback(IPiggyback* this, ILocomotion* newLoco) {
    if (newLoco == NULL) return E_POINTER;     // 0x80004003
    if (this->PiggybackLoco != NULL)
        return E_FAIL;                          // 0x80004005
    this->PiggybackLoco = newLoco;
    newLoco->AddRef();
    return S_OK;
}
```

### End_Piggyback (0x719EE0)
```c
HRESULT End_Piggyback(IPiggyback* this, ILocomotion** ppOut) {
    if (ppOut == NULL) return E_POINTER;

    // Clear chrono source references on the TechnoClass
    FootClass* linked = *(this - 0x0C);  // base+0x0C (LinkedTo)
    if (linked != NULL) {
        linked->ChronoSourceBuilding (+0x428) = 0;
        linked->ChronoSourceHouse (+0x42C) = 0;
    }

    if (this->PiggybackLoco != NULL) {
        *ppOut = this->PiggybackLoco;
        this->PiggybackLoco = NULL;
        return S_OK;
    }
    return S_FALSE;  // 1 = nothing piggybacked
}
```

### Is_Ok_To_End (0x719F30)
```c
bool Is_Ok_To_End(IPiggyback* this) {
    // Cannot end if locomotor is still moving
    if (Is_Moving(this - 0x14))  return false;  // ILocomotion this
    // Must have something piggybacked
    if (this->PiggybackLoco == 0) return false;
    // field_35 (base+0x35) must be 0
    if (*(byte*)(this + 0x1D) != 0) return false;
    // TechnoClass chrono state must be clear
    FootClass* linked = *(this - 0x0C);
    if (linked->ChronoInTransit (+0x27C) != 0) return false;
    // WarpPhase must be 0
    if (*(dword*)(this + 0x20) != 0) return false;  // base+0x38 = WarpPhase
    // TechnoClass+0x6AD must be 0
    if (linked->field_6AD != 0) return false;
    return true;
}
```

### TechnoClass +0x428 and +0x42C -- What They Really Are

These fields store context for the ChronoSphere "return to sender" mechanic:
- +0x428: Pointer to the building (ChronoSphere) that initiated the warp
- +0x42C: Pointer to the house that owns the ChronoSphere

They are:
- Set by the ChronoSphere superweapon handler (FUN_0065EC30)
- Passed to FUN_006B0AE0 when a chrono-warped unit dies (handles passenger release)
- Cleared by End_Piggyback and in Phase 5

For self-teleport (chrono miners), these are always 0.

**Confidence: 95%**

---

## 10. IUnknown Vtable for TeleportLocomotionClass

**Vtable: 0x7F50CC** (set at TeleportLocomotionClass+0x00)

| Offset | Address | Method |
|--------|---------|--------|
| 0x00 | 0x719E30 | QueryInterface |
| 0x04 | 0x71A0E0 | AddRef |
| 0x08 | 0x71A0F0 | Release |
| 0x0C | 0x719C60 | (LocomotionClass method) |
| 0x10 | 0x4B4C30 | (inherited) |
| 0x14 | 0x719CA0 | (LocomotionClass method) |
| 0x18 | 0x719D40 | (LocomotionClass method) |
| 0x1C | 0x55AB40 | (inherited) |
| 0x20 | 0x71A130 | (LocomotionClass method) |
| 0x24 | 0x71A120 | (LocomotionClass method) |
| **0x28** | **0x719BF0** | **TimerCheck -- The timer expiry handler** |
| 0x2C | 0x718090 | (LocomotionClass method) |

### TimerCheck (0x719BF0) -- Used by Phases 1 and 6

This function is called via IUnknown vtable[0x28] during phases 1 and 6 to wait
for the chrono timer to expire. Full pseudocode in Section 3 (Phase 1).

**Confidence: 95%** -- Vtable read directly from binary, TimerCheck fully decompiled.

---

## 11. Chronosphere Superweapon Interaction (0x65EC30)

The ChronoSphere superweapon uses a different entry point into the teleport state machine.

### Key Differences from Self-Teleport:

1. **Phase skip**: Sets `TechnoClass->PendingWarpPhase (+0x280) = 3`, causing the
   state machine to start at Phase 2 (after the pre-phase check copies it to WarpPhase)

2. **Locomotor swap**: Creates a new TeleportLocomotionClass via COM CoCreateInstance
   (CLSID at 0x7E9A90) and uses IPiggyback::Begin_Piggyback to store the old locomotor

3. **Destination calc**: Sets TechnoClass fields:
   - ChronoInTransit (+0x27C, via piVar6[0x9F]) = set externally
   - ChronoDestCoords (+0x288/0x28C/0x290) = calculated destination offset
   - PendingWarpPhase (+0x280) = 3

4. **Post-placement**: After all units are placed:
   ```c
   techno->vtable->Unmark(0);           // vtable+0x124
   techno->PendingWarpPhase = 3;        // piVar9[0xA0] = 3
   techno->vtable->ScatterFrom();       // vtable+0x1EC
   ```

5. **ChronoReinfDelay**: The delay for ChronoSphere warp uses Rules+0xBF0
   (ChronoReinfDelay) instead of the distance-based calculation

**Confidence: 85%** -- Large function with complex iterator patterns.

---

## 12. TechnoClass Chrono Fields (Corrected and Complete)

All offsets are BYTE offsets on TechnoClass:

| Offset | Type | Field Name | Setter/User |
|--------|------|-----------|-------------|
| +0x08C | byte | IsOnBridge | State machine phase 0, Update_Position |
| +0x090 | byte | IsAlive | Checked in phase 5 after validation |
| +0x09C | Coord | Location (X,Y,Z) | 12 bytes: unit's current world coords |
| +0x218 | int | field_218 | Set by FUN_0070C610(val) -- visual warp state |
| +0x21C | ptr | OwnerHouse | HouseClass pointer |
| +0x270 | byte | WarpingOut | Set=1 in chrono-in-transit init, cleared=0 in Phase 2 |
| +0x271 | byte | BeingWarped | Set=1 when warp starts, cleared=0 by TimerCheck/Phase 7 |
| +0x27C | byte | ChronoInTransit | Byte at offset 0x27C, set externally by ChronoSphere |
| +0x280 | int | PendingWarpPhase | Set to 3 by ChronoSphere, 0 by state machine |
| +0x284 | int | ChronoLockDuration | = Rules->ChronoDelay, stored in Phase 3, used in Phase 5 |
| +0x288 | int | ChronoDestCoord.X | Set by ChronoSphere superweapon |
| +0x28C | int | ChronoDestCoord.Y | |
| +0x290 | int | ChronoDestCoord.Z | |
| +0x2B4 | int | Garrison | Checked in TimerCheck |
| +0x2D8 | ptr | LinkedBuilding | Building link, used in water death cleanup |
| +0x3CD | byte | ShouldSelfDestruct | Set=1 when warped onto invalid terrain |
| +0x3D5 | byte | Discovered | Cleared if dest not in playfield |
| +0x428 | ptr | ChronoSourceBuilding | Building that initiated ChronoSphere warp |
| +0x42C | ptr | ChronoSourceHouse | House that owns the ChronoSphere |
| +0x5A4 | ptr | DockBuilding | Target building (refinery, etc.) |
| +0x694 | ptr | FlashAnim | Detached during warp |
| +0x6AD | byte | field_6AD | Checked in Is_Ok_To_End |
| +0x6C4 | ptr | OwnerHouseType | -> HouseTypeClass |

### TechnoTypeClass Chrono Fields

| Offset | INI Key | Type | Notes |
|--------|---------|------|-------|
| +0x410 | NeedsEngineer | bool | Power validation for bridge warp |
| +0x574 | ChronoInSound | int | Per-type warp-in sound (-1 = use global) |
| +0x578 | ChronoOutSound | int | Per-type warp-out sound (-1 = use global) |
| +0x5B4 | SpeedType | int | Movement zone type |
| +0x67C | SpeedType2 | int | Secondary speed type (used in pathfinding) |
| +0xA0 | Strength | int | Max HP (used for damage calc) |
| +0xCCE | Chronoshiftable | bool | Can be moved by Chronosphere |
| +0xE0E | ChronoKillInfantry | bool | On HouseTypeClass: infantry killed by chrono |

**Confidence: 90%** -- Cross-referenced from state machine, Update_Position, and
PostWarpValidation decompilations.

---

## 13. Key Globals

| Address | Name | Notes |
|---------|------|-------|
| 0xB0EBF8 | g_NullCoord_X | Sentinel value for "no coordinate" (XYZ, 12 bytes) |
| 0xB0EBFC | g_NullCoord_Y | |
| 0xB0EC00 | g_NullCoord_Z | |
| 0xB0EC38 | g_BridgeZOffset | Height offset for bridge Z in Update_Position |
| 0xA8ED84 | g_CurrentFrameCounter | Global frame/tick counter |
| 0x8871E0 | g_RulesClass_Instance | Pointer to the singleton RulesClass |
| 0xA8ED44 | g_AnimArray | Array of all AnimClass instances |
| 0xA8ED50 | g_AnimCount | Count of anim instances |
| 0x87F7E8 | g_MapClass_Instance | MapClass singleton for cell lookups |

---

## 14. Warp Animations and Sounds

### Animation Types
| Rules Offset | INI Key | Used In |
|-------------|---------|---------|
| +0x33C | WarpAway | ALL warp anims (phase 0, 2, 5 use this same type) |

The `WarpAway` anim (Rules+0x33C) is used for ALL warp visual effects.
Both warp-out and warp-in spawn the same AnimType at the unit's location:
```c
AnimClass::Constructor(Rules->WarpAway, &coords, 0, 1, 0x600, 0, 0);
```
The `0x600` flag controls the animation speed/loop behavior.

### Sound Effects
| Rules Offset | INI Key | Used When |
|-------------|---------|-----------|
| +0x218 | ChronoInSound | Warp-in (arrival) -- global fallback |
| +0x21C | ChronoOutSound | Warp-out (departure) -- global fallback |

Per-type overrides on TechnoTypeClass:
| Offset | INI Key |
|--------|---------|
| +0x574 | ChronoInSound |
| +0x578 | ChronoOutSound |

Logic: `if (TypeClass->sound != -1 || Rules->sound != -1) PlaySound(...)`.
The per-type sound takes priority when != -1.

---

## 15. Complete Call Graph

```
ILocomotion::vtable[16] (0x7192F0)  -- State Machine Tick (every frame)
  |-- FUN_0070D4A0: Stop all targeting of this unit
  |-- FUN_00468430: Detach anim from unit
  |-- AnimClass::Constructor (0x421EA0): Spawn WarpAway anim
  |-- Math::sqrt (0x4CAC40): Euclidean distance
  |-- Math::ftol (0x7C5F00): Float to int
  |-- FUN_007509E0: Play sound effect
  |-- FUN_0062A4A0: Detach flash anim
  |-- MapClass::Get_CellClass (0x5657A0): Cell lookup
  |-- FUN_00481A00: Crate/powerup pickup
  |-- FUN_007187A0: Post-warp validation
  |-- FUN_0070C610: Set visual warp state on TechnoClass
  |-- TeleportLocomotionClass::Update_Position (0x718260)
  |     |-- CellClass::Get_Cell_At: Cell lookup
  |     |-- CellClass::GetGroundHeight: Terrain Z
  |     |-- Pathfinding_validate_alternate: Find valid cell
  |     |-- CellClass::PlaceInfantryInCell: Infantry subcell
  |-- TimerCheck (0x719BF0): Timer expiry for phases 1 and 6
  |     |-- FUN_0070F770: Garrison check
  |     |-- FUN_00709480: Additional validation

ILocomotion::vtable[17] (0x718100)  -- Head_To_Coord (new destination)
  |-- TeleportLocomotionClass__Process (0x718B70)
  |     |-- CellClass::Get_Cell_At
  |     |-- CellClass::GetGroundHeight
  |     |-- Pathfinding_validate_alternate
  |     |-- CellClass::PlaceInfantryInCell
  |     |-- MapClass::GetZoneID
```

---

## 16. Summary of Key Corrections from v1

1. **WarpFactor is NOT a float ramp.** TechnoClass+0x280 is `PendingWarpPhase` (integer),
   set to 3 by ChronoSphere or 0 by state machine. The visual warp alpha is derived from
   the CDTimerClass countdown at locomotor +0x3C/+0x44.

2. **TechnoClass+0x428/+0x42C are NOT visual warp factors.** They are pointers to the
   ChronoSphere building and its owner house, used for "return to sender" on unit death.

3. **Phase 1 and 6 call TimerCheck (0x719BF0)**, not "End_Piggyback". The IUnknown
   vtable[0x28] resolves to the timer expiry function which advances the phase.

4. **Timer middle field (+0x40) is unused** in the countdown logic. Only StartFrame (+0x3C)
   and Duration (+0x44) participate in the elapsed calculation.

5. **Process (0x718B70) and the State Machine (0x7192F0) are separate functions** at different
   vtable slots. Process is called when a new destination is set (vtable[17]). The state
   machine ticks every frame (vtable[16]).

6. **The ChronoSphere sets PendingWarpPhase=3**, not 2. The state machine then picks this up
   and enters Phase 2 (IN_TRANSIT_START) on the next tick because the pre-phase check copies
   PendingWarpPhase to WarpPhase, and Phase 2 is the first case checked when WarpPhase >= 2.

---

## 14. Mission_Harvest State 2 -- Teleport vs Drive Decision (0x73E5E0)

> **CORRECTION (2026-07-19, verify-doc-fix-swarm slot 4) -- supersedes the 2026-07-18 flag
> below.** [docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) (this session,
> building on [CHRONO_MINER_SET_DESTINATION_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_SET_DESTINATION_GATE_GHIDRA_REPORT.md)) closes most of the
> 2026-07-18 flag's open item and REFUTES this section's "empty cell -> FindFirstBuilding
> NULL -> stays Teleport -> warps" framing outright, not just the function name. Re-verified
> live this session: `get_function_by_address 0x0047eba0` confirms `CellClass__FindFirstUnit`
> (body 0047eba0-0047ebe2); `get_xrefs_to 0x00719400` returns ZERO references (some docs cite
> this address as "TeleportLocomotionClass::InitiateWarp" -- it is a spurious mid-function
> label entirely inside `StateMachineTick`'s own body, not a callable function -- do not cite
> it); `get_function_by_address 0x007192f0` confirms `TeleportLocomotionClass__StateMachineTick`
> (body 007192f0-00719bed) -- this IS the real per-tick `ILocomotion::Process` (vtable slot
> +0x40, matches `DriveLocomotionClass::Process` at the identical offset); `get_function_by_address
> 0x00718b70` confirms `TeleportLocomotionClass__Process` (body 00718b70-007192bd, ending
> exactly where StateMachineTick begins) is a DIFFERENT function, called only synchronously
> from `HeadToCoord` -- it is NOT the per-tick Process despite the name Ghidra assigned it.
>
> The deeper correction: "empty destination cell" alone is NOT sufficient to warp. The warp
> only arms when Teleport's own `HeadToCoord` (vtable+0x44) sets `Is_Moving`, and
> `Set_Destination_Internal` (0x4D94B0) dispatches `HeadToCoord` only to whichever locomotor
> is ALREADY active at that moment. Critically, this section's own "Scenario 1" below (far
> harvest return) is WRONG as written: the Mission_Harvest state-2 fallback `Set_Destination`
> call it attributes the warp to is proven to always have NavCom==NULL at that call site,
> which takes the Teleporter predicate's default "prefer Drive" branch -- that specific call
> can NEVER arm a warp, regardless of distance. The exact call that supplies a DockUnload-flagged
> building as the OLD NavCom (the actual precondition for a warp) remains OPEN --
> see [CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) §2 and §6 for the traced candidates
> (Mission_Enter's CAN_DOCK reassert, `Receive_Radio` case 0x12, the Dock=-list re-target
> block) and why none is independently confirmed yet. Treat "Scenario 1" and "Scenario 2"
> below, and Section 21's parallel "Complete Call Chain," as SUPERSEDED narrative pending that
> open item -- do not restate them as ground truth. The distance thresholds and RulesClass
> offsets elsewhere in this section are unaffected.
>
> **Prior flag (2026-07-18, verify-doc-fix-swarm w2 slot 4), partially superseded above:** the
> "FindFirstBuilding decides teleport-vs-drive" mechanism this section's "Fallback / teleport
> path" and the linked Section 21 narrative depend on has been shown WRONG (it's actually
> `CellClass::FindFirstUnit`, checking `WhatAmI()==1`, nested inside a building-target
> branch). The Mission_Harvest state-2 distance thresholds and RulesClass offsets below are
> independently verified and unaffected; the "how the locomotor swap decides" mechanism is
> UNVERIFIED pending `/re-investigate`.

**Verified from gamemd.exe decompilation. Confidence: 95%.**

### Overview

UnitClass::Mission_Harvest at 0x73E5E0 is the YR harvest state machine (338 lines decompiled).
State 2 (RETURN) handles the decision of how a harvester returns to its refinery. For chrono
miners (Teleporter=yes), this includes the teleport-vs-drive distance check.

### Key Variables

- `param_1[0x1b1]` (byte offset 0x6C4) = TechnoTypeClass pointer
- `cVar1 = *(char*)(TypeClass + 0xCD4)` = **Teleporter flag** (TechnoTypeClass+0xCD4)
- `param_1[0x2f]` (byte offset 0xBC) = harvest state (0=SEEK, 1=HARVEST, 2=RETURN, 3=DOCK, 4=LOST)
- `param_1[0x169]` (byte offset 0x5A4) = current destination

### State 2 (RETURN) -- Complete Logic

```
Entry: Unit has full cargo or can't find more tiberium.

1. If destination already set AND is Teleporter:
     dock = Find_Docking_Bay(TypeClass->DockList, 0, 0)
     if dock found: FootClass::Stop_Moving()

2. If destination already set: return (keep going)

3. dock = Find_Docking_Bay(TypeClass->DockList, 0, 0)

4. IF NOT Teleporter (regular harvester):
     if dock found:
       dist = 3D_Euclidean_Distance(unit, dock) in LEPTONS
       if dist <= RulesClass+0xD78 (HarvesterTooFarDistance) * 256:
         QueueMission(MISSION_DOCK=2, dock)   // vtable+0x278
         state = 3
       // else: fall through to fallback path

5. IF Teleporter (chrono miner):
     if dock found:
       dist = 3D_Euclidean_Distance(unit, dock) in LEPTONS
       if dist <= RulesClass+0xD7C (ChronoHarvTooFarDistance) * 256:
         QueueMission(MISSION_DOCK=2, dock)   // DRIVE to dock (same as regular!)
         state = 3
       // else: fall through to teleport path

6. FALLBACK / TELEPORT PATH (distance exceeded or no dock found):
     g_MapEditorMode++    // bypass ownership checks
     dock = Find_Docking_Bay(TypeClass->DockList, 0, 1)  // 3rd arg=1: find ANY dock
     g_MapEditorMode--
     if dock found:
       dist = 3D_Euclidean_Distance(unit, dock) in LEPTONS
       if dist > 0x300 (3 cells * 256) OR is Teleporter:
         // Compute cell near refinery dock entrance
         cellX = dock->Location.X / 256
         cellY = dock->Location.Y / 256
         dockOffsetX = dock->TypeClass->DockOffset.X (+0x1618)
         dockOffsetY = dock->TypeClass->DockOffset.Y (+0x161C)
         target = (cellX + dockOffsetX, cellY + dockOffsetY)
         validated = Pathfinding_validate_alternate(target, ...)
         if valid cell found:
           Set_Destination(CellClass at validated)
         else:
           Set_Destination(NULL)  // clear destination
```

### The Distance Comparison

Both distance checks use **3D Euclidean distance in leptons**:
```c
dx = unit.Location.X - dock.Location.X;
dy = unit.Location.Y - dock.Location.Y;
dz = unit.Location.Z - dock.Location.Z;
dist = (int)sqrt(dx*dx + dy*dy + dz*dz);
```

The threshold comparison is:
```c
dist <= RulesValue * 0x100
```

Where `0x100 = 256` is the number of leptons per cell. So the Rules values are in **cell units**:
- **HarvesterTooFarDistance** (Rules+0xD78, default 5): regular harvester drive range = 5 cells
- **ChronoHarvTooFarDistance** (Rules+0xD7C, default 50): chrono miner drive range = 50 cells

If within range: harvester DRIVES to dock (QueueMission with Mission_Dock).
If beyond range: falls through to the teleport/fallback path.

### How the Locomotor Swap Works for Chrono Miners

The chrono miner's primary locomotor is TeleportLocomotionClass (CLSID {4A582747-...}).
It also has `Teleporter=yes` in rules.ini.

**Locomotor lifecycle:**

1. **Spawn**: Teleport locomotor is primary (created from the Locomotor= INI key)

2. **Set_Destination called** (any movement order):
   - In `TechnoClass::Set_Destination` (0x741970), the Teleporter block (at 0x7423CD) checks:
     - `TypeClass+0xCD4 != 0` (Teleporter=yes)
     - `this+0x27C == 0` (not ChronoInTransit)
     - `this+0x2B0 == 0` (no suspended destination)
     - `this+0x6AD == 0` (not deploying)
   - Gets current locomotor's CLSID via IPersistStream::GetClassID
   - If NavTarget is a Building with BuildingTypeClass+0x16B3 set:
     - Calls CellClass::FindFirstUnit (0x47EBA0) on the destination cell [corrected 2026-07-19:
       `get_function_by_address 0x0047eba0` confirms `CellClass__FindFirstUnit`, not
       FindFirstBuilding -- see the top-of-section correction box]
     - If a unit is found in dest cell: proceeds to Drive piggyback (step 2a)
     - If dest cell has no unit AND CLSID is TeleportLocomotion: SKIPS piggyback (step 2b)
   - If NavTarget is NULL or not a relevant Building: proceeds to Drive piggyback (step 2a)

   **2a. Drive piggyback (unit DRIVES):**
   - Compares CLSID with Drive CLSID ({4A582741-...}) at 0x7E9A30
   - If current loco is NOT Drive: creates new DriveLocomotionClass, piggybacks Teleport
     under it, sets Drive as active. Unit now drives.
   - If current loco IS Drive: no swap needed, just set destination.
   - FootClass::Set_Destination_Internal calls Head_To_Coord on DriveLocomotionClass.

   **2b. No piggyback (unit TELEPORTS):**
   - TeleportLocomotionClass remains the active locomotor.
   - FootClass::Set_Destination_Internal calls Head_To_Coord on TeleportLocomotionClass.
   - Head_To_Coord sets IsMoving=1 and stores destination coords.
   - Next tick: StateMachineTick Phase 0 initiates the warp.

3. **Unit stops after driving** (reaches destination via Drive):
   - FootClass::AI (0x4DA530) checks IPiggyback::Is_Ok_To_End every tick
   - DriveLocomotionClass::Is_Ok_To_End (0x4AF970) returns true when:
     - Drive's ILocomotion::Is_Moving() == false (at rest)
     - Has a piggybacked locomotor (DriveBase+0x68 != 0)
     - DriveBase+0x65 flag is set (initialized to 1 in constructor, never cleared)
     - Owner+0x6AD == 0 (not deploying)
   - When true: FootClass::AI releases Drive, calls End_Piggyback, restores Teleport as active

4. **Teleport locomotor becomes active again** (after piggyback ends)

5. **Next Set_Destination**: decision repeats at step 2

**Key insight**: The Teleport locomotor performs a warp when its StateMachineTick
(0x7192F0) fires with Is_Moving=true and a valid HeadToCoord destination. This happens when the
TeleportLocomotionClass is the ACTIVE locomotor and receives a Head_To_Coord call (ILocomotion
vtable slot 17, offset 0x44). Head_To_Coord is called by FootClass::Set_Destination_Internal
(0x4D94B0) on whatever locomotor is currently active.

### When Does the Chrono Miner Actually Teleport? (CORRECTED)

The warp is triggered through TechnoClass::Set_Destination (0x741970), which contains a
critical Teleporter decision block starting at 0x7423CD. This block determines whether
the active TeleportLocomotionClass gets piggybacked by Drive (unit DRIVES) or remains
active (unit TELEPORTS). The decision depends on the DESTINATION CELL contents:

**Assembly-verified decision at 0x7424BD-0x7424FA:**

```
007424BD: MOV ECX, [ESI+0x520]      ; building->TypeClass
007424C3: MOV DL, [ECX+0x16B3]      ; BuildingTypeClass flag at +0x16B3
007424C5: TEST DL, DL               ; Flag set?
007424CD: TEST EAX, EAX             ; Destination is valid CellClass?
007424D3: PUSH 0x0
007424D5: MOV ECX, EAX              ; ECX = destination cell
007424D7: CALL CellClass__FindFirstUnit  ; [corrected 2026-07-18, decompile_function
                                          ;  0x47eba0: checks WhatAmI()==1 in the cell's
                                          ;  object list, NOT "any building" -- see the
                                          ;  correction flag at the top of this section]
007424DC: TEST EAX, EAX
007424DE: JNZ 0x7425DB              ; Building found -> Drive piggyback path
007424E4: MOV ECX, 0x4              ; No building at dest cell:
007424E9: MOV EDI, 0x7E9A90         ;   Compare CLSID with TeleportLocomotion
007424F4: CMPSD.REPE                ;   4-DWORD GUID comparison
007424F6: MOV [ESP+0x14], AL        ;   Clear piggyback flag to 0
007424FA: JZ 0x7425DB               ;   If CLSID IS Teleport -> skip piggyback!
```

**Result: Two distinct paths:**

[corrected 2026-07-19: the table below repeats the doc's original "building present/absent"
framing. Per the top-of-section correction box, 0x47EBA0 checks `WhatAmI()==1` (unit), not
`WhatAmI()==6` (building), and is nested inside a branch that already established the NavCom
*target* is a building -- so "dest cell HAS a building" is imprecise. It should read "a unit
is present in the destination cell." Even when the right-hand branch is taken (piggyback
skipped), that alone does not guarantee a warp -- see the correction box for why.]

| Condition | Path | Behavior |
|-----------|------|----------|
| Unit found in dest cell (FindFirstUnit non-NULL) | Drive piggyback (LAB_007425E6) | Unit DRIVES to destination |
| No unit in dest cell + CLSID is TeleportLoco | Skip piggyback (0x7427B2) | Teleport locomotor stays active (does NOT by itself guarantee a warp -- see correction box) |

**Complete teleport trigger chain:**

1. `FootClass::Set_Destination_Internal` (0x4D94B0) is called at the end of Set_Destination
2. It gets the target's coordinates via `target->Get_Coords()` (vtable+0x4C)
3. Calls `ILocomotion::Head_To_Coord` (vtable+0x44) on the ACTIVE locomotor
4. If active locomotor is TeleportLocomotionClass (no piggyback happened):
   - `TeleportLocomotionClass::HeadToCoord` (0x718100) runs
   - Calls `TeleportLocomotionClass::Process` (0x718B70) to validate and compute destination
   - If destination is valid (not NullCoord): sets **IsMoving = 1** (base+0x34) and stores
     warp destination in HeadToCoord (base+0x1C/0x20/0x24)
5. Next tick: `FootClass::AI` calls `ILocomotion::Process` (vtable+0x40)
   = `TeleportLocomotionClass::StateMachineTick` (0x7192F0)
6. Phase 0 checks `Is_Moving()` (0x718080, returns base+0x34 == 1) -> **true**
7. Checks `dest != current position` -> **true** (miner is at ore, dest is near refinery)
8. **INITIATES WARP**: plays WarpOut anim, calculates chrono delay, sets BeingWarped, etc.

**The chrono miner teleports in these scenarios (SUPERSEDED -- see top-of-section correction
box, 2026-07-19):**

1. **Harvest return (far from refinery) -- WRONG, refuted this session.** This scenario
   claimed the state-2 fallback Set_Destination call (with an empty dock-adjacent cell)
   triggers the warp. [CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) §1.5 proves NavCom is
   always NULL at that specific call site (`decompile_function 0x0073E5E0`, state 2's
   fallback branch is gated by `if (param_1[0x169] != 0) goto default;`), which forces the
   Teleporter predicate's default "prefer Drive" branch every time. This call can NEVER arm
   a warp -- the miner DRIVES the long return leg via this path, not teleports.

2. **Harvest return (close to refinery) -- UNVERIFIED, not re-confirmed this session.** The
   claim that the eventual Set_Destination-to-refinery-cell call is what drives the unit is
   plausible (a unit IS present there, so FindFirstUnit is non-NULL and Drive piggyback is
   taken) but the exact accepted-dock call sequence was not independently traced this pass --
   see [CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) §2/§6 open item.

3. **Player move order**: Right-clicking on empty ground calls Set_Destination with an
   empty cell. FindFirstUnit [corrected 2026-07-19: was "FindFirstBuilding", see correction
   box] returns NULL. CLSID is TeleportLocomotion. Piggyback skipped -- but per the correction
   box, this does not by itself guarantee a warp; whether Teleport was already the active
   locomotor and whether HeadToCoord fires with a non-null target was not independently
   re-verified for the player-order path this session.

4. **Player move order to occupied cell**: If the player clicks on a cell containing a unit,
   FindFirstUnit [corrected 2026-07-19: was "FindFirstBuilding"] returns non-NULL. Drive
   piggyback is created. Miner drives.

5. **ChronoSphere superweapon**: Sets ChronoInTransit (+0x27C) and PendingWarpPhase (+0x280)
   directly, bypassing Set_Destination entirely. StateMachineTick picks up the phase.

6. **Chrono Legionnaire erasing**: Via the warp weapon mechanism (sets BeingWarped directly).

---

## 15. Teleporter Flag -- All Readers

**TechnoTypeClass+0xCD4** (Teleporter=yes/no, parsed in TechnoTypeClass::ReadINI at 0x713FF6)

### All code locations that read TypeClass+0xCD4:

| Address | Function | Purpose |
|---------|----------|---------|
| 0x73E6E0 | UnitClass::Mission_Harvest (0x73E5E0) | Select cVar1 branch for Teleporter vs regular harvester |
| 0x7423D3 | TechnoClass::Set_Destination (0x741970) | Gate the locomotor swap block (Teleport<->Drive) |
| 0x740948 | UnitClass::Mission_Guard_Harvester (0x740810) | Check if idle Teleporter should scan for refinery |
| 0x4D93F4 | UnitClass::Mission_Harvest (base RA2, 0x4D9290) | Teleporter-specific cleanup on dock |
| 0x444CCD | TacticalClass::DrawObjects (0x443C60) | Visual rendering check for Teleporter units |

---

## 16. HarvesterTooFarDistance vs ChronoHarvTooFarDistance -- All Readers

### RulesClass+0xD78 = HarvesterTooFarDistance (default: 5 cells)

Written by: RulesClass::ReadGeneral at 0x66FFD9 (CCINIClass::ReadInt with string at 0x83C480)

Read by: UnitClass::Mission_Harvest (0x73E5E0) at 0x73EC10 -- used in state 2 for NON-Teleporter
harvesters. Comparison: `dist <= HarvesterTooFarDistance * 0x100`

### RulesClass+0xD7C = ChronoHarvTooFarDistance (default: 50 cells)

Written by: RulesClass::ReadGeneral at 0x66FFF8 (CCINIClass::ReadInt with string at 0x83C464)

Read by: UnitClass::Mission_Harvest (0x73E5E0) at 0x73EE42 -- used in state 2 for Teleporter
harvesters. Comparison: `dist <= ChronoHarvTooFarDistance * 0x100`

Both values are ONLY read in UnitClass::Mission_Harvest state 2. They are not used anywhere else
in the codebase.

---

## 17. Find_Docking_Bay and Find_Nearest_Dock

### FootClass::Find_Docking_Bay (0x4DF040)

```c
int FootClass::Find_Docking_Bay(DockList* list, int param_3, int param_4)
{
    int best = NULL;
    int best_dist = -1;
    for (int i = 0; i < list->Count; i++) {
        int dist = -1;
        int dock = this->vtable[0x52C](list[i], param_3, param_4, &dist);
        if (dock && (best == NULL || dist < best_dist || best_dist == -1 || dock->field_3D3)) {
            best_dist = dist;
            best = dock;
        }
    }
    return best;
}
```

Iterates the type's DockList, calling vtable+0x52C (Find_Nearest_Dock_Of_Type) for each dock
type. Returns the closest valid dock.

### FootClass::Find_Nearest_Dock (0x4DFCB0)

```c
int FootClass::Find_Nearest_Dock()
{
    BuildingClass* best = NULL;
    int best_dist = INT_MAX;
    // Iterate owner house building list (backwards)
    for (int i = house->BuildingCount - 1; i >= 0; i--) {
        BuildingClass* bld = house->Buildings[i];
        int dist = 3D_Euclidean_Distance(unit, bld);  // in leptons
        if (dist < best_dist && BuildingClass::CanDock(bld, this)) {
            best_dist = dist;
            best = bld;
        }
    }
    if (best) {
        this->field_690 = 1;
        if (this->Destination != best || this->CurrentMission != 8) {
            Set_Destination(best, 1);
            Assign_Mission(8, 1);  // Mission_Enter
        }
        return 1;
    }
    this->field_690 = 0;
    return 0;
}
```

Scans all buildings owned by the unit's house, finds the nearest valid dock.

---

## 18. Mission_Enter for Harvesters (0x739EC0)

UnitClass::Mission_Enter handles the final docking approach. Key behaviors:

- **Mission 9 (Mission_Unload)**: Unit sells itself at the building, giving money for cargo
- **Mission 7 (Mission_Enter)**: Unit enters/docks with building
- **Mission 25 (0x19)**: Also routes to the docking logic

When the harvester is at the same cell as its dock target (building), it queues Mission_Unload
(0x15=21) and stops the locomotor.

The function does NOT have special Teleporter handling -- the chrono miner uses the same
Mission_Enter path as regular harvesters after arriving at the refinery.

---

## 19. IPiggyback COM Interface

### TeleportLocomotionClass ILocomotion vtable (at 0x7F5000)

Verified by reading memory at 0x7F5000 and cross-referencing with decompiled functions:

| Offset | Address | Method | Notes |
|--------|---------|--------|-------|
| 0x00 | 0x71A160 | QueryInterface | |
| 0x04 | 0x71A170 | AddRef | |
| 0x08 | 0x71A180 | Release | |
| 0x0C | 0x55A710 | Link_To_Object | From LocomotionClass base |
| 0x10 | 0x718080 | **Is_Moving** | Returns `base+0x34 == 1` (WarpInProgress flag) |
| 0x14 | 0x7180A0 | Destination | Returns HeadToCoord or current position |
| 0x18 | 0x55ACA0 | Stop_Moving_Loco | From LocomotionClass base |
| 0x40 | 0x7192F0 | **Process** (StateMachineTick) | Called every tick by FootClass::AI |
| 0x44 | 0x718100 | **Head_To_Coord** | Sets destination + arms warp |
| 0x48 | 0x718230 | **Stop_Moving** | Clears dest, WarpInProgress, flags |

Key methods for teleport triggering:
- **Head_To_Coord** (0x718100): Called by FootClass::Set_Destination_Internal. Validates
  destination via Process(), sets IsMoving=1 and HeadToCoord coords. This ARMS the warp.
- **Process/StateMachineTick** (0x7192F0): Called every tick. Phase 0 checks Is_Moving();
  if true and dest != current pos, INITIATES the warp sequence.
- **Is_Moving** (0x718080): Simple flag check at base+0x34. Returns true after Head_To_Coord
  has been called with a valid destination.

---

## 19b. IPiggyback COM Interface

**IID: {92FEA800-A184-11D1-B70A-00A024DDAFD1}** (at global 0x819088)

### Methods (after IUnknown):
| Offset | Method | Description |
|--------|--------|-------------|
| 0x0C | Begin_Piggyback(ILocomotion*) | Store a locomotor underneath this one |
| 0x10 | End_Piggyback(ILocomotion**) | Retrieve and remove the piggybacked locomotor |
| 0x14 | Is_Ok_To_End() | Should the piggyback end this tick? |
| 0x18 | Piggybacker_CLSID(CLSID*) | Get the CLSID of this locomotor |
| 0x1C | Is_Moving_Under_Piggyback() | Is the piggybacked locomotor moving? |

### DriveLocomotionClass IPiggyback Implementation
- vtable at 0x7E7E8C
- Begin_Piggyback (0x4AF8E0): stores at DriveBase+0x68
- End_Piggyback (0x4AF930): retrieves from DriveBase+0x68
- Is_Ok_To_End (0x4AF970): true when not moving, has piggyback, flag set (DriveBase+0x65=1)
- The DriveBase+0x65 flag is initialized to 1 in constructor and never cleared

### TeleportLocomotionClass IPiggyback Implementation
- vtable at 0x7F4FDC
- Begin_Piggyback (0x719E90): stores at TeleportBase+0x48
- End_Piggyback (0x719EE0): retrieves from TeleportBase+0x48; clears owner+0x428/+0x42C
- Is_Ok_To_End (0x719F30): true when not moving, has piggyback, warp complete

### FootClass::AI Piggyback Swap (end of 0x4DA530)
Every tick, FootClass::AI queries the active locomotor for IPiggyback. If Is_Ok_To_End
returns true, it releases the current locomotor and restores the piggybacked one:
```c
ILocomotion* loco = this->Locomotor;  // FootClass+0x674
IPiggyback* piggy = NULL;
loco->QueryInterface(&IID_IPiggyback, &piggy);
if (piggy && piggy->Is_Ok_To_End()) {
    loco->Release();
    this->Locomotor = NULL;
    piggy->End_Piggyback(&this->Locomotor);  // restores piggybacked loco
}
```

---

## 20. Locomotor CLSID Reference

| CLSID | Address | Name |
|-------|---------|------|
| {4A582741-...} | 0x7E9A30 | DriveLocomotionClass |
| {4A582742-...} | 0x7E9A40 | WalkLocomotionClass |
| {4A582743-...} | 0x7E9A50 | HoverLocomotionClass |
| {4A582747-...} | 0x7E9A90 | TeleportLocomotionClass |

---

## Ghidra Labels Applied This Session

### Functions Renamed:
| Address | Name |
|---------|------|
| 0x4AF8E0 | DriveLocomotionClass__Begin_Piggyback |
| 0x4AF930 | DriveLocomotionClass__End_Piggyback |
| 0x4AF970 | DriveLocomotionClass__Is_Ok_To_End |
| 0x4AFB80 | DriveLocomotionClass__ILocomotion_Is_Moving |
| 0x4AF610 | DriveLocomotionClass__Piggybacker_CLSID |
| 0x740810 | UnitClass__Mission_Guard_Harvester |

### Labels Created:
| Address | Label |
|---------|-------|
| 0x7E9A30 | CLSID_DriveLocomotion |
| 0x7E9A40 | CLSID_WalkLocomotion |
| 0x7E9A50 | CLSID_HoverLocomotion |
| 0x7E9A90 | CLSID_TeleportLocomotion |

---

## 21. Definitive Teleport Trigger Mechanism (v3 — verified)

> **UPDATE (2026-07-19, verify-doc-fix-swarm slot 4).** The 2026-07-18 flag below correctly
> identified the FindFirstUnit/FindFirstBuilding mislabel but left the actual mechanism
> UNVERIFIED. [docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) (this session)
> closes most of that gap and REFUTES this section's title claim -- there is no single
> "definitive" trigger at the Set_Destination level. Re-verified live this session:
> `get_function_by_address 0x0047eba0` = `CellClass__FindFirstUnit` (0047eba0-0047ebe2);
> `get_xrefs_to 0x00719400` = zero references (spurious mid-function label some docs cite as
> "InitiateWarp" -- entirely inside `StateMachineTick`'s body, not a real function, do not
> cite); `get_function_by_address 0x007192f0` = `TeleportLocomotionClass__StateMachineTick`
> (007192f0-00719bed), the real per-tick `ILocomotion::Process` (vtable +0x40, matches
> `DriveLocomotionClass::Process` at the same offset); `get_function_by_address 0x00718b70`
> = `TeleportLocomotionClass__Process` (00718b70-007192bd, body ends exactly where
> StateMachineTick begins) -- a separate function, called only synchronously from
> `HeadToCoord`, NOT the per-tick Process despite its Ghidra name.
>
> The warp fires inside `StateMachineTick` when `ChronoInTransit==0 && WarpPhase==0 &&
> Is_Moving()`; `Is_Moving` is set ONLY by Teleport's own `HeadToCoord` (vtable+0x44), and
> `Set_Destination_Internal` (0x4D94B0) dispatches `HeadToCoord` only to whichever locomotor
> is ALREADY active. So "the destination cell is empty" is necessary for Teleport to be
> selected as active by the Set_Destination predicate, but is NOT sufficient for the warp to
> fire -- and this section's own "Complete Call Chain (Harvest Return)" below is WRONG as
> written: it attributes the warp to Mission_Harvest state 2's fallback Set_Destination call,
> which is proven (§1.5 of the WARP_TRIGGER report, re-verified via a fresh
> `decompile_function 0x0073E5E0`) to always have NavCom==NULL at that call site -- forcing
> the "prefer Drive" default branch, never the Teleport-stays-active branch. That call cannot
> arm a warp regardless of distance. The exact call that supplies a DockUnload-flagged
> building as the OLD NavCom (the actual precondition) is OPEN -- see WARP_TRIGGER report §2/§6
> (candidates: Mission_Enter's CAN_DOCK reassert 0x4D9290, `Receive_Radio` case 0x12 at
> 0x4D8FB0, the Dock=-list re-target block inside 0x741970 itself). Treat "Why This Works for
> Harvest Return," "The Complete Call Chain (Harvest Return)," and "When the Chrono Miner
> DRIVES Instead" below as SUPERSEDED pending that open item.
>
> **Prior flag (2026-07-18, verify-doc-fix-swarm w2 slot 4) — needs re-investigate.**
> The central claim of this section — that the Set_Destination decision at 0x7423CD hinges
> on "`CellClass::FindFirstBuilding` returns NULL/non-NULL at the destination cell" — is
> WRONG. Live decompile of 0x47EBA0 (`decompile_function 0x47eba0`, cross-checked with
> `get_function_by_address 0x47eba0`) shows:
> - The function is currently named **`CellClass__FindFirstUnit`** in the live Ghidra
>   project (not "FindFirstBuilding" — that label was this doc's own v1-era guess, recorded
>   in Section 24's label table, and has since been superseded).
> - It walks the cell's object list (`+0xE4` FirstObject, or `+0xE8` BridgeObjects when a
>   second argument is nonzero) and returns the first object whose `WhatAmI()` (vtable+0x2C)
>   equals **1**, not an object whose `WhatAmI()` equals **6** (`BuildingClass::WhatAmI`,
>   verified by `decompile_function 0x459ec0` -> `return 6;`). Whatever RTTI value 1
>   represents, it is not the Building check the doc assumes.
> - Re-reading the full `TechnoClass::Set_Destination` decompile (`decompile_function
>   0x741970`): the call to this function at the address the doc cites (0x7424D7) is nested
>   **inside** a branch that has *already* established the navigation target itself is a
>   Building (`WhatAmI()==6` on `FootClass::GetDestination()`'s result) — it is not a
>   standalone "does this cell contain any building" gate. The FindFirstUnit call there is
>   one of several additional conditions (alongside a `BuildingTypeClass+0x16B3` flag and an
>   AbstractType-0xB check on a second object) gating whether the Teleport locomotor stays
>   active for a move onto that building. None of those additional conditions are captured
>   in this section's (or Section 14/15/23/24's) narrative.
>
> This affects the "empty cell -> teleport, occupied cell -> drive" framing repeated across
> Sections 14, 15, 21, 23, and 24, and the `0x47EBA0 | CellClass__FindFirstBuilding` row in
> Section 24's label table. Per CLAUDE.md's STRUCTURAL-RED-STOP rule, a full mechanism
> rewrite is NOT attempted here — the actual full decision tree (what `+0x16B3` gates, what
> the AbstractType-0xB check is, and what governs a teleport to a plain ground cell with no
> destination-target object at all) needs a bounded `/re-investigate` pass on
> `TechnoClass::Set_Destination` (0x741970) and `CellClass::FindFirstUnit` (0x47EBA0) before
> this section can be corrected in full. Treat the play-by-play below as UNVERIFIED pending
> that pass; only the struct/vtable offsets it also cites (independently verified elsewhere
> in this doc) remain trustworthy.

Previous sections had conflicting claims about whether the chrono miner drives or teleports
when returning from ore. This section resolves the conflict definitively.

### Answer: The Chrono Miner DOES Teleport

The warp is genuine — the unit disappears from the ore field and reappears near the refinery.
The mechanism is in `TechnoClass::Set_Destination` (0x741970), which decides whether to
piggyback DriveLocomotionClass or leave TeleportLocomotionClass active.

### The Decision Point: TechnoClass::Set_Destination (0x7423CD)

When Set_Destination is called on a unit with `Teleporter=yes` (TypeClass+0xCD4):

```
1. Check: is unit Teleporter AND not in transit AND not deploying?
   If no: skip Teleporter block entirely

2. Check: does destination cell contain a unit? [corrected 2026-07-19: was
   "CellClass::FindFirstBuilding" -- get_function_by_address 0x0047eba0 confirms
   CellClass__FindFirstUnit, checking WhatAmI()==1, not a building check]
   CellClass::FindFirstUnit(dest_cell) at 0x47EBA0

3. IF a unit is found on destination cell:
   → Piggyback DriveLocomotionClass over TeleportLoco
   → Unit DRIVES

4. IF no unit on destination cell:
   → Skip the Drive piggyback
   → TeleportLocomotionClass remains the active locomotor
   → Unit warps IF AND ONLY IF Teleport's own HeadToCoord is subsequently invoked with
     Is_Moving armed -- see correction box; this pseudocode alone does not prove that
```

### Why This Works for Harvest Return [SUPERSEDED 2026-07-19 -- see correction box]

Mission_Harvest state 2 computes a **docking cell adjacent to the refinery** (using
`BuildingTypeClass->DockOffset` at +0x1618/+0x161C). This cell is next to the refinery
building, NOT on it — so `FindFirstUnit` [corrected 2026-07-19: was "FindFirstBuilding"]
returns NULL, the Drive swap would be skipped for THIS specific call -- but per
[CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) §1.5, this specific Set_Destination call
(Mission_Harvest state 2's fallback branch) always has NavCom==NULL, which forces the
Teleporter predicate's default "prefer Drive" branch before this check is even reached.
The claim that this call "triggers the warp" is REFUTED.

### The Complete Call Chain (Harvest Return) [SUPERSEDED 2026-07-19 -- see correction box]

```
UnitClass::Mission_Harvest (0x73E5E0), state 2
  → Find_Docking_Bay (0x4DF040): locates nearest refinery
  → Computes dock-adjacent cell from BuildingType->DockOffset
  → Calls TechnoClass::Set_Destination (0x741970)
    → Teleporter block at 0x7423CD:
      → NavCom==NULL at this call site (proven, WARP_TRIGGER report §1.5) -> predicate
        defaults to "prefer Drive" -- FindFirstUnit(dock_cell) result is moot here
      → [ORIGINAL CLAIM, REFUTED: "FindFirstBuilding(dock_cell) returns NULL (empty cell)
        -> SKIP Drive piggyback" -- this does not happen for this specific call]
    → Falls through to FootClass::Assign_Destination (0x4D94B0)
      → Calls active loco->Head_To_Coord (vtable+0x44)
      → Active loco IS TeleportLocomotionClass
      → TeleportLocomotionClass::Head_To_Coord (0x718100)
        → Calls Process (0x718B70) to validate destination
        → Sets IsMoving = 1 (base+0x34)
        → Stores DestCoord and HeadToCoord

[Next game tick:]
TeleportLocomotionClass::StateMachineTick (0x7192F0)
  → Phase 0: detects IsMoving==1, current pos != dest
  → Spawns WarpAway anim at departure
  → Calculates chrono delay from distance
  → Sets BeingWarped (+0x271) = 1
  → Moves unit to destination (instant, single tick)
  → Spawns WarpAway anim at arrival
  → Plays ChronoOut/ChronoIn sounds
  → Sets timer with chrono delay duration
  → Unit appears at destination, drawn 50% translucent

[Subsequent ticks:]
  → Pre-phase check: BeingWarped==1, WarpPhase==0, PendingWarpPhase==0
  → Calls TimerCheck (0x719BF0) each tick
  → When timer expires: clears BeingWarped, unit fully opaque
  → FootClass::AI detects Is_Ok_To_End==true
  → End_Piggyback restores DriveLocomotionClass (if piggybacked)
  → Unit can now drive to the actual dock pad
```

### When the Chrono Miner DRIVES Instead

If the miner is **close** to the refinery (distance <= ChronoHarvTooFarDistance * 256),
Mission_Harvest state 2 takes a different path: it radios the refinery to reserve a dock
slot (`RadioClass::Transmit_Radio`, message RADIO_DOCKING=2), and if accepted, transitions
to state 3 which queues `Mission_Enter` (mission 7). Mission_Enter calls Set_Destination
with the refinery cell itself (which HAS a unit docked/present), so FindFirstUnit
[corrected 2026-07-19: was "FindFirstBuilding"] returns non-NULL, Drive is piggybacked,
and the unit drives to dock. This outcome (drives, close case) is unaffected by the
2026-07-19 correction -- only the "far case warps via this call chain" claim above is
refuted.

### IsMoving Writer — SOLE Location

`TeleportLocomotionClass::Head_To_Coord` (0x718100) at address 0x7181DB is the **only**
function in the entire binary that sets IsMoving=1 (`MOV byte ptr [ESI+0x30], 1`) on
TeleportLocomotionClass. Verified by byte-pattern search across all of gamemd.exe.

### Confidence: 95%

Verified from: Set_Destination decompilation, IsMoving byte-pattern search, Head_To_Coord
xref tracing, and state machine Phase 0 decompilation. The two parallel research agents
converged on the same conclusion from different approaches (forward trace from
Mission_Harvest, backward trace from IsMoving writes).

---

## 22. Self-Teleport Visual Effect (v3 — verified)

### Position Change is Instant (Single Tick)

Phase 0 of the state machine performs the ENTIRE position change in one frame:
- Spawns WarpAway anim at departure
- Moves unit to destination (updates Location, marks/unmarks cells)
- Spawns WarpAway anim at arrival
- Plays both ChronoOut and ChronoIn sounds

The unit is at the destination after one tick. No multi-frame fade-out for self-teleport.

### Visual: Binary 50% Translucency (Not Gradual)

While `BeingWarped` (+0x271) is true, `TechnoClass__Draw` (0x706640) adds flag `0x2004`
to the draw flags, producing **50% translucency**. This is a binary on/off effect —
the unit appears at the destination semi-transparent for the chrono delay duration,
then snaps to full opacity when the timer expires. There is no gradual 0%→100% ramp.

**Re-confirmed 2026-07-18 (verify-doc-fix-swarm w2 slot 4):** `decompile_function 0x706640`
shows `TechnoClass__Draw` calling `(**(code**)(*param_1 + 0x1d4))()` then
`(**(code**)(*param_1 + 0x1d8))()` and OR-ing `0x2004` (or `0x2006` for a building-type
exception) into the draw flags when either call returns true. `get_xrefs_to 0x70c5b0` /
`0x70c5c0` show these vtable slots (`+0x1D4`/`+0x1D8`) are populated with
`TechnoClass__IsWarpingOut`/`TechnoClass__IsBeingWarped` in six per-class vtables (AircraftClass
confirmed directly: `get_xrefs_to 0x7e22a4` -> `AircraftClass__Constructor`; slot bytes
verified via `read_memory 0x7e2470` length 32). So `+0x271` DOES reach `TechnoClass::Draw`,
just through virtual dispatch rather than a direct call — `get_function_callers` on
`TechnoClass__IsBeingWarped` correctly reports zero *direct-call* callers (it is only ever
reached via the vtable slot), which is not the same as "never read." The exact `0x2004`
draw-flag-to-alpha-percentage mapping (the "50%" figure) was not independently re-derived
this pass and remains as originally sourced.

### The "Fade" People Remember is the Temporal Weapon

The smooth visual fade associated with chrono units is actually from the **Temporal weapon**
(Chrono Legionnaire's erasing beam on its TARGET), not from teleport movement:

- `TechnoClass__UpdateTemporalVisual` (0x70E5A0): 10-phase visual state machine
- Fields: +0x198 (timer start), +0x1A0 (timer duration), +0x1A4 (visual phase 0-10)
- `TechnoClass__ScaleByTemporalVisualPhase` (0x70E380): per-frame intensity scaling
- Each phase has different durations (6, 4, ~20, 8, 16, variable, variable, 6, 4, 20 frames)
- Produces smooth mathematical fade curves via switch on phase value

This is set on the TARGET by `TemporalClass__InitiateWarp` (0x71AF20), which sets
`IsWarpingOut` (+0x270) = 1 on the victim. Completely unrelated to locomotor teleport.

### Chronosphere Path IS Multi-Phase

For units warped by the Chronosphere superweapon, the visual process IS gradual:
- Phase 0: Sets `IsWarpingOut` (+0x270) = 1. Timer = 60 frames (4 seconds at 15fps)
- Phase 1: TimerCheck waits. Unit drawn translucent at departure for 60 frames
- Phase 2: Actual position move (instant)
- Phases 3-7: Post-warp timer, validation, cleanup

---

## 23. Corrections to Prior Sections

1. **Section 14 (Mission_Harvest State 2)**: The "QueueMission(2, dock)" call is actually
   `RadioClass::Transmit_Radio(RADIO_DOCKING=2, dock)` (0x65AAA0) — a dock reservation
   protocol, not a mission queue. vtable+0x278 = RadioClass::Transmit_Radio.

2. **Section 14 claim "chrono miner always drives"**: still considered INCORRECT (the miner
   can teleport), but the mechanism this item originally proposed ("skips Drive piggyback
   when distance > ChronoHarvTooFarDistance, because the destination cell has no building")
   is REFUTED, not just unverified.
   [updated 2026-07-19: [CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/CHRONO_MINER_WARP_TRIGGER_GHIDRA_REPORT.md) §1.5 proves Mission_Harvest
   state 2's fallback Set_Destination call always has NavCom==NULL, which forces the
   Teleporter predicate's "prefer Drive" default -- that call cannot arm a warp regardless of
   distance. If/when the miner does warp on return, current evidence points to a short
   final-approach hop (staging cell -> accepted dock pad) gated by a still-OPEN condition
   (which call supplies a DockUnload-flagged building as the OLD NavCom), not a single
   long-range "far distance" trigger. See Sections 14 and 21 correction boxes.]
   [flag added 2026-07-18, superseded above: the "destination cell has no building" test
   itself is unverified as stated -- Section 21's correction flag shows the actual gate is
   `CellClass::FindFirstUnit` (WhatAmI()==1) nested inside a building-target branch, not a
   direct FindFirstBuilding/WhatAmI()==6 check.]

3. **+0x218 (FUN_0070C610)**: Renamed from "SetWarpVisualState" to `TechnoClass__SetGhostCell`.
   Stores a CellClass pointer for building deployment ghost rendering. Not warp-related.

---

## 24. All Ghidra Labels (Cumulative)

### Functions Renamed (all sessions combined)
| Address | Name |
|---------|------|
| 0x4AF8E0 | DriveLocomotionClass__Begin_Piggyback |
| 0x4AF930 | DriveLocomotionClass__End_Piggyback |
| 0x4AF970 | DriveLocomotionClass__Is_Ok_To_End |
| 0x4AFB80 | DriveLocomotionClass__ILocomotion_Is_Moving |
| 0x4AF610 | DriveLocomotionClass__Piggybacker_CLSID |
| 0x4D55F0 | FootClass__Head_To_Coord_Dispatch |
| 0x4D94B0 | FootClass__Assign_Destination |
| 0x4DA530 | FootClass__AI |
| 0x47EBA0 | ~~CellClass__FindFirstBuilding~~ -- WRONG, see Section 21 correction flag (2026-07-18): live Ghidra project names this `CellClass__FindFirstUnit`; decompile shows it checks cell contents for `WhatAmI()==1`, not a building (`BuildingClass::WhatAmI` returns 6, verified `decompile_function 0x459ec0`) |
| 0x65AAA0 | RadioClass__Transmit_Radio |
| 0x65A970 | RadioClass__Transmit_Radio_Impl |
| 0x65AD30 | FootClass__GetDestination |
| 0x70C5B0 | TechnoClass__IsWarpingOut |
| 0x70C5C0 | TechnoClass__IsBeingWarped |
| 0x70C5F0 | TechnoClass__IsNotWarping |
| 0x70C610 | TechnoClass__SetGhostCell |
| 0x70E000 | TechnoClass__ApplyTemporalDamage |
| 0x70E380 | TechnoClass__ScaleByTemporalVisualPhase |
| 0x70E4B0 | TechnoClass__ScaleByWarpInVisualPhase |
| 0x70E5A0 | TechnoClass__UpdateTemporalVisual |
| 0x70E920 | TechnoClass__UpdateGapVisual |
| 0x706640 | TechnoClass__Draw |
| 0x718080 | TeleportLocomotionClass__Is_Moving |
| 0x7180A0 | TeleportLocomotionClass__Destination |
| 0x718100 | TeleportLocomotionClass__HeadToCoord |
| 0x718230 | TeleportLocomotionClass__Stop_Moving |
| 0x718260 | TeleportLocomotionClass__Update_Position |
| 0x718B70 | TeleportLocomotionClass__Process |
| 0x7187A0 | TeleportLocomotionClass__PostWarpValidation |
| 0x7192F0 | TeleportLocomotionClass__StateMachineTick |
| 0x719BF0 | TeleportLocomotionClass__TimerCheck |
| 0x719E90 | TeleportLocomotionClass__Begin_Piggyback |
| 0x719EE0 | TeleportLocomotionClass__End_Piggyback |
| 0x719F30 | TeleportLocomotionClass__Is_Ok_To_End |
| 0x71A100 | TeleportLocomotionClass__Is_Piggybacking |
| 0x71AF20 | TemporalClass__InitiateWarp |
| 0x71ABC0 | TemporalClass__DetachFromTarget |
| 0x71ACD0 | TemporalClass__ClearWarpingOutOnTarget |
| 0x73E5E0 | UnitClass__Mission_Harvest |
| 0x740810 | UnitClass__Mission_Guard_Harvester |
| 0x741970 | TechnoClass__Set_Destination |
| 0x4DF040 | FootClass__Find_Docking_Bay |

### Global Labels
| Address | Label |
|---------|-------|
| 0x7E9A30 | CLSID_DriveLocomotion |
| 0x7E9A40 | CLSID_WalkLocomotion |
| 0x7E9A50 | CLSID_HoverLocomotion |
| 0x7E9A90 | CLSID_TeleportLocomotion |
| 0x819088 | IID_IPiggyback |
| 0xB0EBF8 | g_NullCoord |
| 0xB0EC38 | g_BridgeZOffset_Teleport |
| 0xA8ED84 | g_CurrentFrameCounter |
| 0x8871E0 | g_RulesClass_Instance |
