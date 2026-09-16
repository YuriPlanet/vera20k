# BuildingClass::ChangeOwner — Ghidra Report

Reverse-engineered from `gamemd.exe`. Covers how building ownership transfers work:
the core `ChangeOwner` function, who calls it, and the garrison-specific ownership
reconciliation that runs every tick.

## Overview

Building ownership transfer in the original engine is handled by a single large virtual
method `BuildingClass::ChangeOwner` (vtable+0x3D4). It is used for engineer capture,
garrison ownership, mind control, and house transfer. For garrison specifically, ownership
is **not** transferred at entry time — instead a per-tick reconciliation function
(`CheckAutoSellOrCivilian`) detects the mismatch and calls `ChangeOwner`.

---

## 1. Core Function: `BuildingClass::ChangeOwner` (0x00448260, vtable+0x3D4)

**Size:** ~1,400 bytes
**Signature:** `int __thiscall BuildingClass::ChangeOwner(HouseClass* newOwner, bool announce)`
**Returns:** 1 on success, 0 if same owner (no-op)

### What it does (in order):

| Step | Description | Relevant to garrison? |
|------|-------------|----------------------|
| 1 | Early exit if `newOwner == this->Owner` | Yes |
| 2 | If the old owner's side has the capture-grant flag (`Side+0x1A6`) and `Type+0x1558` (`ProduceCashStartup`) is non-zero, grant that amount via `HouseClass::Add_Credits` and stamp `+0x6D0/+0x6D4/+0x6D8` (the oil-derrick capture grant; corrected 2026-09-06 — this is not a power-credit refund) | No (civilian buildings have `ProduceCashStartup=0`; oil derricks do not) |
| 3 | If `IsRadarJammer` (`Type+0x16A4`), recalculate old owner's jammer mask | No |
| 4 | Disconnect walls if `IsWall` (`Type+0x16BE`) | No |
| 5 | Play EVA notifications if human player involved (`EVA_StructureCaptured` or `EVA_StructureSold`) | Yes — announces capture |
| 6 | Mark building for redraw, set `HasEngineer=true` | Yes |
| 7 | If `Type+0x1552` (IsCapturable): enable production, attach weapon anims | No |
| 8 | Abandon production if building has an active factory | No (civilian buildings don't produce) |
| 9 | `HouseClass::Recount(this)`, followed inside `TechnoClass::ChangeOwner` by `HouseClass::Removed_From_Game` before the owner swap and `HouseClass::Added_To_Game` afterward | Yes |
| 10 | Remove building from old owner's **10 typed tracking lists**: buildings, radar, factories, walls, garrisonable, sensor, gap gen, laser fence, drone source, spysat | Yes (partially — garrisonable list) |
| 11 | If building provides power/drain (`Type+0x1564/0x1568`), subtract from old owner | No |
| 12 | Add building to new owner's **same 10 typed tracking lists** | Yes |
| 13 | If building provides power/drain, add to new owner | No |
| 14 | Update radar overlay if building has radar range (`Type+0xEB8`) | Possibly |
| 15 | Handle docked units: change their owner too via vtable+0x3D4 | No |
| 16 | Recalculate base center for both old and new owner | Minor |
| 17 | Update sidebar if human player involved | Yes |
| 18 | Reconnect walls in all 4 directions if IsWall | No |
| 19 | Update anim facing, direction, and remap | Yes (visual update) |

### Key observation for our engine

Most of ChangeOwner's complexity (steps 2-4, 7-8, 10-12, 15, 18) handles systems we don't
have yet (power grids, production factories, wall connections, typed house lists). For
the currently implemented transfer spine, the required steps are:

1. Reject a same-owner transfer as a no-op.
2. Run spawn cleanup, then remove the entity's live category count from the old house.
3. Detach targeting, change `entity.owner` and the owner index together, then add the
   live category count to the new house.
4. Refresh owner-dependent derived state. Garrison reconciliation passes
   `announce=false`, so its path does not require an EVA capture announcement.

---

## 2. Garrison Entry: `InfantryClass::EnterGarrison` (0x00522910)

**Size:** 318 bytes
**Called from:** `InfantryClass::PerCellProcess` (0x00519630) when infantry arrives at garrison building cell — via a small trampoline `FUN_00519710` (0x00519710) called at 0x005196DB, or directly within the same PerCellProcess branch.
**Note (corrected 2026-05-28: was "Called from InfantryClass::Mission_Enter (0x005196A0)"; binary shows 0x005196A0 = InfantryClass__PerCellProcess, not Mission_Enter; ROOT_CAUSE: RTTI_LABEL_DRIFT — verified via `get_function_by_address 0x005196A0` and `get_xrefs_to 0x00519710`)**
**Important:** This function does **NOT** call ChangeOwner.

### Flow:

```
1. Check infantry type flags:
   IF Occupier (InfantryTypeClass+0xEB4):
     → Garrison entry path (below)
   IF Assaulter (InfantryTypeClass+0xEB5):
     → Assault path: SpawnUnitsWithParachute, stop infantry, navigate to building
   ELSE:
     → Return (neither flag set)

2. Limbo infantry (vtable+0xD4) — remove from map

3. Append infantry pointer to building's occupant DynamicVectorClass:
   - Items array at Building+0x688
   - Count at Building+0x694
   - Capacity at Building+0x68C

4. Recalculate building power via FUN_0070f6e0

5. If this is the FIRST occupant (count == 1):
   - Set building mission to 2 (Defend?) via vtable+0x124
   - If local human player: play EVA_StructureGarrisoned, fire radar event

6. If infantry's house has flag at HouseClass+0x1EC:
   - Clear byte at Infantry+0x691 (can-resize garrison flag)
   - Clear byte at Infantry+0x690

NOTE: Ownership transfer happens LATER via the per-tick reconciliation.
```

---

## 3. Garrison Ownership Reconciliation: `BuildingClass::CheckAutoSellOrCivilian` (0x00458200)

**Size:** ~200 bytes
**Called from:** `BuildingClass::Update` (0x004401AF) — runs every game tick
**This is where garrison ChangeOwner actually happens.**

### Flow:

```
1. Only applies to buildings with Type+0x634 == -1
   (civilian/garrisonable buildings with no weapon turret slot)

2. If building is at red HP → auto-sell (SellBuilding)

3. Find the "Civilian" house:
   - Iterate g_HouseClass_Array
   - Match house where CountryClass+0xBC == FUN_006a46d0()
     (looks up the Civilian side index)
   - Result = puVar8 (the civilian HouseClass*)

4. Get occupant count via vtable+0x408 (GetOccupantCount)

5. IF occupant count == 0 AND building owner != civilian house:
   → Building was garrisoned but all occupants left
   → Play EVA_StructureAbandoned (if human player)
   → Call FUN_00458330 (cleanup/recalc)
   → ChangeOwner(civilian_house, 0)  ← REVERT to neutral

6. IF occupant count > 0 AND building owner == civilian house:
   → Building has occupants but still belongs to civilian
   → Call FUN_00458330 (cleanup/recalc)
   → ChangeOwner(first_occupant->House, 0)  ← TRANSFER to garrisoner
     (first_occupant->House = *(Items[0] + 0x21C))
```

### Key insight

The original engine uses a **lazy reconciliation** pattern: the entry function
(`EnterGarrison`) just adds the infantry to the vector. The ownership transfer
happens on the next tick via `CheckAutoSellOrCivilian`, which detects the
"building is neutral but has occupants" mismatch and calls `ChangeOwner`.

This means there is a **1-tick delay** between infantry entering and ownership
transferring. The same function handles the reverse: when all occupants leave
(are killed/ejected), it detects "building is player-owned but has no occupants"
and reverts to the civilian house.

---

## 4. Occupant Ejection: `BuildingClass::EjectOccupants` (0x004575B0)

**Called on:** building destruction, sell, or explicit eject command

**CORRECTION (2026-05-28): The flow description below from "report 017" is WRONG — the binary at 0x004575B0 is NOT a garrison-ejection function. Decompilation via `decompile_function 0x004575B0` shows this function iterates `UpgradeLevel`, removes upgrades with `BuildingClass__RemoveLastUpgrade`, refunds credits to the owner via `HouseClass__Add_Credits`, and marks sidebar dirty. It is a building upgrade removal function, not an occupant ejector. ROOT_CAUSE: STRUCT_FAMILY_CASCADE — the address was sourced from an external report, not verified directly against the binary.**

**The actual garrison occupant ejection logic in gamemd.exe has not been verified to a specific address in this session. The 1-tick ownership revert via CheckAutoSellOrCivilian (§3) remains correct when occupants are removed by any means.**

### Flow (UNVERIFIED — sourced from report 017, address 0x004575B0 is WRONG):

```
1. Loop while occupant count at Building+0x702 > 0:
   a. Get infantry pointer from occupant array
   b. Unlimbo infantry near building (vtable+0xB8)
   c. If infantry house has HouseClass+0x1EC flag:
      - Clear Infantry+0x691 and Infantry+0x690
   d. Set infantry mission (guard/scatter)
   e. Move infantry to building's position

2. Clear the occupant DynamicVectorClass

3. Ownership revert happens on next tick via CheckAutoSellOrCivilian
   (detects 0 occupants, reverts to civilian house)
```

---

## 5. Engineer Capture (for comparison): `FUN_005202f0`

Unlike garrison, engineer capture calls `ChangeOwner` **directly and immediately**:

```
1. Infantry approaches building, distance check < 0x80 leptons
2. Set building mission to 3 (Guard)
3. Limbo building temporarily (vtable+0xDC)
4. ChangeOwner(infantry->House, 1)  ← immediate, with EVA announce
5. Set building field from infantry type data
6. Destroy infantry (vtable+0xF8) — engineer is consumed
```

---

## 6. Function Reference

| Address | Name | Role |
|---------|------|------|
| `0x00448260` | `BuildingClass::ChangeOwner` | Core ownership transfer (vtable+0x3D4) |
| `0x00522910` | `InfantryClass::EnterGarrison` | Adds infantry to garrison vector, does NOT change owner |
| `0x00458200` | `BuildingClass::CheckAutoSellOrCivilian` | Per-tick reconciliation: transfers owner based on occupant state |
| `0x00458330` | `BuildingClass::RecalcGarrisonState` | Cleanup helper called before ChangeOwner |
| `0x004575B0` | `BuildingClass::EjectOccupants` | **WRONG ADDRESS** — binary is a building upgrade-removal function, not garrison ejector. Real ejection address unverified. (corrected 2026-05-28 via `decompile_function 0x004575B0`) |
| `0x00519630` | `InfantryClass::PerCellProcess` | Per-cell handler that calls AddGarrisonOccupant (corrected 2026-05-28: was `0x005196A0 / Mission_Enter`; binary shows PerCellProcess at 0x00519630; ROOT_CAUSE: RTTI_LABEL_DRIFT) |
| `0x005202f0` | `InfantryClass::Mission_Capture` | Engineer capture — calls ChangeOwner directly |
| `0x0070f6e0` | `TechnoClass::RecalcPower` | Power recalculation after garrison state change |

---

## 7. HouseClass+0x1EC Flag

This flag is checked in both `EnterGarrison` and `EjectOccupants` to conditionally
clear Infantry+0x690/0x691 bytes. From context in `TechnoClass::Unlimbo` report:
"If owner house has radar (+0x1EC)..." — it appears to be a **has-radar** or
**has-active-detection** flag on HouseClass, not `MultiplayPassive`. Its exact
purpose in the garrison flow is unclear but likely relates to updating the house's
detection/radar state when occupants enter/leave. Not critical for our implementation.

---

## 8. Implications for Our Engine

### Current garrison ownership-transfer contract:

1. **After garrison entry**, the building's live update/reconciliation pass detects a
   `CanBeOccupied` civilian building with occupants and transfers it to the first
   occupant's house. It must use the common ownership chokepoint so old/new
   `OwnedBuildings` totals move exactly once; boarding itself does not transfer it.

2. **After the last occupant leaves or dies**, that same reconciliation pass returns
   the empty building to the resolved civilian house through the common chokepoint,
   again moving the old/new building totals once.

3. **What we can skip** (handled by ChangeOwner but irrelevant for neutral buildings):
   - Power grid recalculation (civilian buildings have no power)
   - Production factory management (no production)
   - Wall reconnection (not walls)
   - Typed house list tracking (we don't have these)
   - Docked unit ownership cascade (no docking)
   - Auto-sell at red HP (separate feature)

### MultiplayPassive vs hardcoded names

The original engine finds the "Civilian" house by matching the house's CountryClass
side index, not by hardcoding the name. Current Rust parses and stamps
`MultiplayPassive` for stock civilian/passive houses and uses that role in outcome and
garrison authority paths; resolving the exact civilian owner still follows the
CountryClass side relationship described in §9.5.

---

*Confidence: HIGH for the reconciliation pattern (CheckAutoSellOrCivilian is clearly
the mechanism). MEDIUM for HouseClass+0x1EC identity. All function addresses verified
via live Ghidra decompilation.*

---

## 9. Verification Pass (2026-04-24)

Triggered because [GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) §4 Step 5 incorrectly
claimed ownership transfer happens inside `AddGarrisonOccupant` (0x00522910).
Re-verified this doc's core claims against the live binary to confirm the
actual mechanism.

### 9.1 CheckAutoSellOrCivilian — fully re-verified

`BuildingClass__CheckAutoSellOrCivilian @ 0x00458200` decompilation matches
this doc's §3 exactly. Confirmed structure:

```c
if (*(int *)(param_1->Type + 0x634) == -1) {
    if (IsRedHP(param_1)) SellBuilding(param_1);

    // Find civilian house by side-index match
    civilian_side = FUN_006a46d0();
    civilian_house = NULL;
    for (i = 0; i < g_HouseClass_Array_Count; i++) {
        if (*(int*)(g_HouseClass_Array[i]->CountryType + 0xBC) == civilian_side) {
            civilian_house = g_HouseClass_Array[i];
            break;
        }
    }

    // Revert transition: empty & not civilian-owned → revert to civilian
    if (GetOccupantCount() == 0 && Owner != civilian_house) {
        // EVA_StructureAbandoned + radar event if human
        RecalcGarrisonState(this);  // FUN_00458330
        ChangeOwner(civilian_house, 0);  // vtable+0x3D4
    }

    // Transfer transition: occupied & still civilian-owned → give to first occupant
    if (GetOccupantCount() > 0 && Owner == civilian_house) {
        RecalcGarrisonState(this);
        ChangeOwner(*(Items[0] + 0x21C), 0);  // first_occupant->Owner
    }
}
```

**Confirmed:**
- Called UNCONDITIONALLY from `BuildingClass::Update` at `0x004401AF` (entry 0x0043FB20). Single caller, every tick.
- Gate condition: `BuildingTypeClass+0x634 == -1` (meaning: no primary weapon slot — i.e., civilian/garrisonable buildings).
- First-occupant pointer is `*(*(building+0x688))` = Items[0]; its `+0x21C` is Owner.
- The 1-tick-delay ownership transfer pattern is **correct**: `AddGarrisonOccupant` only appends to the vector, Update's next tick runs CheckAutoSellOrCivilian which does the actual ChangeOwner.

**Confidence: HIGH** — full decompilation matches this doc's claims.

### 9.2 FUN_00458330 — CORRECTION: not a cleanup helper

This doc's §6 table calls `0x00458330` "BuildingClass::RecalcGarrisonState"
with role "Cleanup helper called before ChangeOwner". **This is misleading.**

Full decompilation shows FUN_00458330 is the **anim-variant selector**: it
iterates 5 anim-slot fields on the building (`+0x5A4`, `+0x568`, `+0x56C`,
`+0x570`, `+0x574`) and for each occupied slot picks one of 3 anim names
from `BuildingTypeClass` based on health and occupancy:

| Condition | Anim name offset (slot 0) |
|-----------|---------------------------|
| Health > `RulesClass+0x1700` (red HP threshold) AND occupants < 1 | `Type + 0x1414` (healthy-empty) |
| Health > red HP threshold AND occupants ≥ 1 | `Type + 0x1434` (healthy-garrisoned) |
| Health ≤ red HP threshold | `Type + 0x1424` (damaged) |

Slot offsets: slot 0 = 0x1414/0x1424/0x1434; slot 1 = 0x1018/0x1028/0x1038;
slot 2 = 0x105C/0x106C/0x107C; slot 3 = 0x10A0/0x10B0/0x10C0;
slot 4 = 0x10E4/0x10F4/0x1104.

Each anim name (if non-empty) is passed to `BuildingClass__CreateAnimForSlot`
which spawns/swaps the corresponding animation.

**This is where `ActiveAnim` / `ActiveAnimDamaged` / `ActiveAnimGarrisoned`
variants get selected.** It is called from CheckAutoSellOrCivilian on both
transitions (revert at 0x004582DF, transfer at 0x00458309) specifically to
refresh the garrisoned/empty anim before ChangeOwner triggers the visual
update — hence it *runs* before ChangeOwner, but its purpose is visual
state refresh, not "cleanup".

**Rename suggestion:** `BuildingClass::RefreshAnimVariants` (or
`::UpdateAnimSet`) — it's the occupancy/damage anim-variant dispatcher.

**Confidence: HIGH** — decompilation clearly shows anim-slot iteration
with 3 × 5 name offsets and health/occupancy branching.

### 9.3 ChangeOwner size — stale

This doc's §1 states "Size: ~1,400 bytes". Current Ghidra shows body
`0x00448260 – 0x00449405` = **0x11A5 = 4,517 bytes**. The step-by-step
behavior table in §1 is still accurate; just the size estimate is stale.

### 9.4 Cross-doc impact

- [GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) §4 Step 5 item 6 ("Transfer building
  ownership to infantry's owner") is **WRONG in location** — not in
  AddGarrisonOccupant. The actual transfer is here, one tick later.
- [GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) §16 row for `0x00458200`
  (`CheckAutoSellOrCivilian | Per-tick ownership reconciliation`) is
  accurate and matches this doc.

### 9.5 Open questions — resolved and still-open

**RESOLVED — FUN_006a46d0 is `SideClass::FindIndexByName("Civilian")`:**
- The "Civilian" string literal sits at `0x00818164`.
- `CheckAutoSellOrCivilian` has a DATA xref from `0x00458236` to
  `0x00818164` (the string's address) immediately before the
  `CALL FUN_006a46d0` at `0x0045823D`. In fastcall the string is loaded
  into ECX as `param_1`.
- FUN_006a46d0 iterates `DAT_008b4124` (sides array), calling
  `FUN_007c8d20` (strcmp) on `sides[i]->Name` (field `+0x24`) vs the
  passed string. Returns the first matching index or -1.
- Net effect: "find the side index whose Name == 'Civilian'". The
  returned index is then used to locate the civilian HouseClass by
  matching `HouseClass->CountryType->+0xBC` against it.
- **Confidence: HIGH.**

**Other callers of FUN_006a46d0** (all pass a side name via register):
`HouseClass__Is_Enemy` (0x00501581), `CaptureManagerClass__SetOriginalOwner`
(0x00472341), `AnimClass__AI` (0x004249A7), `InfantryTypeClass__ReadINI`
(via 0x00524553 data ref), plus 5 others. Generic
"get-side-index-by-name" utility — not garrison-specific.

**STILL OPEN — HouseClass+0x1EC:**
- Not in the ownership transfer path. Appears only in `AddGarrisonOccupant`
  (0x00522910) and `SellBuilding` (0x00457DE0) where, if non-zero on the
  infantry's owner, it triggers clearing of infantry `+0x690`/`+0x691`
  (two bytes).
- Byte-pattern searches for common write forms
  (`MOV BYTE PTR [reg+0x1EC], 1`) returned no matches — the flag is
  probably written by computed-offset code (e.g., part of a per-frame
  radar state update) or zero-init only.
- Constructor at `0x004F5190` does not explicitly initialize it, so it
  defaults to 0 (rest of struct zeroed by `new`).
- Treat as informational. Leaving identification for a future pass if
  it starts mattering for sim behavior.
