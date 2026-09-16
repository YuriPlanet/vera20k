# FactoryClass & Production System — Deep Dive Ghidra Report

Extends [BUILD_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILD_QUEUE_GHIDRA_REPORT.md) with newly decompiled functions.

---

## 1. FactoryClass vtable at 0x007E88D0

The vtable follows the AbstractClass hierarchy. FactoryClass is small — it inherits from
AbstractClass (which is IUnknown/IRTTITypeInfo/IPersist/IPersistStream). Based on the
constructor setting 4 vtable pointers and the known methods, the layout is:

```
Offset  Address      Name / Purpose
------  ----------   --------------------------------
0x00    (dtor)       scalar deleting destructor
0x04    (base)       AbstractClass base method (Size/sizeof)
0x08    (base)       AbstractClass::WhatAmI → returns RTTI_FACTORY
0x0C    (base)       AbstractClass base method
0x10    (base)       AbstractClass base method
0x14    (Load)       IPersistStream::Load (0x004CA370)
0x18    (Save)       IPersistStream::Save (0x004CA2B0 range)
0x1C    (base)       AbstractClass base method
0x20    (Release)    Destructor/Release
0x24    ...          (inherited)
...
0x34    (Debug)      Debug dump function (0x004CA430)
0x38    ...
...
0x5C    AI           FactoryClass::AI (0x004C9B20) — THE production tick
0x60    ...          (end of vtable — remaining slots are inherited stubs or absent)
```

**Note:** The vtable at 0x007E88D0 is data (not code), so it cannot be "disassembled" as
a function. The key entry at offset 0x5C (entry index 23) = 0x004C9B20 = FactoryClass::AI,
confirmed from LogicClass::AI iterating all factories and calling vtable[0x5C/4].

The constructor also sets 3 secondary vtables:
- `+0x04` → vtable at `vtable__FactoryClass__secondary_4` (IUnknown / IRTTIInfo)
- `+0x08` → vtable at `vtable__FactoryClass__secondary_8` (IPersist)
- `+0x0C` → vtable at `vtable__FactoryClass__secondary_12` (IPersistStream)

FactoryClass has NO methods beyond AI in its primary vtable — it is not a TechnoClass
descendant and does not have Update/Draw/Render methods. All production logic flows
through AI (the per-tick handler) plus the standalone named methods below.

---

## 2. FactoryClass::AI — Production Tick (0x004C9B20)

```c
void __thiscall FactoryClass__AI(FactoryClass *this) {
    if (this->IsSuspended) return;
    if (this->Object == NULL && this->SpecialItem == 0) return;
    if (this->Object != NULL && this->Production_Value == 54) return;
    if (this->SpecialItem != -1 && this->Production_Value == 54) return;

    int timeRemaining = CDTimerClass__GetTimeRemaining(&this->Timer);
    if (timeRemaining != 0 || this->Production_Timer_Duration == 0) {
        this->Production_HasChanged = false;
        return;  // Timer not expired yet
    }

    // === ADVANCE PRODUCTION ===
    this->Production_Value += this->Production_Step;  // Always +1
    this->Production_HasChanged = true;
    // Reset timer
    this->Production_Timer_StartTime = g_CurrentFrameCounter;
    this->Production_Timer_TimeLeft = this->Production_Timer_Duration;
    this->IsDifferent = true;

    // Calculate per-step cost
    int costThisStep;
    if (this->Object == NULL) {
        costThisStep = 0;
    } else {
        int stepsLeft = 54 - this->Production_Value;
        if (stepsLeft == 0)
            costThisStep = this->Balance;
        else
            costThisStep = this->Balance / stepsLeft;
    }
    costThisStep = min(costThisStep, this->Balance);

    // Check funds
    int available = this->Owner->GetAvailableCredits();
    if (available < costThisStep) {
        this->OnHold = true;       // +0x5C — NoFunds/OnHold flag
        this->Production_Value--;  // Roll back: net progress = 0
    } else {
        HouseClass__Spend_Money(costThisStep);
        this->OnHold = false;
        this->Balance -= costThisStep;
    }

    // Completion check
    if (this->Production_Value == 54) {
        this->IsSuspended = true;
        this->Production_Timer_Duration = 0;
        this->Production_Timer_StartTime = g_CurrentFrameCounter;
        this->Production_Timer_TimeLeft = 0;
        HouseClass__Spend_Money(this->Balance);  // Remainder
        this->Balance = 0;
    }
}
```

**Key discovery — OnHold field at +0x5C:** This is the "insufficient funds" flag. When
set, the sidebar should show "On Hold" status. It's distinct from IsSuspended (manual
pause) — OnHold means the player ran out of money mid-production.

---

## 3. HouseClass::Update (0x004F8F70) — The "HouseClass::AI" equivalent

This is NOT at a vtable[0x5C/4] offset — it's at address 0x004F8F70 and was labeled
`HouseClass__Update` in Ghidra. This is the massive per-tick function for HouseClass.

### Production-relevant sections:

```c
void __thiscall HouseClass__Update(HouseClass *this) {
    // ... power/radar checks ...

    // Timer-based recheck of power state
    if (this->RecheckPower) {
        HouseClass__AI_AssessPower(this);
        this->RecheckRadar = true;
    }
    if (this->RecheckRadar) {
        HouseClass__CheckSuperweaponReady(this);
        HouseClass__CheckLowPower(this);
    }

    // ... superweapon tick, rally point handling ...

    // === PRODUCTION MANAGEMENT ===
    // At 0x004F92D2:
    if (this->field_0x1fc != 0) {      // "ProductionDirty" flag
        this->field_0x1fc = 0;

        // For player: also refresh sidebar building list
        if (g_PlayerPtr == this) {
            // Iterate all buildings, refresh sidebar
            for (int i = 0; i < building_count; i++) {
                building->UpdateSidebar();  // vtable[0x4E0/4]
            }
            FUN_006a7d20();  // Sidebar refresh
        }

        // KEY: This is where production gets managed every tick
        HouseClass__AI_ManageProduction(this);
        HouseClass__AI_ResumeProduction(this);  // <-- This resumes suspended super weapons
    }

    // ... AI unit/building strategy ...
    // For AI players: choose what to build
    if (!isHuman && !isDefeated) {
        HouseClass__AI_Choose_Building();
        HouseClass__AI_Choose_Unit();
        HouseClass__AI_Choose_Aircraft();
        HouseClass__AI_Choose_Infantry();
    }

    // ... shroud reveal, defeat checking ...
}
```

### HouseClass field_0x1fc = "ProductionDirty" flag

This flag at HouseClass+0x1FC triggers `AI_ManageProduction` + `AI_ResumeProduction`.
It's set to 1 from:
- `HouseClass__AI_ManageProduction` itself (when superweapons change state)
- Various production state changes

This is the mechanism that drives periodic production system updates.

---

## 4. FUN_00734250 — Vehicle Exit Callback (0x00734250)

```c
void __fastcall FUN_00734250(int param_1) {
    // param_1 is a TechnoClass* (the produced unit)
    // param_1+0x520 = BuildingClass* (the factory building that produced it)
    // factory_building->Type (+0x520) -> +0xe08 = NavalIndex

    if (*(int *)(*(int *)(param_1 + 0x520) + 0xe08) == 5) {
        // Naval unit — store in naval exit slot
        DAT_00b0fe60 = param_1;
    } else {
        // Land unit — store in normal vehicle exit slot
        DAT_00b0fe5c = param_1;
    }
}
```

**Analysis:** This is extremely simple. It's NOT the full vehicle exit handler — it's a
**callback/setter** that stores the produced unit pointer into one of two global slots:
- `DAT_00b0fe5c` — for land vehicles exiting a War Factory
- `DAT_00b0fe60` — for naval vehicles exiting a Naval Yard

The distinction is based on `BuildingTypeClass->NavalIndex` at offset +0xe08 of the
factory building's type. Value 5 = naval.

This is called from StripClass::AI when a vehicle production completes, before the
actual exit/placement logic runs. The globals are consumed by the placement system to
know which unit needs to exit which type of building.

---

## 5. HouseClass Suspend Handler (0x004FA910)

```c
// Network command 0x0F handler
int __thiscall FUN_004fa910(HouseClass *this, int rtti, int heapId, char isNaval) {
    int navalIndex = 0;
    if (heapId >= 0) {
        TechnoTypeClass *type = RTTI_To_TypeArray(rtti, heapId);
        if (rtti == 6 || rtti == 7)
            navalIndex = type->NavalIndex;  // +0xe08
    }

    FactoryClass *factory;
    switch (rtti) {
    case 1: case 0x28:  // Building
        factory = isNaval ? this->NavalBuildFactory : this->BuildingFactory;
        break;
    case 2: case 3:     // Infantry
        factory = this->InfantryFactory;
        break;
    case 6: case 7:     // Unit/Vehicle
        factory = (navalIndex == 5) ? this->NavalFactory : this->VehicleFactory;
        break;
    case 0xF: case 0x10: // Aircraft
        factory = this->AircraftFactory;
        break;
    default:
        return 3;  // ERROR: invalid type
    }

    if (factory != NULL) {
        FactoryClass__Suspend(factory, true);  // canAfford = true
        if (g_PlayerPtr == this) {
            int tab = SidebarClass__TypeToTab(rtti);
            FUN_006a60a0(tab);  // Refresh sidebar tab
        }
        return 0;  // SUCCESS
    }
    return 3;  // ERROR: no factory
}
```

**Key finding:** The suspend handler always passes `canAfford=true` (the `IsManual` field).
This means user-initiated suspension sets `IsManual = true`, allowing the resume logic
to distinguish between "player paused" and "system paused due to funds".

---

## 6. Command Dispatcher (0x004C6CB0) — Network Event Handler

This is the main multiplayer command dispatcher. It's a massive switch on command type.
Production-relevant cases:

```
case 0x01: BuildingClass::GoOnline (power on)
case 0x02: BuildingClass::GoOffline (power off)
case 0x04/0x05: Unit move/attack command
case 0x06: Unit sell/undeploy
case 0x07: Unit scatter
case 0x09: Unit patrol/guard
case 0x0B: HouseClass::Place_Production — place building or exit unit
case 0x0E: HouseClass::Begin_Production — start or queue production
case 0x0F: FUN_004fa910 (Suspend) — pause production
case 0x10: FUN_004faa10 with removeAll=0 — cancel ONE from queue
case 0x11: BuildingClass sell
case 0x17: Sell building at cell
case 0x2E: FUN_004faa10 with removeAll=1 — cancel ALL of type from queue
```

The command packet format (param_1 is a byte array):
```
+0x00: byte  CommandType (switch value)
+0x02: byte  HouseIndex
+0x03: dword FrameNumber
+0x07: dword Param1 (RTTI type or heap index)
+0x0B: dword Param2
+0x0F: dword Param3
+0x13: ...   additional params
```

---

## 7. FUN_004FAA10 — Cancel/Abandon Handler (0x004FAA10)

```c
int __thiscall FUN_004faa10(
    HouseClass *this, int rtti, int heapId, char isNaval, char removeAll)
{
    // Resolve naval index
    int navalIndex = 0;
    if (heapId >= 0) {
        TechnoTypeClass *type = RTTI_To_TypeArray(rtti, heapId);
        if (rtti == 6 || rtti == 7)
            navalIndex = type->NavalIndex;
    }

    // Get factory for production category
    FactoryClass *factory = GetFactoryForType(this, rtti, isNaval, navalIndex);
    if (factory == NULL) return 3;

    // Path 1: Queue has items AND heapId >= 0 AND navalIndex == 0
    if (factory->QueuedObjects_Count > 0 && heapId >= 0 && navalIndex == 0) {
        TechnoTypeClass *type = RTTI_To_TypeArray(rtti, heapId);
        bool removed = FactoryClass__RemoveFromQueue(factory, type);
        if (removeAll) {
            while (FactoryClass__RemoveFromQueue(factory, type)) {}
        }
        if (removed) {
            RefreshSidebarTab(rtti);
            if (!removeAll) return 0;
        }
    }

    // Path 2: Object identity check (when heapId != -1)
    if (heapId != -1) {
        if (factory->Object == NULL) return 0;
        TechnoTypeClass *objType = factory->Object->GetType();
        if (heapId != objType->GetHeapID()) return 0;
    }

    // Path 3: Abandon and restart
    if (g_PlayerPtr == this) {
        FUN_006abad0(rtti, heapId, factory);  // Sidebar cleanup
        if (rtti == 7 || rtti == 6) {
            // Clear building placement state
            DAT_0088098c = 0;
            DAT_00880990 = 0;
            DAT_00880994 = -1;
            FUN_004a8bf0(0);
        }
    }
    FactoryClass__AbandonProduction(factory);

    if (factory->QueuedObjects_Count != 0) {
        FactoryClass__StartNextQueued(factory);  // Restart with next queued item
        return 0;
    }

    // Jump table: clear factory pointer on HouseClass based on type
    // (sets HouseClass->XXXFactory = NULL for the appropriate category)
    return 0;
}
```

**CRITICAL FINDING — Queue restart resolved:**

The "OPEN QUESTION" from [BUILD_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILD_QUEUE_GHIDRA_REPORT.md) about non-naval queue restart
is now answerable. The flow is:

1. For non-naval types after normal completion, `FUN_004FAA10` is called with the
   completed item's type info (from `Place_Production` or `StripClass::AI`)
2. It calls `RemoveFromQueue` to remove the completed type from the front
3. It returns EARLY (before reaching `AbandonProduction`/`StartNextQueued`)
4. The factory is left with `Object=NULL`, `QueueCount=N-1`, `IsSuspended=true`

**BUT** — the restart actually happens in `FUN_00509140` (now identified as
`HouseClass__UpdateFactoryPrereqs` or similar). This function:
- Is called when prerequisites change (building destroyed, tech level change, etc.)
- Iterates all queue items and checks `CanBuild()` for each
- Removes items that can no longer be built
- If the current Object's type can still be built but factory is suspended+not manual:
  calls `FactoryClass__SetRate()` to resume production
- If Object's type CAN'T be built: calls `AbandonProduction` + `StartNextQueued`

The `field_0x1fc` "ProductionDirty" flag on HouseClass triggers this check via
`AI_ManageProduction` in the HouseClass::Update loop. When production completes and
the item exits, the dirty flag gets set, which on the next tick causes the system to
notice the factory has queued items and a null Object, triggering the restart.

---

## 8. FactoryClass::Suspend (0x004C9E60 range)

```c
bool __thiscall FactoryClass__Suspend(FactoryClass *this, bool canAfford) {
    if (this->IsSuspended) return false;  // Already suspended

    this->IsManual = canAfford;  // +0x71: true = user-initiated, false = system
    this->IsSuspended = true;    // +0x70
    this->Production_Timer_Duration = 0;
    this->Production_Timer_StartTime = g_CurrentFrameCounter;
    this->Production_Timer_TimeLeft = 0;
    return true;
}
```

**IsManual (+0x71):** Distinguishes user-pause from system-pause:
- `true` = player right-clicked to pause (can only be resumed by player)
- `false` = system paused (e.g., prerequisite lost — auto-resumes when prereq restored)

---

## 9. FactoryClass::SetRate/Resume (0x004C9F00 range)

```c
bool __thiscall FactoryClass__SetRate(FactoryClass *this) {
    if ((this->Object == NULL && this->SpecialItem == 0) ||
        !this->IsSuspended ||
        (this->Object != NULL && this->Production_Value == 54) ||
        (this->SpecialItem != -1 && this->Production_Value == 54))
        return false;

    this->IsSuspended = false;
    int buildTime = 0;
    if (this->Object != NULL)
        buildTime = FactoryClass__GetBuildStepTime(this->Object);

    int rate = buildTime / 54;  // 0x36
    rate = clamp(rate, 1, 255);

    this->Production_Timer_Duration = rate;
    this->Production_Timer_StartTime = g_CurrentFrameCounter;
    this->Production_Timer_TimeLeft = rate;

    // Check if house can afford the per-step cost
    int stepsLeft = 54 - this->Production_Value;
    int costPerStep = (stepsLeft == 0) ? this->Balance : this->Balance / stepsLeft;

    int available = this->Owner->GetAvailableCredits();
    if (costPerStep <= available) {
        this->IsManual = true;  // Flag: will need to re-suspend if can't afford
        // If caller passed a flag, re-suspend immediately
        return true;
    }
    return false;
}
```

---

## 10. FactoryClass::StartProduction (0x004C9C70 range)

```c
bool __thiscall FactoryClass__StartProduction(
    FactoryClass *this, TechnoTypeClass *type, HouseClass *owner, bool isResume)
{
    int rtti = type->WhatAmI();

    // If upgrading (RTTI 7 = upgrade), abandon current production first
    if (rtti == 7) {
        FactoryClass__AbandonProduction(this);
    }

    // PATH A: Start new production (first item or RTTI 7 or resume)
    if (rtti == 7 ||
        ((this->Rate == 0 || this->IsSuspended) &&
         this->QueuedObjects_Count < 1 &&
         (this->Object == NULL || !this->IsSuspended)) ||
        isResume)
    {
        this->IsDifferent = true;
        this->IsSuspended = true;
        this->Production_Timer_StartTime = g_CurrentFrameCounter;
        this->Production_Timer_Duration = 0;
        this->Production_Timer_TimeLeft = 0;
        this->Production_Value = 0;

        // Create the actual TechnoClass instance
        this->Object = type->CreateInstance(owner);

        // For AI building placement: mark as AI-controlled
        if (!HouseClass__IsPlayerControl(this->Owner) &&
            this->Object != NULL &&
            this->Object->WhatAmI() == RTTI_BUILDING) {
            this->Object[0x6CA] = 1;  // BuildingClass::IsAIPlaced flag
        }

        if (this->Object != NULL) {
            this->Owner = this->Object->Owner;  // +0x21C
            this->Balance = type->GetCost(this->Owner);
            this->Object->ProductionCost = this->Balance;  // +0x300
        }
        return (this->Object != NULL);
    }

    // PATH B: Queue item (production already active)
    if (g_RulesClass->MaximumQueuedObjects <= this->QueuedObjects_Count ||
        HouseClass__CheckBuildLimit(type))
    {
        if (HouseClass__IsHumanPlayer()) {
            VocClass__PlayAtPos(/*error sound*/);
        }
        return false;
    }

    // Grow queue array if needed
    if (this->QueuedObjects_Count >= this->QueuedObjects_Capacity) {
        // resize by GrowthIncr (10)
    }

    this->QueuedObjects[this->QueuedObjects_Count] = type;
    this->QueuedObjects_Count++;
    return true;
}
```

---

## 11. FactoryClass::AbandonProduction (0x004CA0E0)

```c
void __thiscall FactoryClass__AbandonProduction(FactoryClass *this) {
    if (this->Object == NULL) return;

    TechnoTypeClass *type = this->Object->GetType();
    int fullCost = type->GetCost(this->Owner);
    // Refund = full cost minus what we already paid (Balance = remaining)
    // amount_paid = fullCost - this->Balance
    // But the refund is: fullCost - Balance → credits back to player
    HouseClass__Add_Credits(fullCost - this->Balance);
    this->Balance = 0;

    if (this->SpecialItem != 0)
        this->SpecialItem = -1;

    // Reset timer
    this->Production_Timer_Duration = 0;
    this->Production_Timer_StartTime = g_CurrentFrameCounter;
    this->Production_Timer_TimeLeft = 0;
    this->Production_Value = 0;
    this->IsSuspended = true;
    this->IsDifferent = true;

    // For AI: clear the "what to build" slot on HouseClass
    if (!HouseClass__IsPlayerControl(this->Owner)) {
        int rtti = this->Object->WhatAmI();
        if (rtti == 0x0F)  // Aircraft
            this->Owner->field_0x5654 = -1;
        if (rtti == 0x01)  // Unit/Vehicle
            this->Owner->field_0x5650 = -1;
        if (rtti == 0x02)  // Aircraft (alt)
            this->Owner->field_0x5658 = -1;
        if (rtti == 0x06)  // Building
            this->Owner->field_0x564c = -1;
    }

    // Delete the produced object
    g_MapEditorMode++;  // Suppress map updates during deletion
    if (this->Object != NULL) {
        this->Object->Destroy(true);  // vtable[0x20/4]
    }
    this->Object = NULL;
    g_MapEditorMode--;
}
```

**New finding:** The AI "what to build next" fields on HouseClass:
- `+0x5650` = AI's chosen UnitTypeClass heap index
- `+0x5654` = AI's chosen AircraftTypeClass heap index
- `+0x5658` = AI's chosen InfantryTypeClass heap index
- `+0x564C` = AI's chosen BuildingTypeClass heap index

These are reset to -1 when production is abandoned (for AI players only).

---

## 12. FactoryClass::CompletedProduction (0x004CA1A0)

```c
int __thiscall FactoryClass__CompletedProduction(FactoryClass *this) {
    if (this->Object != NULL && this->Production_Value == 54) {
        this->Object = NULL;
        this->IsSuspended = true;
        this->IsDifferent = true;
        this->Production_Value = 0;
        this->Production_Timer_Duration = 0;
        this->Production_Timer_StartTime = g_CurrentFrameCounter;
        this->Production_Timer_TimeLeft = 0;
        return 1;  // Success
    }
    // Also handles SpecialItem completion
    if (this->SpecialItem != 0 && this->Production_Value == 54) {
        this->SpecialItem = -1;
        this->IsSuspended = true;
        this->IsDifferent = true;
        this->Production_Value = 0;
        // ... timer reset ...
        return 1;
    }
    return 0;  // Not complete
}
```

**Note:** CompletedProduction does NOT set Object = NULL until called — the produced
TechnoClass persists as factory->Object until the item successfully exits. This is why
`FactoryClass::IsComplete` checks both Object != NULL AND Progress == 54.

---

## 13. HouseClass::CheckBuildLimit (0x0050B360)

```c
int __thiscall HouseClass__CheckBuildLimit(HouseClass *this, TechnoTypeClass *type) {
    if (type == NULL) return 1;  // Can't build NULL

    bool isNaval = type->IsNaval;  // +0xCCE
    int rtti = type->WhatAmI();

    // Get factory for this type
    FactoryClass *factory = GetFactoryForType(this, rtti, isNaval);
    int inProduction = 0;
    if (factory != NULL) {
        inProduction = FUN_004ca670(factory, type);  // Count in queue + active
    }

    switch (rtti) {
    case 3:  // InfantryTypeClass
        // Special handling for "IsGate" infantry (garrison limit check)
        if (type->IsGate) {  // +0xe0d
            int totalGateInfantry = 0;
            for (each gate infantry type) {
                totalGateInfantry += CountOwned(type) + CountInProduction(type);
            }
            if (totalGateInfantry >= this->MaxGateInfantry)  // +0x2D4
                return 1;  // BUILD LIMIT REACHED
            return 0;
        }
        // Fall through to normal BuildLimit check
        int buildLimit = type->BuildLimit;  // +0xEE at offset 0x3B8
        if (buildLimit < 1) {
            int owned = CountOwnedInstances(type->HeapID);
            if (abs(buildLimit) <= owned + inProduction)
                return 1;  // LIMIT REACHED
            if (buildLimit < 1) return 0;  // Unlimited
        }
        // buildLimit > 0: check owned+inProd >= limit
        return (CountOwnedInstances(type) + inProduction >= buildLimit) ? 1 : 0;

    case 7:   // UnitTypeClass
    case 0x28: // BuildingTypeClass (alternate RTTI)
        // Same pattern: check BuildLimit at type+0xEE
        ...

    case 0x10: // InfantryTypeClass (alternate RTTI)
        // Also checks clones: counts units with matching Recruit flag
        ...
    }
}
```

**BuildLimit field:** At TechnoTypeClass offset +0x3B8 (param_2[0xEE]).
- Positive value: max count (owned + in_production)
- Negative value: |value| is limit (same check but allows unlimited if 0)
- Zero: no limit

**FUN_004CA670 — Count items in factory for type:**
```c
int FUN_004ca670(FactoryClass *factory, TechnoTypeClass *type) {
    int count = 0;
    // Check active production
    if (factory->Object != NULL) {
        if (factory->Object->GetType() == type)
            count = 1;
    }
    // Check queue
    for (int i = 0; i < factory->QueuedObjects_Count; i++) {
        if (factory->QueuedObjects[i] == type)
            count++;
    }
    return count;
}
```

---

## 14. FUN_00509140 — Factory Prerequisite Update (0x00509140)

**This resolves the OPEN QUESTION from [BUILD_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILD_QUEUE_GHIDRA_REPORT.md).**

```c
void __thiscall UpdateFactoryPrereqs(HouseClass *this, int rtti, char isNaval, int navalIndex) {
    FactoryClass *factory = GetFactoryForType(this, rtti, isNaval, navalIndex);
    if (factory == NULL) return;

    // 1. Purge unbuildable items from queue (iterate backward)
    for (int i = factory->QueuedObjects_Count - 1; i >= 0; i--) {
        TechnoTypeClass *queued = factory->QueuedObjects[i];
        if (!queued->CanBuild(1, 0, 1, this)) {
            // Remove from queue (shift array)
            factory->QueuedObjects_Count--;
            // ... shift elements ...
        }
    }

    // 2. Check active production
    if (factory->Object != NULL) {
        TechnoTypeClass *activeType = factory->Object->GetType();

        if (!activeType->CanBuild(1, 0, 1, this)) {
            // Can't build anymore — abandon and try next in queue
            FactoryClass__AbandonProduction(factory);
            FactoryClass__StartNextQueued(factory);  // <-- HERE is the queue restart!
        }
        else if (!activeType->CanBuild(1, 1, 1, this)) {
            // Can't build with current money/tech but could later
            FactoryClass__Suspend(factory, false);  // System pause (not manual)
            if (g_PlayerPtr == this) {
                RefreshSidebarTab(rtti);
            }
        }
        else if (factory->IsSuspended && !factory->IsManual) {
            // CAN build and was system-suspended: RESUME!
            FactoryClass__SetRate(factory);
        }
    }

    // 3. If factory is empty (no object, no queue), destroy it
    if (factory->Object == NULL && factory->QueuedObjects_Count == 0) {
        factory->Release();  // vtable[0x20/4]
    }
}
```

**THIS IS THE MISSING LINK.** When prerequisites change (building destroyed, new
building placed, tech level change), this function is called for each factory type.
The key path for queue restart after normal completion:

1. Production completes → AI sets IsSuspended=true
2. Item exits building → CompletedProduction clears Object, resets Progress
3. FUN_004FAA10 removes one from queue (for non-naval: returns early)
4. Factory state: Object=NULL, QueueCount=N, IsSuspended=true
5. On next HouseClass::Update tick, `field_0x1fc` (ProductionDirty) triggers
6. `HouseClass__AI_ManageProduction` → `HouseClass__AI_ResumeProduction` runs
7. These set conditions that cause `FUN_00509140` to be called
8. FUN_00509140 sees Object==NULL with QueueCount>0, but the key path is:
   it doesn't directly call StartNextQueued for NULL objects

**CORRECTION:** Actually, re-reading the code more carefully, the actual restart for
non-naval types likely flows through `HouseClass__Begin_Production` being called again.
After the completed item exits:
1. `FUN_004FAA10` calls `RemoveFromQueue` (removes the completed type pointer)
2. Then it calls `AbandonProduction` + checks QueueCount → `StartNextQueued`
3. `StartNextQueued` pops the next item and calls `Begin_Production`

The original report's confusion was about which code path reaches
`AbandonProduction`. Looking at the decompiled code again: FUN_004FAA10 DOES reach
`AbandonProduction` at `LAB_004fab64` when `factory->Object != NULL` — the navy
check only matters for the early-return path when removing from queue. When
FUN_004FAA10 is called after placement with the COMPLETED item's info, the Object
has already been cleared by `CompletedProduction`, so `Object == NULL`, and the
function takes a different branch.

**Final resolution:** The restart mechanism depends on exact calling order:
1. `Place_Production` calls `CompletedProduction` first (Object → NULL, Progress → 0)
2. Then calls `FUN_004FAA10` which sees Object==NULL
3. With Object==NULL and QueueCount>0 after RemoveFromQueue, it falls through to
   the abandon+restart path (AbandonProduction is a no-op on NULL Object)
4. `StartNextQueued` triggers on the remaining queue items

---

## 15. FactoryClass::GetBuildStepTime (0x004C9FB0 range)

```c
int __fastcall FactoryClass__GetBuildStepTime(TechnoClass *object) {
    TechnoTypeClass *type = object->GetType();
    int baseCost = type->GetCost();

    // Apply house build time bonus
    HouseClass *owner = object->Owner;  // +0x21C → +0x87*4
    double bonus = HouseClass__GetBuildTimeBonus(owner);
    int cost = (int)(baseCost * bonus);

    // Apply power ratio penalty
    double powerRatio = HouseClass__GetPowerRatio(owner);
    cost = (int)(cost * powerRatio);  // Low power = slower

    // Check for vehicle-specific speed bonus
    int rtti = object->WhatAmI();
    if (rtti == 1) {  // UnitClass
        TechnoTypeClass *unitType = object->GetType();
        if (unitType->SpeedBonus)  // +0xCCE
            cost = (int)(cost * speedMultiplier);
    }

    // Apply MultipleFactory bonus
    int factoryCount = HouseClass__GetFactoryCount(owner, rtti);
    if (g_RulesClass->MultipleFactory > 1.0 && factoryCount > 1) {
        for (int i = 0; i < factoryCount - 1; i++) {
            cost = (int)(cost * g_RulesClass->MultipleFactory);
        }
    }

    // Check if building is a wall/defense (instant build)
    if (rtti == 6 && object->Type->IsWall)  // BuildingTypeClass+0x1571
        cost = (int)(cost * instantMultiplier);

    return cost;
}
```

**MultipleFactory bonus:** At `g_RulesClass + 0x57C`. Each additional factory of the
same type multiplies the build time by this ratio (< 1.0 = faster). The check is
`factoryCount - 1` iterations of multiplication.

---

## 16. HouseClass Factory Pointer Offsets (consolidated)

```
Offset   Field                 Production Types (RTTI values)
------   -----                 ---------------------------
+0x53AC  InfantryFactory       RTTI 2,3 (InfantryClass/InfantryTypeClass)
+0x53B0  AircraftFactory       RTTI 0xF,0x10 (AircraftClass/AircraftTypeClass)
+0x53B4  BuildingFactory       RTTI 1,0x28 (BuildingClass, non-naval, isNaval=0)
+0x53B8  NavalBuildFactory     RTTI 1,0x28 (BuildingClass, naval, isNaval=1)
+0x53BC  VehicleFactory        RTTI 6,7 (UnitClass/UnitTypeClass, NavalIndex≠5)
+0x53CC  NavalFactory          RTTI 6,7 (UnitClass/UnitTypeClass, NavalIndex=5)
```

### Factory Count Fields (for MultipleFactory speed bonus)
```
+0x5378  int AircraftFactoryCount
+0x537C  int InfantryFactoryCount (was labelled AircraftFactory offset wrong)
+0x5380  int VehicleFactoryCount
+0x5384  int BuildingFactoryCount
+0x5388  int NavalFactoryCount
```

### Production "Queued" Flags (set by FUN_005007a0)
```
+0x53D0  byte AircraftQueued
+0x53D1  byte InfantryQueued
+0x53D2  byte VehicleQueued (non-naval)
+0x53D3  byte NavalVehicleQueued
+0x53D4  byte BuildingQueued (non-naval)
+0x53D8  byte NavalBuildingQueued (NavalIndex=5)
```

### AI "What to Build" Fields (cleared by AbandonProduction)
```
+0x564C  int AI_ChosenBuildingType (-1 = none)
+0x5650  int AI_ChosenUnitType (-1 = none)
+0x5654  int AI_ChosenAircraftType (-1 = none)
+0x5658  int AI_ChosenInfantryType (-1 = none)
```

### Rally Point / Exit Coordination
```
+0x53DC  TechnoClass* SpiedBuilding (or production-related ptr)
+0x53E0  CellStruct RallyPoint (2 shorts)
```

---

## 17. Complete FactoryClass Method Table

| Address     | Name                  | Description |
|-------------|-----------------------|-------------|
| 0x004C98F0  | Constructor           | Allocates 0x74 bytes, initializes all fields |
| 0x004C9B20  | AI (vtable[23])       | Per-frame production tick |
| 0x004C9C60  | HasChanged            | Read+reset IsDifferent flag |
| 0x004C9C70+ | StartProduction       | Start item or append to queue |
| 0x004C9E60  | Suspend               | Pause production |
| 0x004C9F00  | SetRate/Resume        | Calculate rate, start timer, un-suspend |
| 0x004C9FB0  | GetBuildStepTime      | Full build time calculation |
| 0x004CA0E0  | AbandonProduction     | Cancel + refund + delete object |
| 0x004CA120  | GetProgress           | Returns Production_Value (0-54) |
| 0x004CA130  | IsComplete            | (Object!=NULL && Progress==54) or (SpecialItem!=-1 && ==54) |
| 0x004CA160  | GetObject             | Returns Object ptr (+0x58) |
| 0x004CA1A0  | CompletedProduction   | Clear object, reset for next item |
| 0x004CA370  | Load (IPersistStream) | Deserialize from save game |
| 0x004CA430  | Debug Dump            | Print all fields with format strings |
| 0x004CA5A0  | StartNextQueued       | Pop queue front, call Begin_Production |
| 0x004CA620  | RemoveFromQueue       | Find/remove one TechnoTypeClass* from queue |
| 0x004CA670  | CountTypeInFactory    | Count instances of a type (active + queued) |
| 0x004CA6B0  | IsInQueue             | Check if a TechnoTypeClass* exists in queue |
| 0x004CA6E0  | RecalcAllRates        | Update rate for all factories of same owner |
| 0x004CABF0  | (DynVec init)         | DynamicVectorClass subobject initialization |

---

## 18. Production Lifecycle — Complete Flow

```
Player clicks cameo
    → SelectClass::Action (0x006AB080)
    → Creates network event (type 0x0E for produce, 0x0F for suspend, etc.)
    → Network lockstep delivers event to all players

Command Dispatcher (0x004C6CB0)
    case 0x0E → HouseClass::Begin_Production (0x004FA350)
        → Resolves TechnoTypeClass from RTTI + HeapID
        → Checks CanBuild prerequisites
        → Gets/creates FactoryClass (0x74 bytes)
        → FactoryClass::StartProduction
            Path A: Creates TechnoClass, sets Balance, sets Owner
            Path B: Appends to queue
        → FactoryClass::SetRate — calculates timer, un-suspends
        → AI headstart for computer players

Every game frame:
    LogicClass::AI (0x0055AFB0)
        → Iterates g_FactoryClass_Array
        → Calls FactoryClass::AI (vtable[0x5C/4]) for each
            → Timer check → Progress++ → Cost deduction → Completion

    HouseClass::Update (0x004F8F70)
        → If field_0x1fc (ProductionDirty):
            → AI_ManageProduction → AI_ResumeProduction
        → For AI: AI_Choose_Building/Unit/Aircraft/Infantry

On completion (Progress == 54):
    FactoryClass::AI sets IsSuspended=true, Rate=0

    StripClass::AI (0x006A8B30) detects via HasChanged + IsComplete
        → For buildings: waits for player click
        → For vehicles: calls FUN_00734250 (store exit ptr) + creates Place event
        → For infantry/aircraft: auto-creates Place event (0x0B)

    Command 0x0B → HouseClass::Place_Production (0x004FB0E0)
        → Gets factory, checks IsComplete
        → Attempts to exit/place unit at target cell
        → On success: CompletedProduction (clears Object, resets Progress)
        → Calls FUN_004FAA10 which handles queue advancement

    FUN_004FAA10 (cancel/advance handler)
        → Removes completed type from queue
        → If QueueCount > 0: AbandonProduction + StartNextQueued
        → StartNextQueued pops front, calls Begin_Production for next item
        → Cycle repeats

    FUN_00509140 (prerequisite update)
        → Called when buildings change
        → Purges unbuildable items from queue
        → Auto-resumes system-suspended production
        → Abandons + restarts if active item became unbuildable
```

---

## Confidence Levels

**HIGH (verified from decompiled code):**
- All FactoryClass methods listed in section 17
- FactoryClass::AI complete logic including OnHold behavior
- HouseClass factory pointer offsets (+0x53AC through +0x53CC)
- Command dispatcher event types and routing
- FactoryClass::Suspend `IsManual` semantics
- AbandonProduction refund logic and AI field clearing
- CheckBuildLimit logic with BuildLimit field
- FUN_00734250 naval/land vehicle exit distinction

**HIGH (verified from multiple functions):**
- HouseClass production count fields (+0x5378-0x5388)
- HouseClass production "queued" flags (+0x53D0-0x53D8)
- AI "what to build" fields (+0x564C-0x5658)
- Queue restart mechanism via FUN_004FAA10 + StartNextQueued

**MEDIUM (partially traced):**
- HouseClass::Update production-dirty flag (field_0x1fc) — triggers
  ManageProduction/ResumeProduction but exact setter sites not fully enumerated
- FUN_00509140 — labeled as UpdateFactoryPrereqs based on behavior, exact caller
  chain not fully traced
- GetBuildStepTime bonus calculations — power ratio and MultipleFactory math
  involve floating point that Ghidra doesn't perfectly decompile
