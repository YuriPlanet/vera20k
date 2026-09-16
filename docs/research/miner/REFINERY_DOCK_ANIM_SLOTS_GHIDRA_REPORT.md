# Refinery Dock Animation Slots — Ghidra Research Report

**Address(es):**
- `0x73D630` — `UnitClass::Mission_Deploy_Building` (harvester dock FSM, source of all SetAnimSlotImage calls during unload)
- `0x451750` — `BuildingClass::SetAnimSlotImage` (slot art-variant selector)
- `0x451890` — `BuildingClass::CreateAnimForSlot` (anim instantiator)
- `0x45FE50` — `BuildingTypeClass::ReadINI` (verified slot↔INI key mapping)
- `0x459900` — BuildingClass `vtable+0x468` due-gate particle emitter (4 candidate emit points)

**Confidence:** HIGH for the corrected slot/call-site mechanism below. The 2026-07-10 live-binary audit corrected the older report's int-index/byte-offset confusion, per-bale interpretation, slot-10 retrigger claim, particle-count claim, storage-tier model, and unload RNG handoff.

**Active in YR:** Yes. The harvester dock path (`Mission == Unload`) is reached on every standard cargo cycle for War Miner / Chrono Miner. Refineries (`Refinery=yes` at `BuildingTypeClass+0x16BB`) are the primary consumer.

**Parent doc:** [BUILDING_ANIM_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_ANIM_STATE_MACHINE.md) — covers the full 21-slot table, damage state switching, power on/off, cloaking. This report extends it for the refinery-dock-specific case.

---

## 1. Overview

When a harvester docks at a refinery and unloads, gamemd has three slot-call roles: slot **7** at unload initialization, slot **10** at each due dump-gate crossing when slot 10 is currently null, and slot **8** on completion/early exit. The due-gate block also calls `vtable+0x468` before testing cargo. It therefore runs once per timed dump attempt, including the final attempt that discovers no nonempty cargo slot; it is not one trigger per bale. `(corrected 2026-07-10: was “one per bale”; decompile_function 0x0073D630 shows vtable+0x468 and slot 10 before FindFirstNonEmptySlot, with the empty result then taking slot 8/state 4 — OPERATOR_OR_ORDER_DRIFT)`

**The building-side ActiveAnim tier display is independent of the unit-side slot 7/10/8 calls, but it is not four simultaneous loops.** For `Type+0x16BB != 0`, `BuildingClass::UpdateAnimation` computes `tier = floor(total_storage) == 0 ? 0 : floor(total_storage * 4) / Storage`, clears the old slot when the cached tier changes, and selects exactly one of slots 3–6 (tier 0/1/2/3+). The unit unload FSM does not directly toggle those tier slots. `(corrected 2026-07-10: was “slots 3–6 loop forever simultaneously”; decompile_function 0x004509D0 and get_assembly_context 0x00450E0D/0x00450F99 show the single-tier clear/create switch — INFERENCE_HARDENED)`

What changes when a harvester arrives is purely the addition of one-shot per-event anims layered on top:

| Event | Slot | INI Key | Allied refinery anim | Defined? |
|-------|------|---------|----------------------|----------|
| Dock arrival (one-time) | 7 | `PreProductionAnim` | (none) | No → call is no-op |
| Due dump-gate pulse (threshold 14.4 accumulator units) | 10 | `SpecialAnim` | `GAREFNOR` | **Yes when slot 10 is null** → visible one-shot |
| Cargo empty / completion (one-time) | 8 | `ProductionAnim` | (none) | No → call is no-op |
| Due dump-gate particle burst | n/a | (rules side) | one system per configured nonzero offset | **Yes** → visible |

So for stock RA2/YR refineries, the **only sprite animation actually created by these unit-side dock calls is `SpecialAnim` (slot 10)**. It is attempted at each due dump gate only while `building+0x584` (the slot-10 AnimClass pointer) is null. Slot 7 is gated by the unit-type harvester byte `+0xE0E` and an adjacent building lookup; slot 8 is gated by the building-type `Refinery` byte `+0x16BB`. Stock refineries leave `PreProductionAnim` and `ProductionAnim` empty, so those calls short-circuit. `(corrected 2026-07-10: was “slot 10 looping per bale” and treated slots 7/8 as the same refinery gate; decompile_function 0x0073D630 and decompile_function 0x00451750 — OPERATOR_OR_ORDER_DRIFT)`

This matters because the current Rust event path is closer than the historical implementation but still differs in event timing and slot lifetime: it emits only after successful whole-slot drains, resets an occupied SpecialAnim, omits the final empty-gate pulse and slot-10 clear, and specializes refinery ActiveAnim rendering to stock tier 0. See Section 7.

---

## 2. Class Layout / Key Offsets

### BuildingTypeClass slot table

The 21-slot table starts at `BuildingTypeClass + 0xF4C`. Each slot is `0x44` bytes. Slot index = `(field_offset − 0xF4C) / 0x44`.

| Slot | INI Key | Slot Offset | Verification |
|------|---------|-------------|--------------|
| 3 | `ActiveAnim` | `+0x1018` | `LEA EDX,[EBP + 0x1018]` at 0x4617f1 |
| 4 | `ActiveAnimTwo` | `+0x105C` | `LEA EDX,[EBP + 0x105c]` at 0x461a9f |
| 5 | `ActiveAnimThree` | `+0x10A0` | `LEA EDX,[EBP + 0x10a0]` at 0x461d4d |
| 6 | `ActiveAnimFour` | `+0x10E4` | `LEA EDX,[EBP + 0x10e4]` at 0x461ffb |
| 7 | `PreProductionAnim` | `+0x1128` | `LEA EDX,[EBP + 0x1128]` at 0x464259 |
| 8 | `ProductionAnim` | `+0x116C` | `LEA EDX,[EBP + 0x116c]` at 0x463d75 |
| 9 | `TurretAnim` | `+0x11B0` | `LEA EDX,[EBP + 0x11b0]` at 0x464489 |
| 10 | `SpecialAnim` | `+0x11F4` | `LEA EDX,[EBP + 0x11f4]` at 0x462d61 |
| 18 | `IdleAnim` | `+0x1414` | `LEA EDX,[EBP + 0x1414]` at 0x463fab |

**Layout within one slot (0x44 bytes total):**

| +Offset | Type | Purpose |
|---------|------|---------|
| 0x00 | char[16] | Undamaged anim name |
| 0x10 | char[16] | Damaged anim name |
| 0x20 | char[16] | Firing anim name |
| 0x30 | int[2] | X,Y pixel draw offsets |
| 0x38 | int | ZAdjust |
| 0x3C | int | YSort |
| 0x40 | byte | `XXXPowered` flag (default 1) |
| 0x41 | byte | `XXXPoweredLight` flag (default 0) |
| 0x42 | byte | `XXXPoweredEffect` flag (default 0) |
| 0x43 | byte | `XXXPoweredSpecial` flag (default 0) |

The old `+0x30 AnimTypeClass*` row was a layout error: `CreateAnimForSlot` passes `Type + slot*0x44 + 0xF7C` (slot-local `+0x30`) to the 2-D offset transform, then separately reads slot-local `+0x38/+0x3C`. The anim type is resolved from the selected name, and slot-local `+0x3C` is the `…YSort` value (for slot 10, `SpecialAnimYSort` writes Type `+0x1230`). `(corrected 2026-07-10: decompile_function 0x00451890 plus get_assembly_context 0x00462F46 — OFFSET_RETYPED_WRONG)`

### Key BuildingTypeClass flags

| Offset | Field | Purpose |
|--------|-------|---------|
| `+0x16BB` | `Refinery=yes` flag | Gates slot 8 call. Verified: `MOV byte ptr [EBP + 0x16bb], AL` after `ReadBool("Refinery")` at 0x460a6c |
| `+0x16B3` | `DockUnload=yes` flag | Building accepts harvester docking |
| `+0x584` | AnimClass* for slot 10 | Gates slot 10 call (must be null to create) |

### UnitClass dock state

| Offset | Field | Purpose |
|--------|-------|---------|
| `+0xBC` | int | FSM state field for Mission_Deploy_Building (cases 0/1/3/4) |
| `+0xF8` | int | Dump-gate accumulator. Reset to 0 on unload initialization and after each successful whole-slot drain. Compared against `HarvesterDumpRate × 900.0` |
| `+0x2E4` | dword | Outer branch discriminator; zero takes the normal stock refinery-unload path. It is not established here as a mirrored docked-building pointer |
| `+0x6D1` | byte | "First-entry already done" flag for inner-FSM state 1 |

The former `+0x2F/+0x3E/+0xB9` entries copied Ghidra's `int *` indices as byte offsets. Assembly reads/writes `[ESI+0xBC]`, `[ESI+0xF8]`, and `[ESI+0x2E4]`. `(corrected 2026-07-10: search_instructions 0x0073D630 operands 0xBC/0xF8/0x2E4 and get_assembly_context 0x0073D63B/0x0073DFD0 — PARAM1_TYPE_MISREAD)`

---

## 3. Core Logic — the four trigger sites

All four SetAnimSlotImage call sites live inside `Mission_Deploy_Building` at `0x73D630`. The outer dispatcher compares unit byte offset `+0x2E4` with zero; the zero branch at `0x73D641` is the normal stock refinery-unload path and contains the slot calls. Within it, unit `+0xBC` is the FSM state. `(corrected 2026-07-10: was “nonzero/linked path only”; decompile_function 0x0073D630 and get_assembly_context 0x0073D63B show `JZ 0x0073D6E6` into the unload FSM — OPERATOR_OR_ORDER_DRIFT)`

### Trigger 1 — Dock arrival, one-shot

**Address:** `0x73E08E`
**FSM context:** unit `+0xBC == 1`, only on first entry where unit `+0x6D1 == 0`
**Gate:** `*(char *)(param_1[0x1b1] + 0xe0e) != 0` — the Harvester `Refinery=yes`-equivalent flag on the unit's TechnoTypeClass (CMIN/HARV)

```c
BuildingClass__SetAnimSlotImage(7, dVar17 <= *(double *)(g_RulesClass_Instance + 0x1700), 0);
//                               ^slot   ^low-health flag (health <= ConditionYellow)
// ASM: PUSH 0x0; PUSH 0x0; PUSH EAX; PUSH 0x7; MOV ECX,EDI; CALL 0x451750
```

Then: `MOV [ESI+0xBC], 3` — transition FSM to state 3 (dump-gate loop).

**Effect on stock refineries:** No-op. GAREFN/NAREFN don't define `PreProductionAnim`, so the slot-7 art name is empty and `SetAnimSlotImage` short-circuits. The call is defensive — gamemd makes it for any unit with a Harvester-side flag, regardless of whether the building has art.

**Effect on mods:** Plays a one-shot `PreProductionAnim` if defined.

### Trigger 2 — Due dump-gate pulse (the visible one)

**Address:** `0x73E3BA`
**FSM context:** unit `+0xBC == 3`, gated on the dump accumulator
**Gate:** `*(double *)(g_RulesClass_Instance + 0x1528) * _DAT_007e27f8 <= (double)*(int *)(unit+0xF8)` — i.e., `HarvesterDumpRate × 900.0 ≤ accumulator`

The constant `_DAT_007E27F8` is the IEEE-754 double **`900.0`** (`0x408C200000000000`). Default `HarvesterDumpRate=0.016` gives a threshold of 14.4 accumulator units, so the integer accumulator crosses it at 15. This schedules dump attempts, not individual bales. `(corrected 2026-07-10: decompile_function 0x0073D630 and get_assembly_context 0x0073E35B — INFERENCE_HARDENED)`

```c
// Inner gate within the due dump pulse:
if (*(int *)&this_00->field_0x584 == 0) {
    BuildingClass__SetAnimSlotImage(10, dVar17 <= *(double *)(g_RulesClass_Instance + 0x1700), 0, 0);
//                                   ^slot 10 = SpecialAnim
    // ASM: PUSH 0x0; PUSH 0x0; PUSH EAX; PUSH 0xa
}
```

Plus immediately before this block: `(**(code **)(this_00->vtable + 0x468))()` — the particle emitter (see Section 4). The emitter runs even when slot 10 is already occupied and even on the final empty-cargo attempt.

**Effect on stock refineries:** The first due gate with a null slot 10 creates `GAREFNOR` (Allied) or `NAREFNOR` (Soviet). Later due gates do **not** pre-empt it while `building+0x584` is non-null; they skip SetAnimSlotImage but still emit smoke. `CreateAnimForSlot` can replace an occupied slot in general, but this caller's null check prevents that replacement path. `(corrected 2026-07-10: was “restarts every bale”; decompile_function 0x0073D630 plus decompile_function 0x00451890 — OPERATOR_OR_ORDER_DRIFT)`

### Trigger 3 — Cargo empty / completion, one-shot

**Address:** `0x73E517`
**FSM context:** unit `+0xBC == 3`, after the due-gate smoke/slot-10 block and `StorageClass::FindFirstNonEmptySlot()` returns "no slots left"
**Gate:** `this_00->Type[0x16bb] != 0` — the building's `Refinery=yes` flag

```c
BuildingClass__SetAnimSlotImage(8, dVar17 <= *(double *)(g_RulesClass_Instance + 0x1700), 0, 0);
//                               ^slot 8 = ProductionAnim
// ASM: PUSH 0x0; PUSH 0x0; PUSH EAX; PUSH 0x8
```

Then: `MOV [ESI+0xBC], 4` — transition FSM to state 4 (departure prep), followed by `ClearAnimSlot(10)` when `building+0x584` is non-null. Thus the completion attempt clears an active SpecialAnim; it does not leave a post-unload wind-down. `(corrected 2026-07-10: get_assembly_context 0x0073E517 shows the state write and conditional call 0x00451E40 with slot 10 — OPERATOR_OR_ORDER_DRIFT)`

**Effect on stock refineries:** No-op. GAREFN/NAREFN don't define `ProductionAnim`. Defensive.

### Trigger 4 — Mid-deposit early exit on completion

**Address:** `0x73E58F`
**FSM context:** unit `+0xBC == 3`, alternative path entered when unit `+0x5A4 != 0 && +0xB4 != -1 && +0xB4 != 10`
**Gate:** Same `Type[0x16bb] != 0`

Identical call: `SetAnimSlotImage(8, low_health, 0, 0)`. Same effect (no-op on stock).

This is the "bailed out of unload partway through (e.g., something assigned a non-Unload mission)" path.

---

## 4. The Particle Emitter (BuildingClass vtable+0x468 → FUN_00459900)

In addition to the conditional slot-10 sprite call, every due dump-gate block calls `BuildingClass`'s vtable slot 0x468, which is `FUN_00459900`. The vtable ownership is verified independently: vtable base `0x007E3EBC` has slot `+0x468 = 0x00459900`, and its COL points to TypeDescriptor `.?AVBuildingClass@@`. `(corrected/verified 2026-07-10: read_memory 0x007E3EB8/0x007FC360/0x00818D60 and get_xrefs_to 0x00459900)`

```c
void FUN_00459900(BuildingClass *param_1) {
    int type = *(int *)(param_1 + 0x520);  // Type pointer
    int particle_id = *(int *)(type + 0x774);
    int smoke_frames = *(int *)(type + 0x156c);
    if (particle_id == 0) return;

    // For each of 4 candidate offsets at Type+0x7CC, +0x7D8, +0x7E4, +0x7F0
    for (int i = 0; i < 4; i++) {
        Coord3 offset = *(Coord3 *)(type + 0x7CC + i * 0xC);
        if (offset != NullCoordA && offset != NullCoordB) {
            Coord3 spawn_pos = building_pos + offset;
            ParticleSystemClass *ps = ParticleSystemClass::Spawn(particle_id, spawn_pos);
            ps->field_0xEC = smoke_frames;
        }
    }
}
```

The older pseudocode spawned all four candidates and called `Type+0x156C` a palette slot. The binary first rejects a null particle-system ID, skips candidates equal to either null-coordinate sentinel (both are zero in the static image), and copies `+0x156C` to the spawned particle system's `+0xEC` field through `0x006301F0`. `(corrected 2026-07-10: decompile_function 0x00459900/0x006301F0, get_assembly_context 0x0045994A/0x004599CD, and read_memory 0x0089C848 — OPERATOR_OR_ORDER_DRIFT/OFFSET_RETYPED_WRONG)`

**INI source:** The four offsets and the particle system ID come from `[GAREFN]`'s rules-side keys (not art):

| Rules key | Maps to | Allied refinery value |
|-----------|---------|------------------------|
| `RefinerySmokeOffsetOne` | Type+0x7CC | `-92, -208, 312` |
| `RefinerySmokeOffsetTwo` | Type+0x7D8 | `-92, 208, 312` |
| `RefinerySmokeOffsetThree` | Type+0x7E4 | (default 0,0,0 — undefined for Allied) |
| `RefinerySmokeOffsetFour` | Type+0x7F0 | (default 0,0,0 — undefined for Allied) |
| `RefinerySmokeParticleSystem` | Type+0x774 | `SmallGreySSys` |
| `RefinerySmokeFrames` | Type+0x156c | `50` |

The Allied refinery only defines two nonzero offsets (`One` and `Two`), so only those two candidate sites spawn systems. Undefined zero offsets Three/Four are skipped; they do not emit at the building origin.

**Active in YR:** Yes. Confirmed by the live call site. The emitter is independent of the slot-10 null gate and fires on every due dump attempt, including the final empty attempt. `(corrected 2026-07-10: decompile_function 0x0073D630 — OPERATOR_OR_ORDER_DRIFT)`

---

## 5. INI Keys

### Allied Refinery (GAREFN) anim definitions

From `artmd.ini` `[GAREFN]`:

```ini
ActiveAnim=GAREFNL1            ; slot 3, looping (GAREFNL1: LoopCount=-1, LoopEnd=3, Rate=200ms = 800ms cycle)
ActiveAnimTwo=GAREFNL2         ; slot 4, looping
ActiveAnimThree=GAREFNL3       ; slot 5, looping
ActiveAnimFour=GAREFNL4        ; slot 6, looping
SpecialAnim=GAREFNOR           ; slot 10, ONE-SHOT (LoopCount=1, LoopEnd=19, Rate=200ms = 4s anim)
; PreProductionAnim — UNDEFINED (slot 7 call is no-op)
; ProductionAnim — UNDEFINED (slot 8 call is no-op)
; IdleAnim — UNDEFINED
```

From `rulesmd.ini` `[GAREFN]`:

```ini
RefinerySmokeOffsetOne=-92, -208, 312
RefinerySmokeOffsetTwo=-92, 208, 312
RefinerySmokeFrames=50
RefinerySmokeParticleSystem=SmallGreySSys
DockUnload=yes
Refinery=yes                   ; sets Type+0x16BB, gates slot-8 call
NumberOfDocks=1                ; informational; refinery uses single-slot dock at +0x2E4
Storage=200                    ; max ore stored at refinery
```

### Soviet Refinery (NAREFN) anim definitions

Same structure as Allied. Defines `Damaged=` variants (`NAREFNL1D`–`NAREFNL4D`) on each ActiveAnim. SpecialAnim = `NAREFNOR`.

### Yuri Slave Miner Refinery (YAREFN)

```ini
IdleAnim=YAREFN_A              ; slot 18, looping
; No ActiveAnim (slot 3-6 calls produce nothing)
; No SpecialAnim (slot 10 call is no-op)
; No DockUnload — uses entirely separate slave-miner system
```

YAREFN does **not** use the harvester dock FSM at all. Slave miners deploy *into* the refinery, not dock at it. Different code path (`SlaveManagerClass::AI_Update` at 0x6AF6C0).

### Anim definitions (artmd.ini)

```ini
[GAREFNL1]   ; through L4 — the always-looping conveyor anims (slots 3-6)
Normalized=yes
LoopStart=0
LoopEnd=3
LoopCount=-1                   ; infinite loop
Rate=200                       ; 200ms per frame → 4 frames × 200ms = 800ms cycle
Layer=ground

[GAREFNOR]   ; the due-dump "ore arriving" SpecialAnim (slot 10)
Normalized=yes
LoopStart=0
LoopEnd=19                     ; 20 frames
LoopCount=1                    ; ONE-SHOT
Rate=200                       ; 200ms per frame → ~4s total anim length
Layer=ground
```

**Tiny detail:** `GAREFNOR` is not restarted while slot 10 is occupied. On the final empty-cargo gate, state 3 conditionally creates it only if null, then the completion path conditionally clears slot 10 in the same block. Therefore this FSM does not guarantee a post-unload tail/wind-down; an active SpecialAnim is cleared on completion. `(corrected 2026-07-10: decompile_function 0x0073D630 and get_assembly_context 0x0073E517 — OPERATOR_OR_ORDER_DRIFT)`

---

## 6. Integration Points

### Tick cycle position

`Mission_Deploy_Building` runs through the unit mission dispatcher, while `BuildingClass::UpdateAnimation` is reached through `BuildingClass::Update` (`0x0043FB20`). This report has not verified a universal unit-before-building scheduler order, so it does **not** establish same-tick frame-0 visibility or rule out a one-tick difference. `(corrected 2026-07-10: was a fixed three-step order unsupported by the cited bodies; decompile_function 0x0073D630 and get_function_callers 0x004509D0 identify separate object update paths — INFERENCE_HARDENED)`

### Dump-gate accumulator increment

Unit `+0xF8` is the dump-gate accumulator. `Mission_Deploy_Building` resets it at unload initialization (`0x73DFD0`) and after each successful drain (`0x73E493` or `0x73E4D0`). `TechnoClass::AI_Update` advances it through the periodic-accumulator cluster `+0xF8/+0x100/+0x104/+0x108/+0x10C/+0x110`. With the harvester's active period/step of 1, the 14.4 threshold is crossed at 15; the event is a whole-slot dump attempt, not a bale. `(corrected 2026-07-10: decompile_function 0x006F9E50, search_instructions 0x0073D630 operand 0xF8, and get_assembly_context 0x0073DFD0/0x0073E35B — INFERENCE_HARDENED/PARAM1_TYPE_MISREAD)`

### Refinery's continuous ActiveAnim

The refinery ActiveAnim tier display lives in `BuildingClass::UpdateAnimation` (`0x4509D0`). It is not toggled by harvester arrival, but only one of slots 3–6 is selected from the building's storage tier at a time. Power-transition details remain covered by the parent animation-state report and were not re-audited here. `(corrected 2026-07-10: decompile_function 0x004509D0 — INFERENCE_HARDENED)`

### Mission flow into Mission_Deploy_Building

`Mission_Harvest` (`0x73E5E0`) routes the cargo-full return into mission 10, whose unit mission dispatch reaches `Mission_Deploy_Building`. Normal stock unload does not consume a mirrored building pointer at unit `+0x2E4`; the zero branch rediscovers the refinery from the unit's cell plus the engine's adjacent lookup vector at each relevant state. `(corrected 2026-07-10: decompile_function 0x0073D630 and get_assembly_context 0x0073D63B/0x0073E05F — PARAM1_TYPE_MISREAD/INFERENCE_HARDENED)`

After the state-3 slot-8/slot-10 completion work, state 4 performs its contact/radio-side exit sequence separately: `0x0065AE30` scans the receiver's pointer vector at `+0xE4` with count `+0xE8`; when nonempty, the call at `0x73E279` invokes virtual `+0x274` with argument `3`, followed by virtual `+0x1EC` at `0x73E283`. The animation calls therefore do not depend on a persistent mirrored refinery pointer or on this later contact-vector test. `(verified 2026-07-10: decompile_function 0x0065AE30 and get_assembly_context 0x0073E26F/0x0073E279/0x0073E27F — RTTI_LABEL_DRIFT)`

---

## 7. Current Rust Implementation Status

### What we have

- `dock_active_anim` has been removed.
- [miner_dock_sequence.rs](../../../src/sim/miner/miner_dock_sequence.rs) drains one whole StorageClass resource slot per successful due gate and emits one `BaleDepositEvent` for that successful drain (`phase_unloading`, around line 1275).
- [app_building_anim.rs](../../../src/app_building_anim.rs) consumes each event by creating **or resetting** the SpecialAnim overlay and spawning one particle system per nonzero configured smoke offset (around lines 417–558).
- [shp.rs](../../../src/app_instances/shp.rs) suppresses non-primary ActiveAnim variants for refineries and renders only the primary slot for the current stock direct-credit model (around lines 591–603).

`(corrected 2026-07-10: the old section described pre-removal Rust; source scans of miner_dock_sequence.rs/app_building_anim.rs/shp.rs plus decompile_function 0x0073D630 and 0x004509D0 establish the current comparison — RUST_STATUS_STALE)`

### What's wrong vs gamemd

1. **The event stream is one event short.** Rust emits only after a successful whole-slot drain; gamemd runs smoke and the slot-10 null-gated attempt before checking cargo, so the final empty-cargo due gate also produces the pulse side effects before slot 8/state 4.
2. **Rust resets an existing SpecialAnim.** The native caller skips SetAnimSlotImage while `building+0x584` is non-null; Rust overwrites the existing overlay state for every event.
3. **Completion does not clear the Rust SpecialAnim overlay in the native order.** Native completion conditionally clears slot 10 after slot 8/state 4.
4. **`RefinerySmokeFrames` is parsed but not consumed by the refinery event path.** Native `0x00459900` writes Type `+0x156C` to each spawned particle system's `+0xEC`.
5. **No slot 7 / slot 8 calls.** This is a stock no-op for absent art but remains a mod-visible mismatch.
6. **The storage-tier renderer is stock-specialized.** Native code selects slots 3–6 from actual building storage; Rust currently forces refinery tier 0 under its stock direct-credit model rather than implementing the general binary mechanism.

`(corrected 2026-07-10: decompile_function 0x0073D630/0x00459900/0x006301F0/0x004509D0; current Rust confirmed by source scans above — OPERATOR_OR_ORDER_DRIFT/RUST_STATUS_STALE)`

### What needs to change for parity

| Area | Current | gamemd-correct |
|------|---------|----------------|
| Dump-pulse event timing | Successful whole-slot drains only | Emit side effects at every due gate, including the final empty check |
| Slot-10 occupancy | Existing overlay is reset | Create only when slot 10 is null; clear it on completion/early exit in native order |
| Smoke frames | Parsed, unused here | Apply `RefinerySmokeFrames` to each spawned particle-system lifetime field |
| Smoke offsets | Correctly skips zero offsets | Preserve one spawn per nonzero offset (up to four) |
| Slots 7 and 8 | Not implemented | Add equivalent init/completion triggers (stock no-op; mod-visible) |
| ActiveAnim tiers | Stock tier 0 only | Implement native storage-derived single-tier selection where building storage is modeled |

---

## 8. Tiny details worth recording

These are the details that compound into parity drift if missed.

- **Slot 7 and slot 8 have different gates.** Slot 7 requires the unit-type harvester byte `+0xE0E` plus a successful adjacent-building lookup; slot 8 requires building Type `+0x16BB` (`Refinery`). Both short-circuit on an empty selected art name. `(corrected 2026-07-10: decompile_function 0x0073D630/0x00451750 — OPERATOR_OR_ORDER_DRIFT)`
- **Slot 10 is gated on `building+0x584 == 0`.** `+0x584` is the slot-10 AnimClass pointer, not an invisibility flag. Particles still fire while it is non-null, so sprite and smoke are deliberately decoupled. `(corrected 2026-07-10: decompile_function 0x00451890/0x004509D0 and get_assembly_context 0x0073E35B — OFFSET_RETYPED_WRONG)`
- **The damaged selector does not fall back.** `health <= Rules+0x1700` selects the damaged name at slot-local `+0x10`; if that string is empty, `SetAnimSlotImage` returns without trying the undamaged name. Stock GAREFN has no damaged SpecialAnim name, so a damaged refinery's slot-10 dock call is a no-op. `(corrected 2026-07-10: decompile_function 0x00451750 — OPERATOR_OR_ORDER_DRIFT)`
- **Particle emitter fires BEFORE the conditional SetAnimSlotImage(10).** Exact order at the due gate is vtable+0x468, slot-10 pointer test/call, cargo-slot lookup/drain, then possible completion slot 8/clear. `(verified 2026-07-10: decompile_function 0x0073D630 and get_assembly_context 0x0073E35B)`
- **Unit `+0xF8` is reset at unload initialization.** This overwrites the `Random(0,29)` value stored by `UnitClass::Unlimbo`, so the RNG draw is consumed but does not seed unload cadence; the first due attempt waits for the accumulator to cross the threshold. `(corrected 2026-07-10: decompile_function 0x00737BA0 plus get_assembly_context 0x0073DFD0 — OPERATOR_OR_ORDER_DRIFT)`
- **The FSM does not restart GAREFNOR while occupied and does not leave it as a completion wind-down.** Later gates skip slot 10 while `+0x584` is non-null, and the completion/early-exit paths clear slot 10. `(corrected 2026-07-10: decompile_function 0x0073D630 and get_assembly_context 0x0073E517/0x0073E58F — OPERATOR_OR_ORDER_DRIFT)`
- **`CreateAnimForSlot` replacement semantics are narrower than previously stated.** When explicitly replacing an occupied slot, it copies old AnimClass `+0xAC`, nulls the building slot, destroys the old anim, and installs the new one. The former “propagates veterancy and shroud level” wording was not supported by the body. `(corrected 2026-07-10: decompile_function 0x00451890 — INFERENCE_HARDENED)`
- **Dock slot 7/10/8 calls happen on the unit-side FSM (`Mission_Deploy_Building`).** The separate building update owns the storage-tier slot 3–6 display. `(verified 2026-07-10: decompile_function 0x0073D630/0x004509D0)`
- **`MOV [ESI+0xBC],3` writes the unit's FSM state directly.** `0xBC` is already the byte offset; it must not be multiplied again. `(corrected 2026-07-10: get_assembly_context 0x0073E08E and search_instructions 0x0073D630 operand 0xBC — PARAM1_TYPE_MISREAD)`

---

## 9. Open Questions — Resolution Pass

### 9.1. Unit `+0xF8` increment site — RESOLVED (corrected 2026-07-10)

> **CORRECTION — 2026-05-19.** This section's original "never incremented" and "Unlimbo seeds Random(0,2)×30" claims were both wrong. The byte-pattern scan missed the actual incrementer (a `MOV [reg+0xF8], <reg holding sum>` rather than `INC`/`ADD` with imm8) and misread the Unlimbo seed. Authoritative reference for this field is now **[UNIT_0x3E_BALE_CADENCE_TIMER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNIT_0x3E_BALE_CADENCE_TIMER_GHIDRA_REPORT.md)** (2026-05-19 swarm slot-5).

**Corrected finding:** unit byte offset `+0xF8` (shown as `param_1[0x3E]` only because the decompiler types the receiver as `int *`) is a **plain `int` accumulator**, not a `CDTimerClass` embed. It is explicitly incremented:

- **Incrementer:** `TechnoClass::AI_Update @ 0x006F9E50` performs `field_0xF8 += field_0x110` every `field_0x108` frames whenever `field_0x10C != 0`. The cluster `+0xF8/+0x100/+0x104/+0x108/+0x10C/+0x110` is a "periodic accumulator" struct distinct from `CDTimerClass` (which uses StartFrame + duration semantics).
- **Active-flag (`+0x10C`):** set to `1` in `UnitClass::Unlimbo` only when the unit Type has `+0xE18` or `+0xE19` non-zero (i.e., is a Harvester or Weeder). Cleared (`= 0`) for all other unit types — that's why the accumulator only ticks for harvesters/weeders.
- **Gate (in `Mission_Deploy_Building` state 3):** `HarvesterDumpRate × 900.0 ≤ (double)*(int *)(unit+0xF8)` — fires when the accumulator crosses the threshold; resets to 0 on successful drain.

**Seed sites (corrected):**
- `UnitClass::Unlimbo @ 0x00737BA0` seeds with **`Random__RandomRanged(0, 0x1D)` = `Random(0, 29)` uniform** — NOT `Random(0,2)×30`. Verified via `decompile_function 0x00737BA0` (2026-05-19 reconciliation pass).
- `UnitClass::HarvestBrain_Idle @ 0x00737180` (two sub-sites): re-seeds with `Random(0, 2) × 0x1E` → `{0, 30, 60}`. The "slave-only" label previously attached to HarvestBrain_Idle is not supported by the decompile — the function does directional 8-neighbor cell scans + `Set_Destination` and looks like general AI-harvester wander logic. Open as a side question for a future audit.

**Unload consequence (corrected 2026-07-10):** `UnitClass::Unlimbo` still consumes `Random(0,29)` and stores it to `+0xF8`, but unload initialization at `0x73DFD0` unconditionally writes zero to `+0xF8` before state 3. The draw affects RNG sequence parity but does **not** seed unload cadence. Timed dump attempts then occur when the post-reset accumulator crosses `HarvesterDumpRate × 900`; successful attempts drain one whole nonempty StorageClass resource slot, and a later due attempt discovers empty cargo. `(decompile_function 0x00737BA0 and decompile_function/get_assembly_context 0x0073D630/0x0073DFD0 — OPERATOR_OR_ORDER_DRIFT)`

### 9.2. `BuildingClass+0x584` — RESOLVED

**Finding:** `BuildingClass+0x584` is the **active slot-10 (`SpecialAnim`) AnimClass pointer** in the building's 21-entry anim-pointer array. It can be populated by the unit FSM's SetAnimSlotImage/CreateAnimForSlot call and is also read/managed by `BuildingClass::UpdateAnimation`. VERIFIED.

In `BuildingClass::UpdateAnimation` (0x4509D0):

```
0x450CBD: TEST byte ptr [ECX + 0x16A8], AL   ; gated on Type+0x16A8 (`SiloDamage=`)
0x450D1D: MOV EAX, [ESI + 0x584]              ; read current slot-10 anim pointer
0x450D29: TEST EAX, EAX                       ; if null, create
0x450D71: PUSH 0xa; CALL CreateAnimForSlot(10, ...)
0x450D7B: MOV ECX, [ESI + 0x584]
0x450D81: MOV [ECX + 0xAC], EDI              ; writes storage tier (0..3) to anim's +0xAC
0x450D8D: ... PUSH 0xa; CALL ClearAnimSlot(10)  ; clears if tier == 0
```

So `building+0x584` is the slot-10 pointer itself. The `Mission_Deploy_Building` null check ensures the dock pulse does not replace an already-active SpecialAnim.

**Resolved — `Type+0x16A8` is `SiloDamage=`.** `BuildingTypeClass::ReadINI` reads string `SiloDamage` at `0x0081A780` and writes the returned bool to `+0x16A8` at `0x00461180`. Stock GAREFN/NAREFN do not set `SiloDamage`, so the building-side silo display does not populate slot 10 for them; the dock FSM remains its source. `(corrected 2026-07-10: search_instructions 0x0045FE50 operand 0x16A8, get_assembly_context 0x00461169/0x00461180, and read_memory 0x0081A760 — RTTI_LABEL_DRIFT/INFERENCE_HARDENED)`

### 9.3. `[ESI+0xBC] = 3` after slot 7 — RESOLVED (false alarm)

**Finding:** ESI is the **unit** throughout `Mission_Deploy_Building`. `[ESI+0xBC]` is unit byte offset `+0xBC` (shown as `param_1[0x2F]` by the `int *` decompile). Writing 3 is the FSM transition from state 1 to state 3 (dump-gate loop). VERIFIED via:

- `0x73D636: MOV ESI, ECX` — ESI = param_1 (unit)
- ESI never reassigned to a building anywhere along the slot-7 path
- All seven `[ESI+0xBC]` writes inside Mission_Deploy_Building (0x73D8A0, 0x73DCAB, 0x73DD98, 0x73DDDF, 0x73E093, 0x73E51C, 0x73E594) write values in {1, 2, 3, 4} — these are the FSM state numbers from the inner switch at 0x73D6F8
- Switch dispatch table at `0x73E5C0`

The previous trace agent's worry was unfounded. There is no separate building-side dock state field at `building+0xBC`.

### 9.4. Slot 10 trigger from BuildingClass::UpdateAnimation — RESOLVED + NEW FINDING

**Finding:** For stock GAREFN/NAREFN (`SiloDamage` absent), `BuildingClass::UpdateAnimation` does not independently populate slot 10; the unit FSM is the sole driver of their dock-pulse SpecialAnim. A modded refinery with `SiloDamage=yes` would also engage the building-side slot-10 storage display. `(corrected 2026-07-10: decompile_function 0x004509D0 plus get_assembly_context 0x00461180 — INFERENCE_HARDENED)`

But the trace surfaced a **NEW finding worth its own section** — see Section 10 below.

### 9.5. Slot-10 `Powered` flag default — DEFERRED

Not yet investigated. Reference-level question; doesn't block implementation. Default is 1 per parent doc.

---

## 10. VERIFIED — Storage-tier display on slots 3-6

`BuildingClass::UpdateAnimation` (`0x4509D0`) drives slots **3/4/5/6** for refineries, gated on `Type+0x16BB`:

| Address | Slots | Gate |
|---------|-------|------|
| `0x450E0D` | clear the slot selected by the previous cached tier | Type+0x16BB and tier changed |
| `0x450F99` | create the slot selected by the new tier | Type+0x16BB and tier changed |

The exact tier is:

```text
total = floor(StorageClass::GetTotalAmount())
tier = 0,                                      if total == 0
tier = (floor(StorageClass::GetTotalAmount()) * 4) / Type.Storage, otherwise
slot = 3 + min(tier, 3)
```

The value is cached at `BuildingClass+0x6F0`; a change clears the prior tier slot and creates the new tier slot. `(corrected 2026-07-10: decompile_function 0x004509D0 and get_assembly_context 0x00450E0D/0x00450F99 — INFERENCE_HARDENED)`

- **tier 0** → slot 3 (`ActiveAnim` = `GAREFNL1`)
- **tier 1** → slot 4 (`ActiveAnimTwo` = `GAREFNL2`)
- **tier 2** → slot 5 (`ActiveAnimThree` = `GAREFNL3`)
- **tier 3 or greater** → slot 6 (`ActiveAnimFour` = `GAREFNL4`)

Only **one** of the four is selected by this refinery-tier mechanism at a moment. Zero storage selects slot 3, not a hidden state.

**Rust implication:** current rendering intentionally shows only primary slot 3 for stock refineries because the stock Allied/Soviet unload path credits the house rather than filling building storage. General parity still requires the cached storage-derived tier mechanism for maps/mods that use nonzero building storage.

---

## Sources

- **Ghidra functions decompiled:**
  - `0x73D630` (Mission_Deploy_Building) — full read of inner-FSM states 0/1/3/4
  - `0x451750` (SetAnimSlotImage) — confirmed `param_2 = slot index`
  - `0x451890` (CreateAnimForSlot) — confirmed slot replacement semantics
  - `0x45FE50` (BuildingTypeClass::ReadINI) — verified slot↔INI key offsets via `LEA EDX, [EBP + offset]` instructions for every key
  - `0x459900` (vtable+0x468) — particle emitter
  - `0x65AE30` (current label `RadioClass__In_Radio_Contact` (formerly mislabeled PathType__Has_Valid_Steps)) — state-4 receiver `+0xE4/+0xE8` contact-vector nonempty test; current label is misleading
  - `0x460A6C` (Refinery flag write) — confirms `+0x16BB`
- **Parent doc:** [BUILDING_ANIM_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_ANIM_STATE_MACHINE.md) — 21-slot table, damage/power/cloak switching
- **Related doc:** [BUILDING_DOCK_AND_HEAL_STATE_MACHINES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_DOCK_AND_HEAL_STATE_MACHINES.md) — *NOTE: Part 2 of that doc is INCORRECT* (confused harvester dock with `SlaveManagerClass::AI_Update`). The slot 7/8/10 claims it makes are accurate; the FSM structure it claims is not.
- **INI files:**
  - `ini/rulesmd.ini` — `[GAREFN]`, `[NAREFN]`, `[YAREFN]` rules-side keys
  - `ini/artmd.ini` — `[GAREFN]`, `[GAREFNL1]`–`[GAREFNL4]`, `[GAREFNOR]`, Soviet equivalents
- **Memory addresses verified:**
  - `0x007E27F8` — IEEE-754 double `900.0` (frame-per-minute constant)
  - `0x008871E0` — global pointer read by the unload body; `+0x1700` is the health-ratio threshold and `+0x1528` is HarvesterDumpRate

The older `0x00A85C04/0x00A85A2C` bullets hardened one runtime singleton address into a static-binary claim. The instructions read `[0x008871E0]` first and then use offsets `+0x1700/+0x1528`; the static image does not prove a fixed pointee address. `(corrected 2026-07-10: get_assembly_context 0x0073E06C/0x0073E35B and read_memory 0x008871E0 — INFERENCE_HARDENED)`
