# UnitClass::Mission_Deploy_Building — Refinery Unload State Machine

> **Correction 2026-05-21 - resolved open questions**
>
> Later reports resolve this document's stale open questions around
> `DAT_0089F6A0` and radio 0x07. `DAT_0089F6A0` is the hardcoded west-neighbor
> direction-table entry `(-1,0)` initialized by
> `Foundation_direction_table_init @ 0x0049F2F0`, not a `DockingOffset%d`
> value. Radio 0x07 is not sent by stock DockUnload and is also not sent by
> `BuildingClass::MissionRepairAndProduce`; the verified direct sender is the
> carryall pickup path.
>
> **Correction 2026-05-22 - stock reachability re-audit**
>
> [STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md)
> supersedes the remaining reachability mistakes in this report. Stock
> `HARV/CMIN -> GAREFN/NAREFN` unload uses the zero `UnitClass+0x2E4` path.
> The nonzero `+0x2E4` branch is the conditional `ReleaseDockedHarvester` /
> `Force_Track(0x47)` path, not normal stock dump completion.
> The `0x0065AE30` radio contact-vector scan proceeds to the RateTimer/state
> dispatch when any contact exists; no contacts performs cleanup, clears
> `+0x6D1`, optionally queues, and returns `1`. (corrected 2026-07-10: helper
> identity verified through the shared `+0xE4/+0xE8` vector via
> `decompile_function 0x0065AE30`, `decompile_function 0x0065AE60`, and
> `decompile_function 0x0065AD90` — RTTI_LABEL_DRIFT)
> State 4 clears `+0x6D1` only after the `building+0x57C` guard, sets mission
> `0x0A`, optionally radios `3`, queues mission, and uses the timer epilogue.

> **Correction 2026-07-10 - live binary correction pass**
>
> The primary prose below now uses the verified dispatcher polarity, contact-vector
> helper identity, fixed `(-1,0)` refinery lookup, delayed post-drain empty check,
> unload-init accumulator reset, and exact state-4 radio call address. It also calls
> each threshold event a whole-slot drain rather than a "bale." (corrected 2026-07-10:
> verified via `decompile_function 0x0073D630`, `disassemble_function 0x0073D630`,
> `decompile_function 0x0049F2F0`, `decompile_function 0x00737BA0`,
> `decompile_function 0x0065AE30`, `decompile_function 0x0065AE60`, and
> `decompile_function 0x0065AD90` — STRUCT_FAMILY_CASCADE)

**Address:** 0x0073D630  
**Confidence:** HIGH — full decompile + disassembly verified in this session via
`decompile_function 0x0073D630` and `disassemble_function 0x0073D630`.  
**Active in YR:** Yes — every harvester (HARV/CMIN) ore delivery cycle. Also handles
MCV deploy (states 0–4 in the non-harvester branch) and unit-absorb deploy. The
refinery-unload path is distinguished by the harvester's `Harvester=yes`
(`UnitTypeClass+0xE0E`) flag being set.  
**Sole caller of ReleaseDockedHarvester:** Verified — `0x0073D66D CALL 0x004595C0`
is the only call site. Verified via `get_function_callers` on `0x004595C0` (prior session).

---

## 1. Overview

`Mission_Deploy_Building` is the per-tick mission handler invoked while a unit is in
Mission 0x10 (Unload). It serves a dual role: (a) harvester refinery-unload state machine
(the primary YR refinery path), and (b) MCV/deploy-type unit deploy state machine.

The function dispatch splits at entry on two conditions:

1. **`param_1[0xB9] == 0` (zero reciprocal dock-link path):**  
   This is the stock `HARV/CMIN -> GAREFN/NAREFN` DockUnload route. A later
   2026-05-22 reachability pass corrected this report's old `SizeLimit >= 1`
   wording: stock harvesters can enter the harvester-unload path through the
   live `Harvester=yes` type flag even with default `SizeLimit=0`.

2. **`param_1[0xB9] != 0` (conditional reciprocal-link path):**  
   Calls `ReleaseDockedHarvester` immediately if the building is still in cell,
   then falls through to post-release work. This is not the normal stock
   refinery dump-completion path.

The refinery-unload path (`param_1[0xB9] == 0`, stock harvester type) is driven by
`param_1[0xBC]` — the unit's `MissionSubState` field (byte offset `0xBC`). States
observed in the binary: **0, 1, 3, 4** (integer). No state 2 in the harvester switch.

---

## 2. State Variable Identification

| Field | Byte offset | Notes |
|-------|-------------|-------|
| `param_1[0xBC]` | `0xBC` | **FSM state** for the harvester unload (and MCV deploy) path. Verified: `MOV dword ptr [ESI + 0xBC], N` at multiple state-transition sites in disassembly. |
| `param_1[0x3E]` → `param_1+0xF8` | `0xF8` | **Drain-gate accumulator.** Reset to 0 at unload initialization and after each successful whole-slot drain. Gated by `HarvesterDumpRate × 900.0 ≤ counter`. Incremented externally (not inside this function). (corrected 2026-07-10: `MOV [ESI+0xF8],0` occurs at `0x0073DFD0`, `0x0073E493`, and `0x0073E4D0` via `disassemble_function 0x0073D630` — INFERENCE_HARDENED) |
| `byte [ESI + 0x6D1]` | `0x6D1` | **"First-entry-done" flag** — 0 on fresh arrival, set to 1 on state-3 initialisation. |
| `param_1[0x19D]` | `0x674` | Locomotion `ILocomotion*` pointer. Used only in the MCV/deploy sub-path. |
| `param_1[0x1B1]` | `0x6C4` | `UnitTypeClass*` — harvester's TypeClass pointer. |
| `param_1[0x5A4]` | `0x5A4` | `DockLink` / NavTarget (checked at state-4 exit). |
| `param_1[0xB4]` | `0x2D0` | Queued mission ID (checked at state-4 exit to detect override). |
| `ESI + 0x33C` | `0x33C` | Harvester's `StorageClass` (4 floats = 16 bytes). |

The switch dispatch table is at `0x73E5C0`. Valid case values entering via the
harvester switch: 0 (pre-dock approach), 1 (loco-type guard), 3 (deposit pulse loop),
4 (depart prep). Verified via jump table at `0073d707: JMP dword ptr [EAX*0x4 + 0x73e5c0]`.

---

## 3. State-by-State Decode

### Outer dispatcher — `param_1[0xB9] != 0` conditional reciprocal-link branch

**Address:** `0x0073D63B` — `CMP [ESI+0x2E4],0` followed by
`0x0073D641 JZ 0x0073D6E6`: zero jumps to the stock unload dispatcher, while
nonzero falls through to the conditional reciprocal-link branch and its
`ReleaseDockedHarvester` lookup. (corrected 2026-07-10: exact `CMP`/`JZ` sequence
verified via `disassemble_function 0x0073D630` — OPERATOR_OR_ORDER_DRIFT)

**Very early branch — linked-unit path (`param_1[0xB9] != 0`):**

`0x0073D641 JZ 0x0073D6E6` — falls through to the `ReleaseDockedHarvester` call site:

```
0073d647: MOV EAX, [ESI]             ; vtable
0073d649: CALL [EAX + 0x1BC]         ; GetMapCell()
0073d651: CALL 0x0047C520            ; Look_up_building_in_cell
0073d656: TEST EAX, EAX
0073d658: JZ 0x0073D672              ; if building gone, skip to LAB_0073d672
0073d65A: CALL [EAX + 0x1BC]         ; GetMapCell() on building
0073d666: CALL 0x0047C520            ; Look_up_building_in_cell
0073d66B: MOV ECX, EAX
0073d66D: CALL 0x004595C0            ; BuildingClass::ReleaseDockedHarvester
```

After `ReleaseDockedHarvester` returns (or if building not found), falls into
`LAB_0073d672`. This code does a post-release check:
- Reads `UnitTypeClass+0xE0E` (Harvester) and `UnitTypeClass+0xE0F` (Weeder).
- If neither set → routes through a timer/jitter return (tick-delay epilogue).
- If Harvester or Weeder → proceeds to the refinery-unload FSM at `0x0073DEE0`.

**Note: The `param_1[0xB9] != 0` path is the conditional path that calls
`ReleaseDockedHarvester`. The stock `param_1[0xB9] == 0` path is the harvester
unload FSM.**

---

### Harvester unload path — entered when `param_1[0xB9] == 0` on stock harvesters

Entered at `0x0073D6E6`. The outer FSM has a PathType step check and a timer check
before dispatching on `param_1[0xBC]`:

**Radio contact-vector guard at `0x0073DEE2` (`0x0065AE30`):**
- If any contact-vector entry is nonzero (`true`) → proceed to the RateTimer/state dispatch.
- If every contact-vector entry is zero (`false`) → cleanup branch: clear `+0x6D1`,
  optionally queue/commence the next mission, and return `1`.
- The direct `return 5` belongs to the RateTimer/facing wait path, not to the
  no-contact branch. (corrected 2026-07-10: `0x0065AE30` scans the pointer vector at
  `+0xE4` for `+0xE8` entries; `RadioClass::Set_Contact_Count @ 0x0065AE60` grows and
  clears that vector, and `RadioClass::FindDockSlot @ 0x0065AD90` searches it, via
  `decompile_function` on all three addresses — RTTI_LABEL_DRIFT)

**Timer guard at `0x0073DF56`:**
- `RateTimer::Current()` read. `((*timer >> 7) + 1 & 0x1FE) != 0x80` check.
- If not at the 0x80 "slot" → faces the harvester toward 0x4000 (east) via loco vtable+0x4C, returns 5.
- If at the 0x80 slot → dispatches on `param_1[0x6D1]` (first-entry flag).

---

### State-3 initialisation (first entry — `byte [ESI + 0x6D1] == 0`)

**Address:** `0x0073DFBD`  
**Transition from:** Pre-dispatch arrival check.

```
0073DFD0: MOV [ESI + 0xF8], 0       ; reset drain-gate accumulator to 0 (corrected 2026-07-10: exact write via `disassemble_function 0x0073D630` — INFERENCE_HARDENED)
0073DFDA: MOV [ESI + 0x6D1], 1      ; set "first-entry-done" flag
0073DFE0: MOV EAX, [g_CurrentFrameCounter]
0073DFE5: LEA EDX, [ESI + 0x100]    ; periodic-accumulator struct
0073DFED: MOV [ESI + 0x10C], 1      ; enable accumulator
0073DFF3: MOV [EDX + 0x00], EAX     ; +0x100 = start frame
0073DFF5: MOV [EDX + 0x04], iStack_8 ; +0x104 = (stack value — prev frame?)
0073DFFC: MOV [EDX + 0x08], 1       ; +0x108 = step = 1
```

Then, if Harvester flag set (`UnitTypeClass+0xE0E`):

```
0073E013: GetMapCell() + fixed DAT_0089F6A0 value (-1,0) → west-neighbor cell
0073E05F: Look_up_building_in_cell → building (this_00)
0073E065: if building found:
0073E067:   ObjectClass__GetHealthRatio(building)
0073E072:   FCOMP [g_Rules + 0x1700]     ; compare health vs ConditionYellow
0073E08A:   PUSH 0x7
0073E08E:   CALL 0x00451750              ; BuildingClass__SetAnimSlotImage(slot=7, low_health, 0)
0073E093: MOV [ESI + 0xBC], 3           ; → state 3 (deposit pulse loop)
```

**Verified:** Slot 7 (`PreProductionAnim`) fires exactly once on the first tick of unloading,
gated on `UnitTypeClass+0xE0E` (Harvester). For stock refineries (no `PreProductionAnim` defined),
this is a no-op inside `SetAnimSlotImage`. Transition immediately to state 3.

---

### State 3 — Threshold-Gated Whole-Slot Drain Loop

**Address:** `0x0073E2BF` (entered via jump table for `param_1[0xBC] == 3`)  
**Also enters here via `param_1[0x6D1] != 0` && `param_1[0xBC] == 3`.**

This is the hot path — runs every tick during unloading.

#### Step 3a — Locate the refinery building

```
0073E2C8: GetMapCell() + DAT_0089F6A0 offset → dock cell
0073E2F5: MapClass::Get_CellClass → CellClass*
0073E306: Look_up_building_in_cell → this_00 (building)
```

Superseded: `DAT_0089F6A0` is the hardcoded west-neighbor direction-table
offset `(-1,0)`, not a dock offset from `[GAREFN] DockingOffset0=` in artmd.ini.
This locates the refinery by looking one cell west of the harvester's current
dock/pad cell.

#### Step 3b — Building not found path (refinery destroyed)

```
0073E311: Radio contact-vector scan (CALL 0x0065AE30)
0073E31A: if any contact exists: PUSH 0x3; CALL [EDX + 0x274]  ; Transmit_Radio(3) = CLEAR_LINK
0073E32C: SetMission(Harvest=10, queued=1)
0073E338: → timer epilogue (jitter return)
```

**If refinery is gone mid-unload:** Function broadcasts radio cmd 3 (CLEAR_LINK) to clear
the dock-link if the unit's radio contact vector contains a nonzero entry, then immediately transitions the harvester to
Mission_Harvest (10) with a jitter return. No credits are awarded for the resource
amount still in storage —
the harvester returns to harvest with whatever remains in storage. (corrected 2026-07-10:
helper body and owning vector verified via `decompile_function 0x0065AE30`,
`decompile_function 0x0065AE60`, and `decompile_function 0x0065AD90` — RTTI_LABEL_DRIFT)

**Active in YR:** Yes — refinery destruction during unload is a normal in-game event.

#### Step 3c — Building found, gate check

```
0073E355: FILD dword ptr [ESI + 0xF8]     ; load drain-gate accumulator as float (corrected 2026-07-10: exact read via `disassemble_function 0x0073D630` — INFERENCE_HARDENED)
0073E361: FLD double ptr [EDX + 0x1528]   ; HarvesterDumpRate (double, rules+0x1528)
0073E367: FMUL double ptr [0x007E27F8]    ; × 900.0
0073E36D: FCOMPP                           ; compare: threshold vs accumulator
0073E371: TEST AH, 0x41
0073E374: JZ 0x0073E539                   ; if accumulator < threshold → skip (wait)
```

Gate: `HarvesterDumpRate × 900.0 ≤ param_1[0xF8]`. Default 0.016 × 900 = 14.4 frames.
Falls through to the drain block when threshold is crossed.

#### Step 3d — Whole-slot drain block (threshold crossed)

```
0073E37A: CALL [EAX + 0x468]             ; vtable+0x468 → FUN_00459900 (particle emitter)
0073E384: MOV EAX, [EDI + 0x584]         ; slot-10 SpecialAnim pointer
0073E38C: JNZ 0x0073E3BF                 ; if already playing → skip slot-10 call
0073E390: ObjectClass__GetHealthRatio
0073E3B6: PUSH 0xA
0073E3BA: CALL 0x00451750               ; BuildingClass__SetAnimSlotImage(slot=10, low_health, 0, 0)
```

**Order:** particle emitter fires first, then SetAnimSlotImage(slot=10). Both are
threshold-crossing events; if a resource slot exists, that event drains the entire
first nonempty slot. (corrected 2026-07-10: call order and
`RemoveAmount(GetAmount(slot), slot)` verified via `disassemble_function 0x0073D630`
at `0x0073E37E..0x0073E457` — INFERENCE_HARDENED)

```
0073E3BF: LEA ECX, [ESI + 0x33C]        ; harvester StorageClass
0073E3C5: CALL 0x006C9820               ; StorageClass__FindFirstNonEmptySlot → EBP
0073E3CC: CALL [EDX + 0x3C]             ; vtable+0x3C → GetOwner (HouseClass*)
                                         ; returns refinery owner (EBX)
0073E3D5: MOV EAX, [EBX + 0x538C]       ; facility_count (storage facility tally)
0073E3E3: JNZ ... (IsHuman test)
0073E3E9: g_GameMode test
0073E3F3: AIVirtualPurifiers bonus add
```

PurifierBonus formula:
```
facility_count = refinery_owner[+0x538C]
if !IsHuman && g_GameMode != 0:
    facility_count += AIVirtualPurifiers[owner[+0x184] × 4]  ; {4,2,0}[difficulty]
```

```
0073E40C: CMP EBP, -1
0073E40F: JZ 0x0073E423                 ; slot_index == -1 → amount = 0.0
0073E411: StorageClass__GetAmount(EBP) → float on FPU stack
```

**Drain is whole-slot in one call:**

```
0073E44B: MOV ECX, [ESP+0x18]           ; amount = GetAmount result
0073E44F: PUSH EBP                       ; slot_index
0073E450: PUSH ECX                       ; amount (whole slot)
0073E451: LEA ECX, [ESI + 0x33C]
0073E457: CALL 0x006C96B0               ; StorageClass__RemoveAmount(amount, slot)
                                         ; drains ENTIRE slot in one call
```

Post-drain credit award:
```
0073E46D: MOV AL, [EDX + 0xE0F]         ; Weeder=yes check
0073E47C: JZ standard_path             ; not Weeder → standard credits
  standard_path:
    0073E4A8: CALL 0x004F9610           ; HouseClass__Add_Tiberium_Credits(drained, slot)
    0073E4B4: bonus > 0 check
    0073E4C9: CALL 0x004F9610           ; HouseClass__Add_Tiberium_Credits(bonus, slot)
    0073E4D0: MOV [ESI + 0xF8], 0       ; reset accumulator
    0073E4DA: JMP 0x0073E539            ; → LAB_0073E539 (early-exit check)

  weeder_path (TS-legacy — never reached for HARV/CMIN):
    Math__ftol(drained)
    HouseClass__Add_Tiberium_To_Storage(ftol_result, slot)
    MOV [ESI + 0xF8], 0
    JMP 0x0073E539
```

**"No slot" path (all slots drained):**

```
0073E4DC: MOV AL, [EDX + 0x16BB]        ; Refinery=yes flag
0073E4EA: JZ skip_slot8               ; if not refinery, skip
0073E4EC: ObjectClass__GetHealthRatio
0073E513: PUSH 0x8
0073E517: CALL 0x00451750              ; SetAnimSlotImage(slot=8, low_health, 0, 0)
0073E51C: MOV [ESI + 0xBC], 4         ; → state 4 (depart prep)
0073E526: MOV EAX, [EDI + 0x584]      ; slot-10 SpecialAnim pointer
0073E52E: JZ 0x0073E539               ; if not playing, skip
0073E530: PUSH 0xA
0073E532: MOV ECX, EDI
0073E534: CALL 0x00451E40             ; BuildingClass__ClearAnimSlot(building, slot=0xA)
```

This path is evaluated only after the drain threshold is crossed. A successful
last-slot removal resets `+0xF8` and jumps to `0x0073E539`, so storage emptiness is
not observed on that same invocation. The next calls wait until
`HarvesterDumpRate × 900.0 <= +0xF8` is true again; only that later gated event
finds slot `-1` and enters state 4. (corrected 2026-07-10: verified from
`0x0073E355..0x0073E539` via `disassemble_function 0x0073D630` — OPERATOR_OR_ORDER_DRIFT)

**IMPORTANT:** The `ClearAnimSlot` argument is `0xA` (decimal 10), NOT `0xB`. This matches
`SetAnimSlotImage(slot=10, ...)` — both use slot index 10 (`SpecialAnim`). Slot index 10 decimal
= 0xA hex. `PUSH 0xA` confirmed at `0073E530`.

---

### State 3 — early-exit check at LAB_0073E539

```
0073E539: MOV EAX, [ESI + 0x5A4]       ; DockLink / NavTarget
0073E53F: TEST EAX, EAX
0073E541: JZ 0x0073E5B1                ; no override → return 1
0073E543: MOV EAX, [ESI + 0xB4]        ; queued mission
0073E549: CMP EAX, -1
0073E54C: JZ 0x0073E5B1                ; no queued mission → return 1
0073E54E: CMP EAX, 0xA                 ; Mission_Harvest = 10 (0xA)
0073E551: JZ 0x0073E5B1                ; queued is Harvest → return 1
; otherwise: mission override detected (Unload interrupted mid-cycle)
0073E553: MOV AL, [ECX + 0x16BB]       ; Refinery=yes
0073E55F: JZ skip_slot8_early
0073E563: SetAnimSlotImage(slot=8, low_health, 0, 0)
0073E594: MOV [ESI + 0xBC], 4          ; → state 4 (force-depart)
0073E5A8: PUSH 0xA
0073E5AC: CALL 0x00451E40              ; BuildingClass__ClearAnimSlot(slot=10)
```

The early-exit guard fires when a non-Harvest mission has been queued mid-unload (e.g.
player clicks Attack). It forces state 4 (depart) without awarding credits for the
partially-drained slot.

---

### State 4 — Depart Prep (non-Weeder path)

**Address:** `0x0073E17F` (entered from jump table for `param_1[0xBC] == 4`)

This is the exit handoff. For non-Weeder harvesters (`UnitTypeClass+0xE0F == 0`):

```
0073E181: GetMapCell() + DAT_0089F6A0 → dock cell
0073E1C6: Look_up_building_in_cell → building (iVar3)
; Guard: building exists AND Refinery=yes AND building[+0x57C] != 0
0073E1D5: MOV DL, [ECX + 0x16BB]      ; Refinery=yes
0073E1DF: CMP [EAX + 0x57C], EBX      ; +0x57C != 0 → "anim in progress"?
0073E1EA: JNZ 0x0073E5B1              ; if guard true → return 1 (wait another tick)
```

After guard passes:
```
0073E1F6: MOV [ESI + 0x6D1], 0        ; clear first-entry flag (reset for next visit)
; Check for mission override
0073E1FF: CMP [ESI + 0x5A4], EBX      ; DockLink == 0?
0073E207: CMP [ESI + 0xB4], -1        ; no queued mission?
0073E20C: CMP [ESI + 0xB4], 0xA       ; queued == Harvest?
; All true → normal exit path:
0073E24F: PUSH 0
0073E250: PUSH 0xA                     ; Mission_Harvest = 10
0073E254: CALL [EDX + 0x1E8]          ; SetMission(Harvest=10, queued=0)
0073E25A: CALL [EAX + 0x200]          ; IsInMap() or similar validation
; if validated:
0073E268: CALL 0x0065AE30             ; has_radio_contact?
0073E26F: JZ 0x0073E27F
0073E273: PUSH 0x3
0073E279: CALL [EDX + 0x274]          ; Transmit_Radio(3) → CLEAR_LINK
0073E27F: CALL [EAX + 0x1EC]          ; QueueMission
```

**Exit sequence verified:** State 4 sets `SetMission(Harvest=10, queued=0)`, optionally
sends radio cmd 3 (CLEAR_LINK) if radio contact still active, calls QueueMission, then
falls into the timer epilogue. (corrected 2026-07-10: indirect radio call is at
`0x0073E279`, while `0x0073E27F` begins QueueMission, via
`disassemble_function 0x0073D630` — GHIDRA_ADDRESS_SHIFT)

---

### Timer epilogue — all paths converge here

**Address:** `0x0073E289`

```
0073E28B: CALL 0x005B3A00             ; MissionClass__GetMissionTimerEntry
0073E290: FLD double [EAX + 0x10]     ; mission timer rate
0073E293: FMUL [0x007E27F8]           ; × 900.0
0073E299: CALL 0x007C5F00             ; Math__ftol
0073E29E: PUSH 0x2
0073E2A0: MOV ESI, EAX
0073E2A2: PUSH 0x0
0073E2A4: MOV ECX, [0x00A8B230]       ; g_Random (or g_Frame?) instance
0073E2AA: ADD ECX, 0x218
0073E2B0: CALL 0x0065C7E0             ; Random__RandomRanged(0, 2) → EAX
0073E2B5: ADD EAX, ESI
```

Returns `MissionTimerEntry × 900.0 (ftol) + Random(0, 2)`. This is the tick-delay before
the next Mission_Deploy_Building call. This fires every time the function returns without
a hard exit (return 1 / return 5 / return 10 / return 0xA).

---

## 4. Transmit_Radio Call Inventory — Q1 (0x19 LEAVE_DOCK) and Q2 (0x07 DOCKING_COMPLETE)

### All vtable+0x274 (Transmit_Radio) calls found in function body:

| Address | PUSH args before call | Radio cmd | Context |
|---------|----------------------|-----------|---------|
| `0x0073DD84` | `PUSH 0x3` | **cmd 3 = CLEAR_LINK** | Non-harvester deploy path, state 0 initialisation. NOT in harvester unload path. |
| `0x0073E16A` | `PUSH 0x3` | **cmd 3 = CLEAR_LINK** | Weeder state-4 exit, if has radio contact. |
| `0x0073E279` (argument pushed at `0x0073E275`) | `PUSH 0x3` | **cmd 3 = CLEAR_LINK** | Non-Weeder state-4 exit, if has radio contact. (corrected 2026-07-10: exact instruction addresses via `disassemble_function 0x0073D630` — GHIDRA_ADDRESS_SHIFT) |
| `0x0073E322` | `PUSH 0x3` | **cmd 3 = CLEAR_LINK** | State-3 refinery-destroyed path (radio clear before Mission_Harvest). |

**No other vtable+0x274 calls exist in this function.** The only radio command issued by
`Mission_Deploy_Building` to any contact is **cmd 3 (CLEAR_LINK)**, and only in exit
scenarios.

### Q1 — Does case 0x19 (LEAVE_DOCK) fire at end of cycle?

**ANSWER: NO.** There is no `PUSH 0x19` anywhere in `Mission_Deploy_Building`'s disassembly.
Radio cmd 0x19 (LEAVE_DOCK / CANCEL_DOCK) is **never transmitted from this function**. Verified
by exhaustive search of all `CALL dword ptr [EAX/EDX/ECX + 0x274]` sites in the disassembly —
every one is preceded by `PUSH 0x3`.

LEAVE_DOCK (0x19) is a `TechnoClass::Receive_Radio` base handler that clears the "docking/deploying"
flag on the unit. It may be sent by other callers (e.g. BuildingClass::MissionRepairAndProduce or
BuildingClass::Receive_Radio case 0x15), but **Mission_Deploy_Building never sends it**.

### Q2 — Does case 0x07 (DOCKING_COMPLETE) fire after the last drain event?

**ANSWER: NO.** There is no `PUSH 0x7` anywhere in `Mission_Deploy_Building`'s disassembly.
Radio cmd 0x7 (DOCKING_COMPLETE) is **never transmitted from this function**. Verified by
exhaustive search.

`DOCKING_COMPLETE` (0x7) is documented in `HARVESTER_DOCK_UNLOAD.md` as being sent by
`BuildingClass::MissionRepairAndProduce` state 1 to the docked unit. However, the refinery
unload path in YR does NOT go through `BuildingClass::MissionRepairAndProduce` for the
drain logic — that path is in `UnitClass::Mission_Deploy_Building` on the unit side. The
building's Mission 0x14 (`MissionRepairAndProduce`) branch is for UnitRepair/Hospital/Armory
etc., **not for Refinery/DockUnload**. The building's role in the refinery-dock sequence is
minimal (it receives radio 0x15 DOCK_NOW from `PerCellProcess` and responds by setting the
unit's mission to 0x10 Unload). The drain loop, state machine, and exit handoff all live
here in `Mission_Deploy_Building` on the **unit side**.

**CORRECTION to HARVESTER_DOCK_UNLOAD.md §4a:** That doc describes `BuildingClass::MissionRepairAndProduce`
as the dump handler, with a building-side state machine (field_0xBC states 0/1/2). This is
**WRONG for standard refineries**. The drain loop for HARV/CMIN is entirely in `UnitClass::Mission_Deploy_Building`.
The building's `MissionRepairAndProduce` only fires for UnitRepair/Bunker/Hospital/Armory/etc.
(see `HARVESTER_DOCK_UNLOAD.md §2.3 CORRECTION` which already noted this).

---

## 5. Anim Slot Trigger Sequence (Exact Ordering)

All anim calls within `Mission_Deploy_Building` call `BuildingClass::SetAnimSlotImage`
at `0x00451750` (verified address from `REFINERY_DOCK_ANIM_SLOTS_GHIDRA_REPORT.md`).

| Event | Slot index | Call address in function | Trigger condition |
|-------|-----------|--------------------------|-------------------|
| Dock arrival (one-time) | **7** (`PreProductionAnim`) | `0x0073E08E` | State-3 init block (`byte [ESI+0x6D1] == 0`), gated on `UnitTypeClass+0xE0E` (Harvester). One-shot on first tick only. |
| Per-gate pulse | **10 (0xA)** (`SpecialAnim`) | `0x0073E3BA` | State-3 threshold-crossing block, gated on `building[+0x584] == 0` (slot-10 anim not currently playing). (corrected 2026-07-10: trigger precedes whole-slot lookup/removal via `disassemble_function 0x0073D630` — INFERENCE_HARDENED) |
| Cargo empty / completion | **8** (`ProductionAnim`) | `0x0073E517` | State-3, after `FindFirstNonEmptySlot` returns -1 (all slots drained). Also fires at `0x0073E58F` on early-exit mid-unload. Gated on `Type[0x16BB]` (Refinery=yes). |
| Per-gate particle emitter | n/a | `0x0073E37E` (vtable+0x468) | State-3, fires **before** SetAnimSlotImage(10). Independent of slot-10 gate. (corrected 2026-07-10: trigger belongs to each threshold crossing, not to a fixed-size bale, via `disassemble_function 0x0073D630` — INFERENCE_HARDENED) |

**Order within the threshold-crossing block:**
1. `vtable+0x468` (particle emitter) — fires unconditionally on every gate-crossing.
2. `SetAnimSlotImage(10, ...)` — fires only if `building[+0x584] == 0`.
3. `StorageClass::FindFirstNonEmptySlot` — determines whether any ore remains.
4. `StorageClass::GetAmount` + `StorageClass::RemoveAmount` + `Add_Tiberium_Credits`.

**ClearAnimSlot** on slot 10:
- Called at `0x0073E534` (state 3 → state 4 transition, drain complete).
- Called at `0x0073E5AC` (early-exit, mission override path).
- **Slot arg: `PUSH 0xA`** (decimal 10) confirmed at both sites. Verified against disassembly.

---

## 6. Storage-Drain Loop (Whole-Slot Order and Timing Constants)

The unit of work is the first nonempty resource slot selected at each eligible
threshold crossing. (corrected 2026-07-10: `GetAmount(slot)` is passed to
`RemoveAmount(amount, slot)` via `disassemble_function 0x0073D630` —
INFERENCE_HARDENED)

### Per-fire drain is whole-slot, not a fixed-size bale

**Key finding (verified from binary):** Each time the gate condition fires
(`HarvesterDumpRate × 900 ≤ accumulator`), the code calls:
```
StorageClass__GetAmount(slot_index)        → full float value in slot
StorageClass__RemoveAmount(amount, slot)   → drains ENTIRE slot in one call
```

A standard War Miner (HARV) carrying only slot-0 ore drains that resource in **one
whole-slot event**. The same invocation resets `+0xF8` and returns; it does not
immediately perform another `FindFirstNonEmptySlot` call. (corrected 2026-07-10:
successful drain branches through `0x0073E4D0` to `0x0073E539`, not back to
`0x0073E3C5`, via `disassemble_function 0x0073D630` — OPERATOR_OR_ORDER_DRIFT)

Binary-derived sequence for a full single-slot harvester:
- one full drain-gate interval before slot 0 is removed;
- `+0xF8` resets to 0 after the successful drain;
- one additional full drain-gate interval before `FindFirstNonEmptySlot → -1` and
  the state-4 transition;
- state 4 then performs its depart-guard and mission handoff.

The exact wall-clock dwell also depends on mission-handler scheduling, so the former
`~17–20 frames` estimate was too short by the required final gate interval.
(corrected 2026-07-10: threshold branch, reset, return, and later no-slot branch
verified via `disassemble_function 0x0073D630` — OPERATOR_OR_ORDER_DRIFT)

For a harvester carrying both Ore (slot 0) and Gems (slot 1):
- first gate: slot 0 drained (ore), accumulator reset to 0;
- second gate: slot 1 drained (gems), accumulator reset to 0;
- third gate: `FindFirstNonEmptySlot → -1`, then state 4.

Thus this case requires three gate intervals before state 4, not two plus an
immediate next-tick empty check. (corrected 2026-07-10: verified via
`disassemble_function 0x0073D630` at `0x0073E355..0x0073E539` — OPERATOR_OR_ORDER_DRIFT)

### Slot drain order

`FindFirstNonEmptySlot` at `0x006C9820` scans from slot 0 upward. Slot 0 = Riparius/Ore
is always drained first. Slot 1 = Vinifera/Gems drains second. Slots 2 and 3 are
unused in YR. **Order: Ore first, then Gems.**

### Timing constants

| Constant | Value | Source | Purpose |
|----------|-------|--------|---------|
| `HarvesterDumpRate` | `0.016` (minutes) | `g_RulesClass_Instance + 0x1528` | Gate: `0.016 × 900 = 14.4` frames per drain event |
| `900.0` | IEEE-754 double `0x408C200000000000` | `DAT_007E27F8` | 60 sec × 15 fps multiplier |
| Timer rate gate | `((*timer >> 7) + 1) & 0x1FE == 0x80` | `RateTimer::Current()` | Throttles arrival dispatching to specific timer slot |
| Per-accumulator step | 1 / frame | External — `TechnoClass::AI_Update @ 0x006F9E50` | Increments `+0xF8` by `+0x110` every `+0x108` frames when `+0x10C != 0` |
| Unlimbo RNG consumption | `Random(0, 29)` (uniform) | `UnitClass::Unlimbo @ 0x00737BA0` | The RNG draw and initial `+0xF8` store occur, but unload initialization overwrites `+0xF8` with 0 at `0x0073DFD0`; the stored jitter does not seed unload cadence. (corrected 2026-07-10: verified via `decompile_function 0x00737BA0` plus `disassemble_function 0x0073D630` — INFERENCE_HARDENED) |

---

## 7. Stock State-4 Exit + Conditional Reciprocal-Link Release

> **Superseded 2026-05-22:** The original text in this section describes the
> conditional nonzero-`+0x2E4` branch, but incorrectly frames it as the normal
> stock refinery unload exit. The implementation-safe stock path is now:
> `Mission_Deploy_Building` state 4 on zero `unit+0x2E4`, wait on
> `building+0x57C` if set, clear `unit+0x6D1`, set/queue mission `0x0A`,
> optionally transmit radio `3`, and leave through the mission-timer epilogue.
> It does not call `ReleaseDockedHarvester`, does not `Force_Track(0x47)`, and
> does not install a fresh NavCom exit destination. Keep the body below only as
> historical context for the conditional reciprocal-link branch.

### The `param_1[0xB9] != 0` path — when does it fire?

The function is called once per tick as long as the unit is in Mission 0x10 (Unload).
When `param_1[0xB9] != 0`, it takes the conditional reciprocal-link branch and
calls `BuildingClass::ReleaseDockedHarvester` at `0x0073D66D`. The 2026-05-22
reachability audit verifies that this is not the normal stock CMIN/HARV
dump-completion branch.

**The stock unload state machine (states 1/3/4) runs when `param_1[0xB9] == 0`.**
State 4 is the stock exit path: it waits on `building+0x57C` if needed, clears
`unit+0x6D1`, sets or queues mission `0x0A`, optionally transmits radio `3`, and
leaves through the mission-timer epilogue without `ReleaseDockedHarvester` or
`Force_Track(0x47)`.

For full details on `BuildingClass::ReleaseDockedHarvester` (0x004595C0) see
[RELEASEDOCKEDHARVESTER_0x4595C0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/RELEASEDOCKEDHARVESTER_0x4595C0_GHIDRA_REPORT.md). That function handles:
- `ClearAnimSlot` slots 0xA and 0xB (teardown of unloading anims)
- `BunkerWallsDownSound` VOC
- `CreateAnimForSlot` slots 0xC and 0xD (`SpecialAnimThree`/`Four`)
- `Force_Track` exit locomotion with track 0x47
- `SetMission(MOVE=2)` on unit
- Dock teardown (both sides cleared)
- `RadioCommand(CLEAR=3)` to notify production system

### Post-release shared epilogue (LAB_0073D672)

After the conditional release branch returns, or when building lookup fails, the
function falls into `LAB_0073D672` which:
1. Checks `UnitTypeClass+0xE0E` / `+0xE0F` (Harvester / Weeder).
2. For standard harvesters: checks `UnitTypeClass+0x404` (size/type field).
3. Calls `thunk_FUN_005b2ef0` (timer helper) and returns the jitter tick-delay.

The next mission is branch-dependent. For the normal stock zero-link state-4
exit, mission scheduling is handled in `Mission_Deploy_Building`; the
conditional `ReleaseDockedHarvester` branch is not the stock CMIN/HARV exit.
See [STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md)
for the 2026-05-22 canonical reachability correction.

---

## 8. Edge Cases

### Refinery destroyed mid-unload (state 3)

When `Look_up_building_in_cell()` returns null in state 3 (`this_00 == NULL`):
```
0073E311: Radio contact-vector scan (CALL 0x0065AE30)
0073E31C: PUSH 0x3; CALL [EDX + 0x274]   ; conditional radio CLEAR_LINK (cmd 3)
0073E32C: SetMission(Harvest=10, queued=1)
0073E338: → jitter timer epilogue
```
No credits awarded for current in-progress drain. Harvester transitions immediately to
Mission_Harvest. **The radio CLEAR_LINK is conditional** - only sent if the
`0x0065AE30` scan finds a nonzero entry in the `+0xE4/+0xE8` contact vector. If
false, the CLEAR_LINK is skipped and only `SetMission(Harvest=10)` fires. No
crash, no hang. (corrected 2026-07-10: vector ownership and predicate verified via
`decompile_function 0x0065AE30`, `decompile_function 0x0065AE60`, and
`decompile_function 0x0065AD90` — RTTI_LABEL_DRIFT)

### Mission override mid-unload (e.g. player clicks Attack)

The early-exit check at `LAB_0073E539` fires when:
- `param_1[0x5A4]` (DockLink/NavTarget) is non-zero, AND
- `param_1[0xB4]` (queued mission) is not -1 AND not 0xA (Harvest)

When this triggers: fires `SetAnimSlotImage(slot=8)` (ProductionAnim), transitions to
state 4, clears slot-10 anim. Credits for the current partial slot are NOT awarded
(the `RemoveAmount` may or may not have fired on this same tick — if it did, credits
were awarded; if the early-exit fires before the gate check, they were not). No refinery
interaction occurs — the building is left with any remaining state it had.

### Harvester sold mid-unload

When a unit is sold, `ObjectClass::UnInit` is called which removes it from the mission
system. `Mission_Deploy_Building` will not be called again. The dock link is cleaned up by
the sell path (likely via `UndockUnit` or RadioCommand CLEAR broadcast). No special
branch in `Mission_Deploy_Building` handles the sell case — it simply stops being called.

### Temporal weapon / Chrono wipe

`BuildingClass::UndockUnit` (0x4593A0) handles chrono-wipe/temporal scenarios. That
function does NOT call `Mission_Deploy_Building` — it directly ejects the unit and clears
dock links. From `Mission_Deploy_Building`'s perspective, the unit simply stops being called
(same as sell case).

### Mind control during unload

Not gated inside `Mission_Deploy_Building`. The function operates on whatever unit is
executing it. If the unit is mind-controlled mid-unload, it may continue executing the
unload loop for the new owner. The refinery owner's credits are still determined by the
building's `GetOwner()` call (`vtable+0x3C`), so credits go to the refinery owner, not
the harvester's current controller. This is a subtle parity detail — credits go to the
building's owner regardless of who controls the harvester.

### Out-of-storage capacity at refinery

The refinery's own `Storage=` capacity is not checked by `Mission_Deploy_Building`. The
function drains the **harvester's** `StorageClass`, not the refinery's. There is no
capacity-overflow guard — `StorageClass::RemoveAmount` simply removes from the harvester's
storage and `Add_Tiberium_Credits` adds credits unconditionally. The refinery's storage
field is only used by `BuildingClass::UpdateAnimation` for the tier-display visual
(see `REFINERY_STORAGE_FLOW_GHIDRA_REPORT.md §3c`).

---

## 9. Tiny Details (Constants, Clamps, Off-by-Ones, Write Order, Early-Outs)

1. **`UnitClass::Unlimbo` consumes `Random(0, 29)`, but unload starts from 0.**
   Unlimbo stores the random result into `+0xF8`; state-3 unload initialization later
   overwrites `+0xF8` with 0 at `0x0073DFD0`. Preserve the RNG consumption, but do not
   use the stored result as the first unload-gate seed. (corrected 2026-07-10: verified
   via `decompile_function 0x00737BA0` and `disassemble_function 0x0073D630` —
   INFERENCE_HARDENED)

2. **Accumulator reset writes 0 to `[ESI + 0xF8]`** at `0073E4D0` and `0073E493`. The
   write happens **after** `Add_Tiberium_Credits` returns, not before. Order: RemoveAmount
   → drained > 0 check → Add_Tiberium_Credits(base) → Add_Tiberium_Credits(bonus) → reset.

3. **The last nonempty-slot drain and the empty completion gate are separate events.**
   The last successful drain fires the particle emitter and optional slot-10 image,
   removes the whole slot, resets `+0xF8`, and returns. After another full gate
   interval, a later event finds slot `-1`, fires slot 8 when applicable, and enters
   state 4. (corrected 2026-07-10: verified from `0x0073E355..0x0073E539` via
   `disassemble_function 0x0073D630` — OPERATOR_OR_ORDER_DRIFT)

4. **Slot 10 (`SpecialAnim`) is cleared with `PUSH 0xA`** (decimal 10). This is
   `BuildingClass::ClearAnimSlot(building, 10)`. The argument is the slot index, not a
   slot offset. `0xA == 10` decimal. Confirmed at `0073E530` and `0073E5A8`.

5. **`DAT_0089F6A0` is the fixed signed `(-1,0)` west-neighbor offset.** The function
   adds it to the harvester's current cell to find the refinery; it is initialized by
   `Foundation_direction_table_init @ 0x0049F2F0` and is not loaded from
   `DockingOffset0`. (corrected 2026-07-10: initializer and unload callsites verified
   via `decompile_function 0x0049F2F0` and `disassemble_function 0x0073D630` —
   INFERENCE_HARDENED)

6. **`building[+0x57C]` guard in state 4.** After the depart-prep path locates the
   building, it checks `[EAX + 0x57C] != 0` — if true, returns 1 (wait another tick).
   This is likely an "anim in progress" or "loco not ready" flag. The exact semantic of
   `BuildingClass+0x57C` is unverified but acts as a throttle before the unit fully departs.

7. **Timer epilogue returns `MissionTimerEntry × 900.0 (ftol) + Random(0, 2)`.** This is
   the tick-delay before the next Mission_Deploy_Building call. The separate Unlimbo
   `Random(0,29)` store is overwritten at unload initialization and does not add a
   second cadence jitter. (corrected 2026-07-10: both RNG sites and the intervening
   reset verified via `decompile_function 0x00737BA0` and
   `disassemble_function 0x0073D630` — INFERENCE_HARDENED)

8. **Credit write order is: base first, bonus second.** Both are separate `Add_Tiberium_Credits`
   calls. Any display that watches for a credit-change event will fire twice per drain —
   once for base amount, once for purifier bonus (if non-zero). The bonus is skipped if
   `bonus <= FLOAT_007E1748 (0.0f)`.

9. **Weeder path (`UnitTypeClass+0xE0F`) is present but TS-legacy.** It calls
   `Math__ftol(drained)` then `HouseClass__Add_Tiberium_To_Storage(ftol_result, slot)`.
   `Add_Tiberium_To_Storage` is at `0x004F9700`. This path is never reached in standard
   YR — standard YR has no Weeder harvester.

10. **No Teleporter (`UnitTypeClass+0xCD4`) check anywhere in `Mission_Deploy_Building`.**
    Confirmed by full decompile read. Zero Teleporter branches in the unload path.
    Identical path for HARV and CMIN. (Consistent with `RELEASEDOCKEDHARVESTER` finding.)

---

## 10. Diffs vs HARVESTER_DOCK_UNLOAD Docs (Corroborated / Corrected)

| Claim | Status | Notes |
|-------|--------|-------|
| "`BuildingClass::MissionRepairAndProduce` handles the refinery dump" (HARVESTER_DOCK_UNLOAD.md §4a) | **WRONG** | Refinery drain is entirely in `UnitClass::Mission_Deploy_Building`. `MissionRepairAndProduce` handles UnitRepair/Bunker/Hospital/etc., not Refinery/DockUnload. Already noted as CORRECTION in HARVESTER_DOCK_UNLOAD.md §2.3. |
| "State machine states 0/1/3/4" | **CORROBORATED** | Jump table at `0x73E5C0` confirmed, cases 0/1/3/4 present. |
| "FSM state at `UnitClass+0xBC`" | **CORROBORATED** | `MOV [ESI+0xBC], N` at all state-transition sites. |
| "Per-bale drain is whole-slot" | **MECHANISM CORROBORATED; TERMINOLOGY CORRECTED** | `RemoveAmount(GetAmount(slot), slot)` drains a whole resource slot per threshold crossing, not a fixed-size bale. (corrected 2026-07-10: verified via `disassemble_function 0x0073D630` at `0x0073E3C5..0x0073E457` — INFERENCE_HARDENED) |
| "HarvesterDumpRate × 900 ≤ counter gate" | **CORROBORATED** | `0073E361–0073E374` confirmed. |
| "Slot 7 fires on dock arrival, slot 10 per-gate, slot 8 on later empty completion" | **CORROBORATED WITH TIMING CORRECTION** | All three call sites are verified; slot 8 follows a later threshold crossing after the final successful drain reset. (corrected 2026-07-10: verified via `disassemble_function 0x0073D630` — OPERATOR_OR_ORDER_DRIFT) |
| "Particle emitter fires before SetAnimSlotImage(10)" | **CORROBORATED** | Order at `0x0073E37E` (vtable+0x468) before `0x0073E3BA` (SetAnimSlotImage). |
| "ClearAnimSlot slot 10 on completion" | **CORROBORATED with correction** | Slot index is `0xA` (decimal 10), confirmed `PUSH 0xA` at `0073E530`. |
| "Radio 0x07 DOCKING_COMPLETE fires after last bale" ([HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) §8.3) | **WRONG** | No `PUSH 0x7` anywhere in `Mission_Deploy_Building`. Radio 0x07 does not fire from this function. The `UnitClass::Receive_Radio case 7` is real; 2026-05-21 follow-up verifies the direct sender as the carryall pickup path and refutes `BuildingClass::MissionRepairAndProduce` as a 0x07 sender. |
| "Radio 0x19 LEAVE_DOCK fires at end of cycle" | **WRONG** | No `PUSH 0x19` anywhere in `Mission_Deploy_Building`. Not transmitted from this function. |
| "Only radio transmitted: cmd 3 (CLEAR_LINK), conditionally on exit" | **CONFIRMED NEW FINDING** | All four `CALL [EAX+0x274]` sites push 0x3. Two in state-4 exit, one in state-3 refinery-destroyed path, one in non-harvester-deploy path. |

---

## 11. Open Questions — Final State

| ID | Question | Status |
|----|----------|--------|
| OQ-1 | What is `BuildingClass+0x57C`? (state-4 guard in depart-prep) | **OPEN** — likely "anim-playing" or "loco-busy" flag. Semantic unverified. Does not affect credits or timing, only adds a 1-tick delay on depart. |
| OQ-2 | `DAT_0089F6A0` exact value — is it the same as `DockingOffset0` from artmd.ini? | **RESOLVED 2026-05-21** — value is `0x0000FFFF`, signed `(-1,0)`, initialized by `Foundation_direction_table_init @ 0x0049F2F0`; not from artmd.ini. |
| OQ-3 | When exactly is `param_1[0xB9]` set by the building side (linkage call)? | **OPEN** — the linkage call `FUN_004595C0` is invoked from `PerCellProcess` when the harvester reaches the pad. Exactly when during the tick cycle (before or after Mission_Deploy_Building on the same tick) determines whether states 3/4 can complete on the same tick as linkage. |
| OQ-4 | `Building::MissionRepairAndProduce` — does it run at all for DockUnload buildings? | **OPEN** — the building enters mission 0x14 on DOCK_NOW (radio 0x15). Whether that mission dispatches to MissionRepairAndProduce for Refinery buildings vs a null handler is unverified. If it does, it runs a building-side state machine in parallel with the unit-side FSM. |
| OQ-5 | `Radio cmd 0x07 DOCKING_COMPLETE` — what function actually sends it? | **RESOLVED 2026-05-21** — verified direct sender is `AircraftClass::Mission_Move_Carryall @ 0x00416D50`; not stock refinery and not `BuildingClass::MissionRepairAndProduce`. |
| OQ-6 | `UnitClass::Receive_Radio case 7` path — is it dead for harvesters? | **OPEN** — the case is present in the binary ([HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) §6) but if cmd 0x07 is never sent to a refinery-docked harvester, the case is never executed in normal play. |
| OQ-7 | `RulesClass+0x1528` vs earlier `0x16E8` claim in [HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) | **RESOLVED** — this function uses `0x1528` (`HarvesterDumpRate` as confirmed by INI parser). The `0x16E8` claim was incorrect. |

---

**2026-05-22 addendum to Open Questions:** OQ-1 is now resolved for stock
GAREFN/NAREFN: `building+0x57C` is the `Anims_0[8]` / ProductionAnim pointer,
and stock refineries normally do not wait there because slot 8 is empty/commented.
OQ-3 and OQ-4 are resolved for stock DockUnload reachability: normal ore unload
does not establish reciprocal `unit/building +0x2E4`, so the old "linkage call
from PerCellProcess" model is not stock refinery behavior. See
[STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md)
and [BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_NAVCOM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_NAVCOM_GHIDRA_REPORT.md).

## Sources

- **Ghidra MCP — functions decompiled this session:**
  - `0x0073D630` (`UnitClass::Mission_Deploy_Building`) — full decompile via `decompile_function 0x0073D630`
  - Full disassembly via `disassemble_function 0x0073D630`

- **Prior-art docs read (not re-decompiled):**
  - [STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_REACHABILITY_GHIDRA_REPORT.md) — 2026-05-22 canonical stock reachability correction for zero-link state-4 exit, the contact-vector predicate polarity, and conditional `ReleaseDockedHarvester` (corrected 2026-07-10: `0x0065AE30` role verified via `decompile_function 0x0065AE30`, `decompile_function 0x0065AE60`, and `decompile_function 0x0065AD90` — RTTI_LABEL_DRIFT)
  - [RELEASEDOCKEDHARVESTER_0x4595C0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/RELEASEDOCKEDHARVESTER_0x4595C0_GHIDRA_REPORT.md) — ReleaseDockedHarvester body (HIGH confidence)
  - `HARVESTER_DOCK_UNLOAD.md` — partial narrative; §4a building-side claim corrected here
  - [HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) — lifecycle doc; §8.3 radio-0x07 claim corrected here
  - `REFINERY_DOCK_ANIM_SLOTS_GHIDRA_REPORT.md` — anim slot context (HIGH confidence, corroborated)
  - `REFINERY_STORAGE_FLOW_GHIDRA_REPORT.md` — drain flow (HIGH confidence, corroborated)
  - [MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md) — approach choreography

- **INI files:** `ini/rulesmd.ini`, `ini/artmd.ini` (referenced for constant validation)

- **Key binary addresses verified this session:**
  - `0x0073D66D` — `CALL 0x004595C0` (ReleaseDockedHarvester call site)
  - `0x0073DD84`, `0x0073E16A`, `0x0073E279`, `0x0073E322` — all vtable+0x274 (Transmit_Radio) sites (corrected 2026-07-10: exact call instruction via `disassemble_function 0x0073D630` — GHIDRA_ADDRESS_SHIFT)
  - `0x0073E08E` — `CALL 0x00451750` with `PUSH 0x7` (SetAnimSlotImage slot 7)
  - `0x0073E3BA` — `CALL 0x00451750` with `PUSH 0xA` (SetAnimSlotImage slot 10)
  - `0x0073E517` — `CALL 0x00451750` with `PUSH 0x8` (SetAnimSlotImage slot 8)
  - `0x0073E37E` — `CALL [EAX + 0x468]` (particle emitter, before slot-10 call)
  - `0x0073E534` — `CALL 0x00451E40` with `PUSH 0xA` (ClearAnimSlot slot 10, completion)
  - `0x0073E5AC` — `CALL 0x00451E40` with `PUSH 0xA` (ClearAnimSlot slot 10, early-exit)
  - `0x0073E3C5` — `CALL 0x006C9820` (StorageClass__FindFirstNonEmptySlot)
  - `0x0073E457` — `CALL 0x006C96B0` (StorageClass__RemoveAmount — drains whole slot)
  - `0x0073E4A9` — `CALL 0x004F9610` (HouseClass__Add_Tiberium_Credits, base amount)
  - `0x0073E4C9` — `CALL 0x004F9610` (HouseClass__Add_Tiberium_Credits, bonus amount)
  - `0x007E27F8` — `900.0` (IEEE-754 double, frame-per-minute multiplier)
