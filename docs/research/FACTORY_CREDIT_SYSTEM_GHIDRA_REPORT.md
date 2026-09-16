# FactoryClass Credit System — Ghidra Report

**Source**: Ghidra decompilation of `gamemd.exe`
**Confidence**: High — all functions directly decompiled
**Date**: 2026-03-26
**Supplements**: [BUILD_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILD_QUEUE_GHIDRA_REPORT.md), FACTORYCLASS_PRODUCTION_DEEP_DIVE.md

## Overview

The RA2/YR credit system has two pools: **cash** (Credits at HouseClass+0x30C) and
**ore storage** (StorageClass at HouseClass+0x2FC — `Spend_Money` `LEA ESI,[EBX+0x2FC]` at `0x004F9798`; corrected 2026-09-06, `+0x314` is the separate weed StorageClass used by `Add_Tiberium_To_Storage`). Cash is spent first; ore is
drained from silos only when cash runs out.

## HouseClass Credit Fields

| Byte Offset | Type | Field | Description |
|-------------|------|-------|-------------|
| +0x24 | ptr | Interface vtable | COM interface with GetAvailableCredits at slot 6 |
| +0x1DC | int | InitialCredits | Starting credits (set once, never modified) |
| +0x2DC | int | TotalSpent | Cumulative spending (for score screen) |
| +0x30C | int | Credits | Current cash balance (directly spendable) |
| +0x310 | int | OreCapacity | Whether storage capacity is nonzero |
| +0x2FC | 16B | StorageClass | 4 floats: ore bails per tiberium type (corrected 2026-09-06 from +0x314) |
| +0x54E8 | int | HarvestedCredits | Cumulative ore value harvested (statistics) |

## Add_Credits (0x004F9950)

Trivial — just adds to the cash pool:

```c
void HouseClass::Add_Credits(int amount) {
    this->Credits += amount;   // +0x30C
}
```

Called by:
- `AbandonProduction` — refunds `TypeCost - Balance` (unspent portion)
- `DepositOreCredits` (0x004F9610) — harvester unload: ore value * income multiplier
- Sell building — refund based on health ratio

## GetAvailableCredits (virtual, via HouseClass+0x24)

Called indirectly through a COM-style interface subobject at HouseClass+0x24.
Vtable slot 6 (offset +0x18).

```c
int GetAvailableCredits() {
    return this->Credits + (int)StorageClass::GetTotal(&this->Storage);
}
```

Returns the **sum of both pools**. All known callers:

| Caller | Purpose |
|--------|---------|
| FactoryClass::AI | Check if can afford production step |
| FactoryClass::SetRate | Check if can afford before resuming |
| CreditsClass::AI (0x004A2600) | Sidebar credit display target |
| TriggerCondition 0x0C | "Credits >= N" map trigger |
| TriggerCondition 0x34 | "Credits <= N" map trigger |

## Spend_Money (0x004F9790)

The main deduction function. Two-pool logic:

```c
void HouseClass::Spend_Money(int amount) {
    float oldStorageTotal = StorageClass::GetTotal(&this->Storage);

    int cash = this->Credits;  // +0x30C

    if (cash >= amount) {
        // Simple path: enough cash
        this->Credits = cash - amount;
    } else {
        // Drain all cash, take remainder from ore storage
        int deficit = amount - cash;
        this->Credits = 0;
        int actualSpent = cash;

        if (deficit > 0 && StorageClass::GetTotal() > 0.0) {
            // Iterate refineries, drain ore bails one at a time
            for (int i = 0; i < this->OwnedBuildingCount; i++) {
                if (building[i] == NULL) continue;
                if (StorageClass::GetTotal() <= 0.0) break;

                while (deficit > 0) {
                    int slot = StorageClass::FindFirstNonEmptySlot();
                    while (StorageClass::GetAmount(slot) > 0.0 && deficit > 0) {
                        float removed = StorageClass::Remove(1.0, slot);
                        int removedInt = ftol(removed);
                        deficit -= removedInt;
                        actualSpent += removedInt;

                        if (deficit < 0) {
                            // Overpaid — refund fractional excess to Credits
                            actualSpent += deficit;
                            this->Credits -= deficit;
                            deficit = 0;
                            break;
                        }
                    }
                    if (StorageClass::GetTotal() <= 0.0) break;
                }
            }
        }
        amount = actualSpent;
    }

    Notify_Credit_State_Change(oldStorageTotal);   // 0x004F9970 — name is a VERA label, unverified
    this->TotalSpent += amount;  // +0x2DC
}
```

**In practice for YR**: Most buildings have `Storage=0` (no silo capacity), so the
ore storage pool is rarely used. Credits from harvester deposits go directly to
+0x30C via `Add_Credits`. The two-pool logic is inherited from Tiberian Sun's
silo system but is mostly vestigial in standard YR gameplay.

## StorageClass Layout (HouseClass+0x2FC, 16 bytes)

```
+0x00  float  Amount[0]  (Riparius / Ore)
+0x04  float  Amount[1]  (Cruentus / Gems)
+0x08  float  Amount[2]  (Vinifera)
+0x0C  float  Amount[3]  (Aboreus)
```

| Method | Address | Description |
|--------|---------|-------------|
| GetTotal | 0x006C9650 | Sum all 4 floats, return ftol as int |
| GetAmount | 0x006C9680 | Return Amount[slot] |
| AddAmount | 0x006C9690 | Amount[slot] += amt |
| Remove | 0x006C96B0 | Subtract from slot, clamp to 0, return remaining |
| FindFirstNonEmpty | 0x006C9820 | First slot index where Amount > 0.0, or -1 |

## Credit Counter Display (CreditsClass)

`CreditsClass::AI` at 0x004A2600, called every frame from sidebar draw:

```c
void CreditsClass::AI(bool forceUpdate) {
    int target = GetAvailableCredits(g_PlayerPtr);
    if (target < 0) target = 0;

    // Geometric decay: step = |diff| / 8, clamped [1, 143]
    int diff = target - this->displayed;
    int step = abs(diff) >> 3;
    step = clamp(step, 1, 143);
    if (target < this->displayed) step = -step;

    this->displayed += step;

    if (displayed changed) {
        this->animating = true;
        this->counting_up = (step > 0);
    }
    this->dirty = true;
}
```

The `/8` geometric decay creates the classic RA2 "counting up/down" animation.
Step is clamped to max 143, so very large credit changes still animate smoothly.

### CreditsClass layout (static at 0x00A83E18)

| Offset | Type | Field |
|--------|------|-------|
| +0x00 | i32 | target |
| +0x04 | i32 | displayed |
| +0x08 | u8 | dirty |
| +0x09 | u8 | counting_up |
| +0x0A | u8 | animating |
| +0x0C | i32 | direction (1=up, 3=down) |

## Notify_Credit_State_Change (0x004F9970)

> The name `Notify_Credit_State_Change` is a VERA-assigned label for `0x004F9970`; it is not verified against any native symbol (marked 2026-09-06).

Triggers building animation updates when ore storage changes:

```c
void HouseClass::Notify_Credit_State_Change(float oldStorageTotal) {
    int oldTotal = (oldStorageTotal != 0) ? (int)oldStorageTotal : 0;
    int newTotal = (this->OreCapacity != 0)
        ? (int)StorageClass::GetTotal(&this->Storage) : 0;

    if (oldTotal != newTotal) {
        for (int i = 0; i < this->OwnedBuildingCount; i++) {
            Building* bld = this->OwnedBuildings[i];
            if (bld != NULL && !bld->IsDestroyed && bld->Type->HasCreditAnim) {
                bld->PlayAnim(2);  // trigger visual state update
            }
        }
    }
}
```

## Record_Last_Built (0x004FB6B0)

Called after Place_Production succeeds:

```c
void HouseClass::Record_Last_Built(TechnoClass* object) {
    this->ProductionCompletedFlag = 1;  // +0x246

    TechnoTypeClass* type = object->GetType();
    int rtti = object->WhatAmI();

    switch (rtti) {
    case 1:   // UnitType
        this->LastBuiltUnit = type->GetHeapID();        // +0x274
        break;
    case 2:   // AircraftType
        this->LastBuiltAircraft = type->GetHeapID();    // +0x278
        break;
    case 6:   // BuildingType
        this->LastBuiltBuilding = type->GetHeapID();    // +0x26C
        break;
    case 0xF: // InfantryType
        this->LastBuiltInfantry = type->GetHeapID();    // +0x270
        break;
    }

    // Increment "times built" counter (unless IsBuildableOnce)
    if (!type->IsBuildableOnce)
        IndexClass::Increment(type->GetHeapID());

    // Play announce sound with per-type delay from rules
    // Buildings have no announce sound

    this->ShouldRecalcSidebar = 1;  // +0x1FC (the ProductionDirty flag!)
}
```

**Key finding**: This function sets `+0x1FC` (the ProductionDirty flag), which
triggers `AI_ManageProduction` + `AI_ResumeProduction` on the next HouseClass::Update
tick. This is part of the queue restart chain.

### Last-built tracking fields

| Offset | Field | RTTI |
|--------|-------|------|
| +0x26C | LastBuiltBuilding | 6 |
| +0x270 | LastBuiltInfantry | 0xF |
| +0x274 | LastBuiltUnit | 1 |
| +0x278 | LastBuiltAircraft | 2 |

## Starting Credits Initialization

`HouseClass::Set_Credits_And_Color` (0x004FCE00):

```c
void Set_Credits_And_Color(int colorScheme, int unused, int startingCredits) {
    this->InitialCredits = startingCredits;  // +0x1DC
    this->Credits = startingCredits;         // +0x30C
    this->Type->ColorScheme = colorScheme;   // Type+0xC0
    this->DisplayColorScheme = colorScheme;  // +0x16054
}
```

Called during `ScenarioClass::Create_Houses` for each player slot. Both
+0x1DC (preserved reference) and +0x30C (live balance) start at the same value.
StorageClass at +0x2FC starts zeroed.

## Complete Money Flow

```
STARTING CREDITS (lobby)
    │
    ▼
Set_Credits_And_Color → +0x1DC (InitialCredits) + +0x30C (Credits)
    │
    ├─────────────────────────────────────────┐
    │                                         │
    ▼                                         ▼
 +0x30C Credits (Cash)                 +0x2FC StorageClass (Ore in Silos)
    │                                         │
    │ ◄── Add_Credits (refund, sell)          │ ◄── Add_Tiberium_To_Storage
    │ ◄── DepositOreCredits (harvest)         │     (weed harvester: raw bails)
    │                                         │
    └────────────┬────────────────────────────┘
                 │
                 ▼
    GetAvailableCredits = Credits + StorageClass::GetTotal()
                 │
                 ▼
    FactoryClass::AI: available >= costPerStep?
        YES → Spend_Money(costPerStep)
              ├─ Deducts from Credits first
              └─ If insufficient, drains StorageClass
        NO  → OnHold = true, rollback 1 production step
                 │
                 ▼
    Cancel → AbandonProduction
        → Add_Credits(TypeCost - Balance)  // full refund of spent amount
        → Balance = 0
                 │
                 ▼
    Complete → CompletedProduction
        → Spend_Money(remaining Balance)  // pay rounding remainder
        → Balance = 0
        → Record_Last_Built → sets ProductionDirty (+0x1FC)
```
