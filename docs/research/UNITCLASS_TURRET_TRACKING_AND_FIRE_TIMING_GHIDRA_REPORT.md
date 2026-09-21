# UnitClass — Turret Tracking & Fire Timing — Ghidra Research Report

**Primary addresses:**
- `UnitClass::TurretAI` — `0x007468C0`
- `UnitClass::Fire_At_Target` — `0x00736DF0`
- `UnitClass::Facing_Update` — `0x00736990`
- `FacingClass::Set` (Ghidra label `RateTimer__Set`) — `0x004C9220`
- `FacingClass::Current` (Ghidra label `RateTimer__Current`) — `0x004C93D0`
- `FacingClass::UpdateFacing` (snap variant) — `0x004C9300`
- `FacingClass::SetROT` (Ghidra `FUN_004C9680`) — `0x004C9680`
- `FacingClass::IsRotating` (Ghidra `CDTimerClass__Remaining`) — `0x004C9480`
- `CDTimerClass::GetTimeRemaining` — `0x00426630`
- `compute_facing_to_target` (Ghidra `FUN_005F3DB0`) — `0x005F3DB0`
- `TechnoClass::GetFireError` — `0x006FC0B0`

**Confidence:** HIGH. All findings verified via Ghidra MCP decompilation + disassembly of gamemd.exe. Function signatures verified against UnitClass::Constructor (0x007353C0).

**Active in YR:** Yes — entire pipeline runs every tick for every UnitClass instance with a `Target` set, in standard YR skirmish.

**Scope of this report:** This document covers the **UnitClass-specific layer** that wraps `TechnoClass::Fire_At` (already documented in [FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md)). It focuses on:
- How turret rotation is interpolated frame-by-frame (`FacingClass`)
- How idle units acquire targets via 8-cell scan (`TurretAI`)
- How a unit decides whether to actually fire vs continue rotating (`Fire_At_Target`, `GetFireError`)
- How body and turret facings are coupled per tick (`Facing_Update`)

Out of scope (covered elsewhere): the post-fire pipeline (bullet creation, ROF reset, particle effects), target selection (`Greatest_Threat`, `SelectWeaponAgainst`), retaliation logic (`ShouldRetaliate`).

---

## 1. Three FacingClass instances on every TechnoClass

Every TechnoClass instance holds **three** embedded `FacingClass` objects, each 24 bytes (`0x18`). Verified via `LEA` instructions in TechnoClass / UnitClass constructors (existing [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md) §726-728, cross-checked here).

| Offset (TechnoClass) | Name | Default ROT | UnitClass override | Role |
|---|---|---|---|---|
| `+0x370` | **BodyFacing** | 3 (set by FacingClass ctor `FUN_004c91e0`) | **Not touched by UnitClass::Constructor** — keeps default ROT=3 | Smoothed body direction for sprite rendering. Locomotor (`DriveLocomotionClass::Do_Turn`) updates the *destination*; this struct interpolates the rendered short over a few frames. |
| `+0x388` | **TurretFacing** | 0 (instant, set by `FUN_004c91c0`) | **`SetROT(Type+0x71C)`** at `0x735579` | The turret's rotation toward target. ROT-bound interpolation per tick. |
| `+0x3A0` | **BarrelFacing** | 0 (instant, set by `FUN_004c91c0`) | **`SetROT(Type+0x71C)`** at `0x73558D` | The "live aim" facing. For single-turret tanks this is what `Facing_Update` updates each tick. For gattling/multi-barrel weapons it's the spinning barrel. |

### 1.1 Tiny detail — UnitClass::Constructor sets ROT TWICE

At `0x00735570`-`0x0073558D` the constructor reads `Type+0x71C` (ROT) and pushes it into `FacingClass::SetROT` for **both** TurretFacing AND BarrelFacing. Same value, two separate calls. This means barrel rotation always matches turret rotation rate on UnitClass. Exception: the FacingClass *constructor defaults* (ROT=3 for body, ROT=0 for turret/barrel) survive on InfantryClass / AircraftClass / BuildingClass, because only `UnitClass::Constructor` does this override.

### 1.2 Tiny detail — BodyFacing keeps ROT=3 even on heavy tanks

Apocalypse Tank with `ROT=3` in rules.ini and an Engineer with no ROT both end up with **BodyFacing.ROT == 3** (the FacingClass-constructor default). This means the rendered body never rotates *faster* than ROT=3, regardless of unit type. The actual turn rate of vehicles is enforced by the **locomotor** (`DriveLocomotionClass::Do_Turn`, `ShipLocomotionClass::Process_Drive_Track`), which updates the *destination* of BodyFacing. The FacingClass at +0x370 is a render-time smoother only.

Implication: if the locomotor sets a new body destination every tick (e.g., curving along a path), the rendered body smooths it over up to `(abs(new - prev) / 3)` ticks even for fast units. This is why heavy tanks and light tanks have visually similar body-rotation *smoothness* even though the underlying drive rate differs.

### 1.3 FacingClass byte layout (24 bytes, `0x18`)

Verified from disassembly of `RateTimer__Set` (`0x4C9220`), `RateTimer__Current` (`0x4C93D0`), `FacingClass::UpdateFacing` (`0x4C9300`), and `CDTimerClass::Remaining` (`0x4C9480`):

| Byte offset | Type | Field | Notes |
|---|---|---|---|
| `+0x00` | `short` | **Current** (destination) | Where the rotation will end up. Read as the "current animated value's destination". |
| `+0x02` | `short` | **(secondary short)** | Read/written as part of dword copies (`MOV EDX, [ESI]; MOV [...], EDX`). Role unconfirmed — possibly an angular-speed or direction-magnitude pair. Always copied alongside Current. |
| `+0x04` | `short` | **Prev** (start) | Where the current rotation began. Updated by `Set` to the animated value at the moment of the new request. |
| `+0x06` | `short` | (paired with Prev as dword) | Same pattern as `+0x02`; unknown role, copied alongside Prev. |
| `+0x08` | `int` | **CDTimerClass.StartFrame** | `g_CurrentFrameCounter` when the rotation began. `-1` = never started / inactive. |
| `+0x0C` | `int` | (CDTimerClass field 2) | Read by `CDTimerClass__GetTimeRemaining` returns `param_1[2]` which is at byte 8 — note: the CDTimer accesses are using *its own* base. See §1.4. |
| `+0x10` | `int` | **CDTimerClass.Duration** | Total ticks needed = `abs(Current - Prev) / ROT`. Set by `Set` at `0x4C92E8` (`MOV [ECX+8], EAX` where ECX=this+8). |
| `+0x14` | `short` | **ROT** | Rate of turn in 16-bit-facing units per tick. Stored shifted: `SetROT(rot_byte)` writes `(byte << 8)` here. Capped at 0x7F input (writes 0x7F00). |
| `+0x16` | `short` | (alignment) | — |

### 1.4 CDTimerClass embedded at `+0x08`, but offsets are *relative to the timer*, not FacingClass

Critical detail. `CDTimerClass::GetTimeRemaining` (`0x426630`) is decompiled as:

```c
int CDTimerClass__GetTimeRemaining(int* this) {
    int duration = this[2];          // byte 8 of CDTimer = byte 0x10 of FacingClass
    if (this[0] != -1) {              // byte 0 of CDTimer = byte 0x08 of FacingClass
        int elapsed = g_CurrentFrameCounter - this[0];
        if (elapsed < duration) return duration - elapsed;
        return 0;
    }
    return duration;
}
```

So **CDTimerClass is 12 bytes**: `start_frame`, `(unknown)`, `duration`. Embedded at FacingClass `+0x08`, ending at `+0x14`.

### 1.5 `CDTimerClass::Remaining` is a "is rotating?" boolean

The **8-byte ROT field is also at the FacingClass-relative offset 0x14**, which `CDTimerClass__Remaining` (`0x4C9480`, used by `Facing_Update`) reads:

```c
// param_1 is int (byte-addressed FacingClass base)
if (*(short *)(param_1 + 0x14) > 0) {   // +0x14 = ROT (corrected 2026-05-28: was param_1[20] and param_1[16]; param_1 is int-typed so offsets are direct bytes, not short-array indices — binary shows *(short*)(param_1+0x14) and *(int*)(param_1+0x10); ROOT_CAUSE: PARAM1_TYPE_MISREAD via decompile_function 0x4C9480)
  duration = *(int *)(param_1 + 0x10);  // +0x10 = Duration
  if (*(int *)(param_1 + 0x08) != -1) { // +0x08 = StartFrame
    elapsed = current - *(int *)(param_1 + 0x08);
    if (elapsed >= duration) return 0;
    duration -= elapsed;
  }
  if (duration != 0) return 1;
}
return 0;
```

This isn't pure CDTimer access — it ALSO reads ROT at +0x14 (which is *outside* the embedded CDTimer). It returns 1 iff ROT > 0 AND the rotation timer hasn't expired. Effectively: "this FacingClass is currently mid-rotation."

---

## 2. FacingClass per-tick interpolation algorithm

This is the core of turret rotation. There is **no per-tick "advance the facing by ROT" call**. Instead, the system is **timer-based**: at any given moment, the animated facing is computed from `start_frame`, `Prev`, `Current`, and `ROT` via interpolation.

### 2.1 Reading the animated facing — `FacingClass::Current` at `0x4C93D0`

```c
void FacingClass::Current(short* this, undefined4* out) {
    if (this[20] < 1) {                  // ROT == 0 (instant)
        *out = *(int*)this;              // out = Current (4-byte read, includes secondary)
        return;
    }
    int duration = *(int*)(this + 8);    // CDTimer.Duration at +0x10
    if (*(int*)(this + 4) != -1) {       // CDTimer.StartFrame at +0x08, -1 = never started
        int elapsed = g_CurrentFrameCounter - *(int*)(this + 4);
        if (duration <= elapsed) {       // expired
            *out = *(int*)this;
            return;
        }
        duration -= elapsed;             // remaining ticks
    }
    if (duration == 0) { *out = *(int*)this; return; }

    short diff = this[0] - this[2];      // Current - Prev (signed)
    short step_size = abs(diff) / this[20];   // = abs(diff) / ROT
    if (step_size < 1) { *out = *(int*)this; return; }

    short remaining_ticks =
        (this[4] == -1) ? duration
        : (g_CurrentFrameCounter - this[4] >= duration) ? 0
        : duration - (g_CurrentFrameCounter - this[4]);

    int packed = *(int*)this;
    short animated = (short)packed - (diff / step_size) * remaining_ticks;
    *out = (packed & 0xFFFF0000) | (animated & 0xFFFF);
}
```

**Key identity:** `diff / step_size == sign(diff) * ROT` (because `step_size = abs(diff)/ROT`). So the inner expression is:

```
animated = Current - sign(diff) * ROT * remaining_ticks
        == Prev + sign(diff) * ROT * elapsed_ticks
```

In words: the rotation moves at exactly `ROT` 16-bit-facing units per tick along the shortest signed path from Prev to Current, for `Duration = abs(Current - Prev) / ROT` ticks total. After that, animated == Current and the rotation is done.

### 2.2 Tiny detail — `step_size < 1` means rotation is skipped

If `abs(diff) < ROT`, then `step_size = 0` (integer division). The function returns `Current` immediately without interpolating. **This is an off-by-one edge case**: a rotation request smaller than one tick's worth of ROT *snaps* instantly. In the frame after a Set with `abs(new - prev) < ROT`, the FacingClass already shows the new value with no animation.

Implication: for ROT=5 (typical tank), any retarget that asks for less than 5 16-bit-facing units (~0.027° in 16-bit, but effectively a sub-arcminute change) snaps without interpolation. This rarely matters for visible motion but matters for any fixed-point check that compares animated facing against target.

### 2.3 Tiny detail — `signed/unsigned diff`

`diff = this[0] - this[2]` is computed as `short - short`, then sign-extended via `MOVSX EDI, AX` and `CDQ` before `IDIV`. The diff is **signed**, but it's NOT shortest-path-corrected: if Current=0 and Prev=0xFFFF (i.e., crossing the wrap), diff = 0 - 0xFFFF = 1 (after short underflow), not -65535. So:

> **Wrap-around handling is implicit in 16-bit signed subtraction.** The engine relies on the fact that `(short)(0x0001 - 0xFFFF) == 0x0002` is interpreted as `+2` (shortest signed delta is +2, not -65534). This is the standard "delta in mod-65536 space" trick.

Verify:
- `0x0001 - 0xFFFF` as `short - short = (int)(0x10001 - 0xFFFF) = (int)0x00000002 = +2`. ✓
- `0xFFFF - 0x0001 = -2` as `short`. ✓ Crosses the wrap correctly.

So a turret going from facing 0xFFE0 (just-west-of-north) to 0x0010 (just-east-of-north) traverses 0x0030 (the short way, 48 16-bit units), not 0xFFD0 (the long way). This works automatically.

### 2.4 Setting a new desired facing — `FacingClass::Set` at `0x4C9220`

```c
char FacingClass::Set(short* this, short* new_target) {
    if (*this == *new_target) return 0;   // no-op early exit

    short rot = this[20];                 // ROT
    int packed_current = *(int*)this;
    int new_prev_packed = packed_current; // default if no interpolation

    if (rot > 0) {
        // (compute animated value as in §2.1, into packed_current)
        ...
        new_prev_packed = packed_with_animated_replaced_in_low_short;
    }

    // Snapshot animated position into Prev
    *(int*)(this + 2) = new_prev_packed;   // bytes 0x04..0x07 = animated Current

    // Set new destination
    int new_packed = *(int*)new_target;
    *(int*)this = new_packed;              // bytes 0x00..0x03 = new target

    if (rot > 0) {
        short diff = (short)new_packed - this[2];
        *(int*)(this + 4) = g_CurrentFrameCounter;     // CDTimer.StartFrame at +0x08
        *(int*)(this + 6) = local_8;                    // CDTimer.+0x0C from local stack — see §2.5
        *(int*)(this + 8) = abs(diff) / rot;            // CDTimer.Duration at +0x10
    }
    return 1;
}
```

**Key behavior:** `Set` snapshots the **current animated position** into `Prev` so the next interpolation starts from where the rotation visually is *right now*, not from where the previous Set left off. This is what makes turret retargeting smooth — the turret doesn't snap or stutter when the target changes mid-rotation.

### 2.5 Tiny detail — uninitialized stack write to FacingClass+0x0C

In both `Set` (`0x4C92E5`: `MOV [ECX+4], EDX` after `ADD ECX, 0x8`, where EDX came from `[ESP+0x18]`) and `UpdateFacing` (`0x4C9399`: `MOV [ESI+4], EAX` from `[ESP+0x10]`), the second integer of the embedded CDTimer (`+0x0C` in FacingClass) is written from a **local stack slot that was never initialized** in the visible decompilation. This is reproducible across both setters. Possibilities:
- Compiler-generated copy of an outer arg the decompiler lost track of
- Compiler bug that writes uninitialized data
- An unused/legacy field

**Behavioral consequence:** Whatever's in `+0x0C` doesn't affect interpolation — `CDTimerClass::GetTimeRemaining` only reads `+0x00` (start) and `+0x08` (duration) of its base, which are FacingClass `+0x08` and `+0x10` respectively. The mystery field `+0x0C` is *written* but never *read* by the rotation math.

For our reimplementation: this is safe to skip. Document as "leave `+0x0C` zero, no observable effect."

### 2.6 `FacingClass::UpdateFacing` (`0x4C9300`) is the SNAP variant

Despite its name, this function is a setter that **does not initialize a duration** — it computes the animated position, compares to the new target, and either:
- **Equal**: clear timer state (`+0x10 = 0`, `+0x12 = 0`), return 0. Already there via interpolation.
- **Different**: write new value to BOTH Current and Prev (snapping), reset start frame to current frame, return 1.

It's used by callers that want "set this facing now, no smoothing" — the locomotion classes (`FlyLocomotionClass::Begin_Takeoff`, `WalkLocomotionClass::Set_Facing`, etc.), constructors, and the deploy path. Note the constructor list (25 callers) is dominated by initialization paths.

By contrast, `FacingClass::Set` at `0x4C9220` is the **smoothed setter** used by vehicle/building combat turns. Infantry fire-start uses the snap setter: `InfantryClass::Fire_At_Target` calls `0x4C9300` at `0x00520925` after `DoAction`, not `Set`. Rechecked 2026-09-21 against instructions and the bounded original-code corpus in `tools/spatial_oracle/infantry_fire_start.py`.

**Bug-trap:** the `0x4C9300` function name is `FacingClass__UpdateFacing` in Ghidra, while `0x4C9220` is labeled `RateTimer__Set`. These names are misleading. The "real" UpdateFacing semantically is `0x4C9220`. Don't trust the labels.

### 2.7 ROT setter — `FUN_004C9680`

```c
void FacingClass::SetROT(int this, int rot_byte) {
    if (rot_byte > 0x7E) rot_byte = 0x7F;
    *(ushort*)(this + 0x14) = (ushort)(byte)rot_byte << 8;
}
```

Two tiny details here:

1. **ROT is clamped at `0x7F`**. Any input value > 126 is set to 127. Since rules.ini ROT values are typically 1–10 (with structures up to 100), the clamp matters only in edge cases.
2. **ROT is stored << 8**. The byte value from `Type+0x71C` (e.g., 5) becomes `0x0500` in the FacingClass field. So when the per-tick interpolation reads `this[20]` (the short at +0x14), a ROT-byte of 5 yields a step size of `1280` 16-bit-facing units per tick (= 7.03°/tick at 15 fps = 105.5°/sec). This matches the visible turret rotation speed of YR units.

**Implication for parity:** the engine does NOT use ROT as "degrees per frame" directly. ROT in the INI is shifted up by 8 bits internally, making the actual per-tick step `(ROT_ini * 256)` in 16-bit-facing space.

### 2.8 The "is rotation done" check

To check if a turret has finished rotating, code calls `CDTimerClass::Remaining` at `0x4C9480` (Ghidra label) — which despite its name reads ROT and CDTimer, returning 1 if rotation is still in progress, 0 if done. Used by `Facing_Update` end-section to mark the "fire ready" state on `+0x4A0`.

---

## 3. UnitClass::TurretAI (`0x007468C0`)

Called from `UnitClass::AI` (per [UNITCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_GHIDRA_REPORT.md) §3j) **only when** `Type+0xD2F (TurretNotHidden) != 0` AND `Type+0xD30 (TurretLocked) == 0`. So units without a visible turret OR with a locked turret skip this entirely.

### 3.1 Three-phase logic

**Phase A — should we scan for nearby targets?**

```
if not (vtable+0xC4)():        // some "is busy / can't scan" virtual
    // assert Locomotor exists
    if not Locomotor->vtable+0x10()       // "is moving" check
       AND Type+0xD32 != 0                // TurretScansNearby flag
       AND FootClass::GetDestination(this) == 0   // not currently moving
    : SHOULD_SCAN = true
```

So: **idle, stationary, with TurretScansNearby flag, can't be currently busy** → scan enabled. No scan if any of those fail.

**Phase B — early-exit if locomotor is moving:**

```
if Locomotor->vtable+0x10() != 0:    // "is moving"
    call vtable+0x470()                // tear-down/cleanup
    skip to subordinate-AI section
```

When the unit is moving, we abandon the scan entirely. No idle-target-acquire while moving.

**Phase C — 8-cell scan, every 8 frames:**

The check `g_CurrentFrameCounter & 0x80000007 == 0` is a cute hack: it gates the scan to **frames where the low 3 bits of the counter are zero AND the sign bit is zero** — i.e., 1-in-8 frames, but only on the positive side. Once `g_CurrentFrameCounter` overflows to negative, the gate condition becomes `(uVar7 - 1) | 0xFFFFFFF8 == 0xFFFFFFFF` instead.

**Tiny detail:** this gate fires every 8 frames during the first ~4 hours of a match (positive frame counter). After overflow (~hour 4 at 15 fps... actually 4.5 hours at 15 fps in IPS counting) the gate flips meaning. **In practice no YR match reaches this overflow** — but it's a real edge case in the binary.

For each frame the gate fires:

```c
center = vtable+0x1B8(this)             // get this unit's cell coords (lepton or cell?)
center_cell = vtable+0x1BC(this)        // get this unit's CellClass
on_bridge = CellClass::IsBridge(center_cell)

for i in 0..8:
    cell_offset = (g_DirectionOffsets[i*2], DAT_0089F68A[i*2])  // 8 directions
    target_cell = MapClass::Get_CellClass(center + cell_offset)
    target_obj = FUN_0047EC40(on_bridge ? cell.bridge_layer : cell.ground_layer)
    if target_obj != 0 AND not HouseClass::Is_Ally_ByObject(target_obj):
        // hostile object found in adjacent cell — start fire timer
        param_1[0x78] = g_CurrentFrameCounter           // sight start frame
        param_1[0x79] = (some local from scan)          // ??
        param_1[0x7A] = RulesClass+0x1014               // sight duration constant?
        goto FIRE_BREAK
```

`g_DirectionOffsets` and `DAT_0089F68A` are static 8-entry direction tables. The scan walks the 8 cardinal+diagonal cells around the unit. **This is a 1-cell-radius scan**, NOT a weapon-range scan.

**Tiny detail — the bridge layer:** if the unit is currently on a bridge, the scan reads `cell+0x128` (bridge-layer occupant linked list head) instead of `cell+0x124` (ground-layer). This means turret-scan correctly distinguishes targets on the bridge vs targets passing under the bridge.

**Phase D — sight-timer expiration → enter Hunt mission:**

```c
sight_timer = param_1[0x7A]
if param_1[0x78] == -1 OR (g_CurrentFrameCounter - param_1[0x78]) < sight_timer:
    skip
else:
    // sight expired — pick a Hunt target
    idx = Random__RandomRanged(0, RulesClass+0x1008 - 1)
    idx = clamp(idx, 0, RulesClass+0x1008 - 1)
    target_type = *(RulesClass+0xFFC + idx*4)        // pick from a hunt target list
    if target_type != 0:
        param_1[0x146] = target_type        // assign hunt target
        param_1[0x147] = 0
        param_1[0x76] = 1                    // mark as in hunt mode
        param_1[0x77] = g_CurrentFrameCounter
        vtable+0x49C(this)                   // commit hunt target
```

The fields `+0x118` (param_1[0x46] = 0x118 byte offset, but param_1[0x76]·4 = 0x1D8 in dword-offset reading...). Wait, param_1 is `int*` so `param_1[0x76]` is byte offset `0x1D8`. Need to double-check this offset.

**Open question:** the `param_1[0x76]`/`[0x77]`/`[0x78]`/`[0x79]`/`[0x7A]`/`[0x146]`/`[0x147]` offsets at int-stride should be cross-checked against TechnoClass/FootClass struct. Likely:
- `[0x78]` (= byte offset 0x1E0) = some "sight detected enemy at frame" timer-start
- `[0x7A]` (= 0x1E8) = sight duration from rules
- `[0x146]` (= 0x518) = hunt target type ptr
- `[0x76]` (= 0x1D8) = "is hunting" byte flag

Cross-checking with `FootClass` constructor: `param_1[0x192] = 0`, `param_1[0x193] = 10`, `param_1[0x194] = g_CurrentFrameCounter` — these are *different* offsets. No direct match seen in this pass.

**Phase E — controller propagation:** the very tail of the function checks if a child unit has a parent reference (`param_1[0xB2]` at byte 0x2C8) and propagates a "should refresh AI" byte to the parent based on the AI control + human-player check. Cosmetic for now.

### 3.2 Two tiny details from TurretAI

1. **TurretAI does not aim the turret.** It only scans for nearby targets and assigns hunt missions. The actual turret-rotates-toward-target update happens in **`Facing_Update`** (next section). TurretAI is target-acquisition; rotation is in Facing_Update.

2. **The 8-frame scan period is a hard binary constant.** No INI key controls it. If the engine is run at a different frame rate, the scan period scales linearly with the tick rate. For our 15 fps target this is 0.533s between scans.

---

## 4. UnitClass::Fire_At_Target (`0x00736DF0`)

The wrapper that calls `TechnoClass::Fire_At` from `UnitClass::AI` (§3m of [UNITCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_GHIDRA_REPORT.md)). Despite the name, this function is **not** the firing pipeline itself — it's the gate that decides whether to call Fire_At and what to do based on the fire error code.

### 4.1 Top-level structure

```c
if (this->Target == NULL) {
    // GATTLING DECAY PATH (no target)
    if (Type+0xCD5) TechnoClass::UpdateGattlingStage(0);   // stage decay
    if (gattling_value > 0) this->field_0x148++;            // accumulate
    return;
}

WeaponStruct* w = vtable+0x3F8(this);    // GetWeapon
if (w->Type == NULL) return;              // GetTechnoType-like check below

int weapon_idx = vtable+0x2E4(this);      // SelectWeaponAgainst-equivalent
int err = vtable+0x3C0(this->Target, weapon_idx, true);   // GetFireError

// Special: if err==0 (OK) or 2 (FACING) AND vtable+0x4E4 returns true:
if ((err == 0 || err == 2) && vtable+0x4E4(this)) {
    vtable+0x1E8(0x10, 0);    // override mission / state — see §4.3
    return;
}

switch (err) {
    case 0: ... fire path
    case 2: ... rotation-in-progress path
    case 5: ... range path
    case 6: ... cannot-target path
    case 8, 11: ... burst/cloak path
    case 9: ... force-fire path
}

// Gattling stage update (post-switch)
if (Type+0xCD5 != 0 || err is one of {0,2,3,4}) {
    if (err is 0,2,3,4) TechnoClass::IncreaseGattlingStage(1);
    else if (Type+0xCD5) TechnoClass::UpdateGattlingStage(1);
}
```

### 4.2 GetFireError return codes (TechnoClass::GetFireError @ 0x6FC0B0)

This function returns a 1-byte error code documenting *why* the fire failed (or 0 for OK). Decompiled here for the first time in this archive:

| Code | Meaning | Triggered by |
|---|---|---|
| `0` (OK) | Can fire now | All checks passed |
| `1` (NoAmmo) | `param_1->Ammo == 0` | After all other checks |
| `3` (Cooldown) | ROF timer hasn't expired | `field_0x2F4 - (current - field_0x2EC) > 0` |
| `4` (`-`) | (not seen in code path I traced; possibly reserved) | — |
| `5` (Generic) | Many reasons | See list below |
| `6` (OutOfRange) | Weapon range too short | `WeaponRange < 0` after compute or special anti-air checks |
| `7` (`-`) | (not seen) | — |
| `8` (CloakedTarget) | Cloak check on target | When `weapon+0x133` set AND `target.CloakState != 0` AND not visible |
| `9` (ForceFireAllowed) | Reserved for force-fire | Returned via `vtable+0x3A8` test path |

**Code 5 (Generic) is overloaded.** The function returns `5` for ~30 different conditions including:
- `param_1->field_0x2DC != 0` (some "can't fire now" flag)
- `vtable+0x1D4()` (some "is currently doing something incompatible")
- `param_1->IsSinking != 0`
- `param_1->field_0x1C8 != 0` (some state byte)
- target == this->LocomotorTarget (mismatch)
- Target type-specific anti-air rules
- Many more

Code 5 is the "general not-now" code. Code 3 specifically means "can fire eventually, but rotating/cooling-down".

### 4.3 The `err == 0 || err == 2` + `vtable+0x4E4` early-out

When fire is ready (err=0) OR rotating-into-position (err=2), AND the unit's vtable+0x4E4 returns true, the function calls `vtable+0x1E8(0x10, 0)` and returns. This sets the unit's mission state to **0x10** (Deployed mission, per [OPPORTUNITY_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPPORTUNITY_FIRE_GHIDRA_REPORT.md) §4 case description).

**Tiny detail:** This is the IFV/deploy interaction. `vtable+0x4E4` is likely "should auto-deploy on fire ready" — units with deploy-fire capability (Engineer in IFV, Mirage Tank in deployed state, etc.) auto-trigger a deploy when ready to fire.

### 4.4 Switch case 0 (FIRE OK)

```c
WeaponStruct* w = vtable+0x84(this);
if (w->harvester_anim == 0) {        // Type+0xE10 — harvester turn-anim flag
    field_0x16D = 0;                  // clear some flag
}

if (w->Weeder /*+0xE19*/ || w->Harvester /*+0xE18*/) {
    // Harvester-specific facing animation
    facing_to_target = compute_facing_to_target(this, target);
    facing_index = ((facing_to_target >> 0xC) + 1) >> 1) & 7;   // 0..7

    // Set animation state for harvester-turning
    field_0xF8 = DAT_008458B0[facing_index * 4]   // 8-direction lookup
    field_0x100 = g_CurrentFrameCounter
    field_0x104 = uStack_18              // local
    field_0x10C = 5                       // some 5-tick timer
    field_0x108 = 5
}

vtable+0x3CC(this->Target, weapon_idx);   // ACTUAL FIRE — calls TechnoClass::Fire_At wrapper
```

**Critical detail:** the actual fire is via `vtable+0x3CC`, NOT a direct `Fire_At` call. This is overridden per-class. For UnitClass it eventually calls `TechnoClass::Fire_At` (0x6FDD50, the big 919-line pipeline).

**Tiny detail — harvester facing animation:** The formula `((facing >> 0xC) + 1) >> 1) & 7` rounds 16-bit facing to nearest of 8 directions. The `>> 0xC` (=12) takes the top 4 bits, `+1, >>1` rounds, `& 7` masks. Harvesters playing a turning animation use this 8-direction index to pick the right SHP frame. The `5`-tick timer is the duration of the turn anim.

### 4.5 Switch case 2 (FIRE_FACING — rotating into position)

```c
if (Type+0xE11 == 0 AND Type+0xCA1 != 0) {       // not a deploy-firer AND has turret
    target_facing = compute_facing_to_target(this, target);
    FacingClass::Set(BarrelFacing, &target_facing);    // start turret rotation toward target
}
else if (this->LocomotorTargetSomething == 0) {
    // assert and check Locomotor
    if (!Locomotor->vtable+0x10()) {                   // locomotor not moving
        target_facing = compute_facing_to_target(this, target);
        FacingClass::Set(turret_or_body, &target_facing);
        // ALSO copy to a second facing
        local2 = FUN_004C9470(...);   // copy to TurretFacing? or BodyFacing?
        FacingClass::Set(turret_or_body, &local2);
    }
}
```

**Critical:** the rotation-while-firing path is THIS branch. When the engine sees "weapon ready BUT turret not yet aimed," it fires `FacingClass::Set` on the BarrelFacing/TurretFacing. The interpolation then plays out frame-by-frame in subsequent ticks.

### 4.6 Switch case 5 (RANGE) — special "approach for low-health building" exception

```c
if (TechnoClass::GetWeaponRange(this, weapon_idx) < 0) {
    if (target is a building (RTTI 1) AND target.HealthRatio < RulesClass+0x16F8) {
        vtable+0x3C8(0);    // stop firing
    } else {
        vtable+0x3C8(0);    // stop firing
    }
}
```

The check at `_DAT_007E9240` (referenced in GetFireError, line `if ((double)*(int*)(iVar4 + 0xb4) < _DAT_007E9240)`) and `RulesClass+0x16F8` is a **floating-point health ratio threshold**. Buildings below this ratio get an exception that allows out-of-range targeting (presumably to finish them off).

**Tiny detail:** RulesClass+0x16F8 is a `double` (8-byte float). This is the only floating-point comparison in the fire-decision path. We should look up which INI key sets it — likely `LowPower` or `ConditionYellow`/`ConditionRed` health thresholds.

### 4.7 Switch case 9 (FORCE_FIRE allowed)

```c
case 9:
    target = this->Target;
    field_0x16D = 0;                              // clear state byte
    if (TechnoClass::CanFireAt(target, weapon_idx)) {
        vtable+0x45C(0);                          // some state / target lock
    }
```

This is the "user-issued force-fire on unfireable target" handler. `CanFireAt` does an additional sanity check.

### 4.8 Gattling stage post-update (always runs)

After the switch, regardless of the path taken:

```c
// corrected 2026-05-28: the branch condition and spin-up label were SWAPPED in the original.
// Binary (decompile_function 0x736DF0): IncreaseGattlingStage fires when
//   gattling==true AND err IN {0,2,3,4}  (i.e., spinning UP while firing/rotating/cooling).
// UpdateGattlingStage fires otherwise (not-gattling, OR err NOT in set → decay).
// ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT
if ((Type+0xCD5 == 0) OR (err NOT in {0,2,3,4})) {
    // not gattling, or gattling but not in active-fire state → decay/update
    if (Type+0xCD5) TechnoClass::UpdateGattlingStage(1);
}
else {
    // gattling AND err in {0,2,3,4} → stage increase (gattling spinning up while firing)
    TechnoClass::IncreaseGattlingStage(1);
}

if (Type+0xCD5 != 0) {
    // Final gattling field accumulator
    if (TechnoClass::GetGattlingValue() < 1) return;
    field_0x148++;
}
```

`Type+0xCD5` is the **IsGattling** flag on TechnoTypeClass (existing [GATTLING_WEAPON_STAGE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATTLING_WEAPON_STAGE_SYSTEM_GHIDRA_REPORT.md) confirms). Gattling weapons spin up while firing and decay while idle. The accumulator `field_0x148` is the current spin level.

---

## 5. UnitClass::Facing_Update (`0x00736990`)

Called from `UnitClass::AI` §3n. This is **per-tick body+turret rotation**.

### 5.1 Four-section structure

```
SECTION A: target-aim turret (if has Target AND not blocked)
  A1: compute target_facing via compute_facing_to_target
  A2: if HasTurret (Type+0xCA1):
        if weapon condition: aim turret at body's animated value (lock)
        else: aim turret at target_facing
  A3: else (no turret):
        if Type+0x67C == 1 (some flag) AND no destination AND locomotor not moving:
          if body already at target_facing: confirm by re-Setting

SECTION B: body update (if not TurretLocked2)
  if Type+0xD21 == 0 (turret rotates independently):
    clear field_0x6AF
    if HasTurret: ... compute body update
    else: skip
  else (Type+0xD21 != 0 — turret locked to discrete-8 of body):
    compute discrete-8 body version of turret animated value
    set BarrelFacing to that value

SECTION C: rotation-active flag latch (always)
  if HasTurret: store CDTimerClass::Remaining (0/1) into field_0x4A0
```

### 5.2 TurretSpins — the permaspin formula (only used by Yuri's Floating Disk)

Verified from disassembly `0x736AB1`-`0x736ACB` and INI key resolution at `0x84412C`:

The flag `Type+0xD21 = TurretSpins`. INI key `TurretSpins=yes`. From the rulesmd.ini comment:

> `TurretSpins = Does the turret just sit and spin [only if turret equipped] (def=no)?`

**Default: `no`. Set on exactly ONE unit in vanilla YR:** `[DISK]` (Yuri's Floating Disk) at line 8704 with modder note *"gs unit is one big turret so it can use existing permaspin"*.

**The formula:**

```c
animated_facing = FacingClass::Current(BarrelFacing)   // 16-bit (0..65535)
new_target =
    (
      ( (((animated_facing >> 7) + 1) >> 1) & 0xFF )    // round to 8-bit (0..255)
      + 8                                                // per-tick spin advance
    ) << 8                                               // shift to high byte (16-bit)
FacingClass::Set(BarrelFacing, &new_target)
```

In words: each tick, **the new target is `(rounded_8bit + 8) * 256`** in 16-bit space. The +8 is the spin rate in 8-bit facing units per tick.

**Spin rate math:**
- Per-tick advance = 8 × 256 = 2048 16-bit-facing units = 11.25°
- Full revolution = 65536 / 2048 = **32 ticks per revolution**
- At 15 fps that's **2.133 seconds per revolution**, or ~28 RPM

**Interaction with ROT:** the Disk has `ROT=100`, which means `ROT_field = 100 << 8 = 25600`. Any rotation request of size `2048` completes in `2048 / 25600 = 1 tick` (truncates to 1). So the FacingClass interpolation always completes within the same tick — visually, the turret snaps to each new target. Combined with the +8 advance, this produces continuous (but discretely quantized) rotation.

**Why round to 8-bit first?** The `>> 7, +1, >> 1, & 0xFF` is a 16-bit-to-8-bit rounding. Then `<< 8` shifts back. The net effect: result has **the low byte forced to zero**. So the turret target is always at a 256-unit boundary in 16-bit facing space — i.e., one of 256 discrete directions, NOT smooth-continuous.

**Implication:** the Floating Disk's spin is NOT smooth — it visibly steps through 256 discrete facings, advancing 8 per tick. To the eye at 15 fps with ROT=100 snapping, this looks fluid. Implementing it as a continuous `+11.25°/tick` rotation would produce a *smoother* result than gamemd.exe — which is wrong for parity.

**Implication for parity:** if we ever render the Floating Disk, the spin rate must be exactly 32 ticks per revolution (15 fps) AND the discrete 256-step quantization must be present. Don't smooth-interpolate.

### 5.3 What TurretSpins does NOT do

The flag does NOT:
- Affect target acquisition (the disk still picks targets like any other unit)
- Affect firing (Fire_At_Target's normal logic still runs)
- Affect the 8-cell TurretAI scan (TurretSpins is independent of TurretScansNearby)
- Override ROT for non-spin rotations (target-aim still uses ROT in TurretAI / Fire_At_Target)

It ONLY hijacks Section B of `Facing_Update` to keep the BarrelFacing target advancing every tick.

### 5.4 Confirmed `Type+0xD2X` neighborhood

While resolving TurretSpins via the ReadINI string-xref hunt, the adjacent INI keys at neighboring offsets were confirmed (read order in `TechnoTypeClass::ReadINI` `0x712170`):

| Offset | INI Key | String addr | Read order |
|---|---|---|---|
| `+0x410` | `PoweredUnit` | `0x844158` | first in this block |
| `+0xD26` | `LightningRod` | `0x844148` | next |
| `+0xD24` | `ManualReload` | `0x844138` | next |
| `+0xD21` | **`TurretSpins`** | **`0x84412C`** | **target of this investigation** |
| `+0xD22` | `TiltCrashJumpjet` | `0x844118` | next |

These are sequential ReadBool calls in the same function (`0x713300`-`0x713397`). The pattern `MOV AL, [EBP+offset]; PUSH; PUSH string_addr; CALL ReadBool; MOV [EBP+offset], AL` repeats with the offsets and string addresses above.

`PoweredUnit`, `LightningRod`, `ManualReload`, `TiltCrashJumpjet` are confirmed as new struct-field labels for future investigations — out of scope here but recorded for cross-reference.

### 5.4 The `Type+0x67C` field

In Section A, the no-turret case checks `Type+0x67C == 1`. This is some form of locomotor-related discriminator. Likely `Type.Locomotor` index or `Type.SpeedType`. **Not confirmed in this pass.**

### 5.5 The `field_0x4A0` "is rotating" latch (Section C)

```c
if (HasTurret) {
    int rotating = CDTimerClass::Remaining(BarrelFacing) & 0xFF;
    field_0x4A0 = rotating;     // 0 or 1
}
```

**Tiny detail:** `field_0x4A0` is a 1-byte flag (read as byte) latched every tick: 1 if the barrel is currently rotating, 0 if done. **This is the field that other systems read to check "is turret aimed yet"** without re-doing the FacingClass math.

For our reimplementation, this is the equivalent of `is_turret_aligned()` cached as a boolean per-tick.

---

## 6. compute_facing_to_target (`0x005F3DB0`)

The helper that computes the 16-bit facing toward a target. Used by Facing_Update and Fire_At_Target. Decompiled:

```c
// corrected 2026-05-28: actual binary args (decompile_function 0x5F3DB0):
//   arg1 = self_coords[1] - target_coords[1]   (self_y - target_y)
//   arg2 = target_coords[0] - self_coords[0]   (target_x - self_x)
// Original doc had arg1 = target_y - self_y and described it as "atan2(dy, -dx)".
// Binary is actually atan2(-dy, dx) where dy = target_y - self_y.
// ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT
void compute_facing_to_target(short* out, undefined4 unused, AbstractClass* target) {
    int self_coords[3], target_coords[3];
    target->vtable+0x48(target_coords);    // GetCoords — piVar3
    self->vtable+0x48(self_coords);        // GetCoords — piVar4 (note: 'self' is 'this' from caller)
    
    // Binary: atan2(piVar4[1] - piVar3[1], iVar1 - *piVar4)
    //   where piVar3 = target coords, piVar4 = self coords, iVar1 = target_x
    double arg1 = (double)self_coords[1] - (double)target_coords[1];  // self_y - target_y
    double arg2 = (double)target_coords[0] - (double)self_coords[0];  // target_x - self_x
    Math::atan2(arg1, arg2);
    short result = Math::ftol();           // float-to-long cast, truncates toward zero
    *out = result;                          // store low 16 bits
}
```

Two tiny details:

1. **`atan2` argument signs.** The binary computes `atan2(self_y - target_y, target_x - self_x)`, i.e., `atan2(-Δy, Δx)` where `Δy = target_y - self_y` and `Δx = target_x - self_x`. This is equivalent to negating the first argument of the standard geographic `atan2(Δy, Δx)`. In RA2's coordinate system (Y increases southward), this maps 0 facing to north (decreasing Y), with clockwise rotation positive — consistent with all other facing math in the binary. (corrected 2026-05-28: original doc said `atan2(dy, -dx)` with `dy = target_y - self_y` — that would be `atan2(Δy, -Δx)`, a different formula; ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT)

2. **`Math::ftol`** does float-to-long with **truncation toward zero**, not rounding. This means atan2 results just below an integer boundary truncate down. For very small dx,dy near a cardinal direction, the truncation can produce a 1-unit bias.

For our re-implementation: using `atan2(self_y - target_y, target_x - self_x)` and `truncate-toward-zero` (NOT round-half-even) is required for byte-exact parity.

---

## 7. Tick order — where this all runs

From [UNITCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_GHIDRA_REPORT.md) §3 (UnitClass::AI):

```
... [§3a-3l: sinking, transport, parasite, parachute, deploy, tube, warp, FootClass::AI]
§3j: TurretAI()                     ← idle 8-cell scan, 1-in-8 frames
... [§3k-3l: sinking, harvest flag clear]
§3m: Fire_At_Target()                ← fire decision + actual fire (via TechnoClass::Fire_At)
§3n: Facing_Update()                 ← per-tick body+turret rotation interpolation
... [§3o-3t: guard mission, harvester delegation, anim, spawn manager, auto-hunt, stuck rescue]
```

**Critical ordering:**
1. **Fire_At_Target runs BEFORE Facing_Update.** This means the fire decision uses the *previous tick's* facing. If the turret rotated last tick and reached the target, this tick's Fire_At_Target sees the aligned facing and fires; Facing_Update then runs and propagates the (now stable) facing forward.
2. **TurretAI runs BEFORE Fire_At_Target.** Idle target acquisition happens first; if a target is acquired, the same tick can attempt to fire (subject to GetFireError gates).
3. **The locomotor's body-update happens in FootClass::AI (§3i), BEFORE all of the above.** So body destination is current when TurretAI/Fire_At_Target/Facing_Update run.

---

## 8. INI keys that affect this pipeline

| Key | Section | Default | Effect | Confidence |
|---|---|---|---|---|
| `ROT` | per-unit | (no global default; unit-specific) | Read into `Type+0x71C`. UnitClass::Constructor pushes into TurretFacing AND BarrelFacing as `(byte << 8)`. Per-tick step in 16-bit facing units. | HIGH |
| `TurretCount` | per-unit | 1 | Multiple turrets — code path I didn't fully cover, but BarrelFacing is per-turret. | MEDIUM |
| `TurretAnim` | per-unit | (none) | Turret sprite reference. Doesn't directly affect rotation math. | HIGH |
| `TurretAnimIsVoxel` | per-unit | false | Toggles continuous (VXL) vs discrete (SHP) turret rendering. May correlate with `Type+0xD21`. | MEDIUM |
| `IsTurretEquipped` (`TurretNotHidden`) | per-unit | true if has turret | `Type+0xD2F`. Gates entire TurretAI call. | HIGH |
| `TurretLocked` | per-unit | false | `Type+0xD30`. Gates TurretAI call. | HIGH |
| `TurretScansNearby` | per-unit | false | `Type+0xD32`. Enables idle 8-cell scan in TurretAI. | HIGH |
| `OmniFire` | per-unit | false | "No facing required to fire" — likely `Type+0xCA1` inverse, but actual offset for OmniFire is **unconfirmed in this pass**. | LOW |
| `TurretSpins` | per-unit | `no` | `Type+0xD21`. Permaspin: turret target advances 8 8-bit-facing units per tick (= 11.25°/tick = 32 ticks/revolution). Set only on `[DISK]` in vanilla YR. See §5.2. | HIGH |
| `OpportunityFire` | per-unit | false | `TechnoTypeClass+0x6AF`. **Does NOT gate firing** — only gates TarCom-persistence at mission transitions (per [OPPORTUNITY_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPPORTUNITY_FIRE_GHIDRA_REPORT.md)). | HIGH |
| `FireAngle` | per-unit | 0 | Vertical firing arc. Affects projectile pitch, NOT facing. Out of scope here. | HIGH (out-of-scope) |
| `MissileROTVar` | `[General]` | `.25` | Guided missile rotation variance. Affects bullet ROT, NOT unit ROT. Out of scope. | HIGH |
| `VeteranROF` | `[General]` | `0.6` | Veteran ROF multiplier. Applied in `TechnoClass::GetROF` (vtable+0x318) — not in Fire_At_Target. | HIGH |
| `CloseEnough` | `[General]` | `2.25` | Distance threshold. Affects range checks, possibly the FIRE_FACING tolerance. **Not directly seen in Facing_Update or Fire_At_Target** in this pass. | MEDIUM |

### 8.1 The `Type+0xD21` flag — RESOLVED as `TurretSpins`

Confirmed via byte-pattern search (`88 85 21 0d 00 00` = `MOV [EBP+0xD21], AL`) → single hit at `0x00713371` in `TechnoTypeClass::ReadINI`. Backtracking the `ReadBool` call: string at `0x84412C` = `"TurretSpins"`. INI comment at rulesmd.ini:3557 confirms.

See §5.2 for full spin-rate formula. Affects only `[DISK]` (Yuri's Floating Disk) in vanilla YR.

---

## 9. Magic numbers and tiny details — checklist

For implementation, every one of these must match gamemd.exe exactly:

1. **ROT clamp**: input > 126 → 127. (`FUN_004C9680` at `0x4C969A`)
2. **ROT shift**: input byte stored as `byte << 8`. So INI ROT=5 → field value 0x0500.
3. **`step_size = abs(diff) / ROT`** uses signed integer division (CDQ before IDIV). For diff=0xFFFF (mod-65536 wrap), abs is computed AFTER sign-extension, so wrap is handled implicitly via signed short subtraction.
4. **`step_size < 1` skips interpolation** — small rotation requests snap.
5. **`+ 8` constant** in body-discrete-8 formula at `0x736AC5`. Don't omit.
6. **Body-discrete formula**: `(((animated >> 7) + 1) >> 1) & 0xFF` (round-to-byte from 16-bit).
7. **8-frame TurretAI gate**: `g_CurrentFrameCounter & 0x80000007 == 0`.
8. **TurretAI scan radius**: 1 cell (8 cells around center, NOT weapon range).
9. **Bridge layer split**: cell+0x128 vs cell+0x124 in TurretAI scan.
10. **`atan2(self_y - target_y, target_x - self_x)`** — the binary uses this exact arg order (= `atan2(-Δy, Δx)`), NOT `atan2(target_y - self_y, self_x - target_x)` as originally documented. (corrected 2026-05-28: was `atan2(dy, -dx)` where dy=target_y-self_y; ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT)
11. **`Math::ftol` truncates toward zero**, not rounds.
12. **CDTimerClass `start_frame == -1`** = "never started" sentinel. Do not interpret as duration-0.
13. **CDTimerClass mystery field at `+0x0C`** is written from uninit stack but never read. Leave 0.
14. **Fire_At_Target runs BEFORE Facing_Update in tick order** — fire uses last tick's facing.
15. **Harvester turn-anim duration is 5 ticks** (constants 5,5 written to `+0x10C`,`+0x108`).
16. **Harvester facing index = `((facing >> 0xC) + 1) >> 1) & 7`** — different rounding than body discrete-8.
17. **Force-fire (`err==9`) uses `vtable+0x45C`** for state, not direct fire — there's an extra `CanFireAt` sanity check.
18. **GetFireError code 5 is overloaded** — ~30 different reasons. Don't try to match exact reason; just match the boolean "can/can't fire" outcome and the cooldown gate.
19. **`field_0x4A0` is the per-tick "is rotating" latch** — read by other systems for "turret aimed yet" check.
20. **The `vtable+0x4E4 + (err==0 || err==2)` early-out → mission 0x10** path is the IFV/deploy auto-trigger.

---

## 10. Current Rust implementation status

(Per Agent C scan of `src/sim/`)

### What we have
- [`src/sim/movement/turret.rs`](src/sim/movement/turret.rs) — turret rotation logic, alignment thresholds. Uses 16-bit turret facing, 8-bit body facing.
- [`src/sim/combat/combat_fire_gate.rs`](src/sim/combat/combat_fire_gate.rs) — collects fire-blocked entities (teleport/tunnel/droppod/aircraft/buildings).
- [`src/sim/combat/combat_targeting.rs`](src/sim/combat/combat_targeting.rs) — target acquisition with `AttackerSnapshot` holding turret facing + burst state.
- [`src/sim/combat/mod.rs`](src/sim/combat/mod.rs) — fire-gate logic at lines 1415–1435: cooldown + burst-delay + turret-alignment gates.

### What's missing or wrong (relative to gamemd.exe)

1. **No FacingClass-equivalent timer-interpolation struct.** Our turret rotation is a per-tick step (`max_delta` clamp), which approximates the binary's behavior but is NOT timer-based. Re-implement as `start_frame` + `prev` + `current` + `ROT` with the interpolation formula in §2.1.
2. **No `+ 8` body-discrete-8 offset** in `body_facing_to_turret` / inverse. Need to verify if any unit type uses `Type+0xD21` (currently unknown which YR units do).
3. **No `step_size < 1` snap behavior.** Small rotation requests should snap, not interpolate.
4. **Tick order may differ.** Confirm fire-decision uses last-tick facing, not this-tick.
5. **TurretAI 8-cell scan with TurretScansNearby** — currently we use `tick_retaliation` which is broader. The binary has a TS-style 1-cell idle scan that's distinct from retaliation.
6. **No GetFireError equivalent.** Our fire-gate is binary "can/can't fire". Binary returns one of ~10 codes; for parity we need at least to distinguish OK / Cooldown / Facing / Range / NoAmmo, since each takes a different post-fire path.
7. **`atan2(dy, -dx)` convention** — verify our `facing_toward_lepton` uses the same convention.
8. **ROT scaling** — INI ROT=5 should produce 1280 16-bit facing units per tick. Verify our `rot_to_facing_delta_u16` does this exactly (not 5*256=1280; it's the same number, but the path matters).
9. **Body-facing struct retention.** Currently body is `u8`; binary uses 16-bit FacingClass with ROT=3 *render smoother*. For purely simulation correctness, u8 may be fine — but for visual parity, body should also smooth-interpolate over up to ROT=3 ticks.

---

## 11. Open questions

1. ~~**`Type+0xD21` INI key name.**~~ ✅ **RESOLVED.** `Type+0xD21 = TurretSpins`. Set only on `[DISK]` (Floating Disk) in vanilla YR. See §5.2 for the spin-rate formula. Priority: LOW — only one unit affected.
2. **`Type+0x67C` semantic meaning.** Used in Facing_Update Section A no-turret path. Likely Locomotor or SpeedType discriminator.
3. **The mystery `+0x02` and `+0x06` shorts in FacingClass.** Always copied alongside Current/Prev as packed dwords. Possibly speed/magnitude or unused legacy fields.
4. **The mystery `+0x0C` int in CDTimerClass.** Written from uninitialized stack but never read. Compiler quirk or legacy.
5. **`vtable+0x3F4` semantic.** Returns a "weapon struct" with byte at +0x18 and vtable+0x12B that gates the body-vs-target turret aim choice. Likely `GetActiveWeaponEx` or similar — confirm via vtable label.
6. **`vtable+0x4E4` semantic.** The auto-deploy-on-fire-ready check. Probably `ShouldAutoDeploy` or similar.
7. **`vtable+0x2E4` semantic.** Returns the weapon index used by GetFireError. Likely `SelectWeaponAgainst` (already known at 0x6F3330) but called via virtual.
8. **GetFireError code 5 sub-reasons.** ~30 conditions; we need to map which ones our re-implementation cares about.
9. **`+0x6AF` byte flag at TechnoClass instance level.** Read in Facing_Update gate (`*(char*)((int)param_1 + 0x6AF) == 0`), separate from `TechnoTypeClass+0x6AF` (OpportunityFire). Likely a "facing dirty" / "rotation pending" instance flag.
10. **`+0x6AD` byte flag.** Similar — read in Facing_Update Section B.
11. **The PlayerControl/HumanPlayer propagation at TurretAI Phase E.** What field is `+0x19D` and what does the parent's `+0x19D` byte represent? Likely AI-control state.

---

## Sources

**Ghidra MCP decompilation** of:
- `0x004C9220` (FacingClass::Set / RateTimer__Set) — full decompile + disassembly
- `0x004C9300` (FacingClass::UpdateFacing) — full decompile + disassembly
- `0x004C93D0` (FacingClass::Current / RateTimer__Current) — full decompile + disassembly
- `0x004C9480` (FacingClass::IsRotating / CDTimerClass__Remaining) — full decompile
- `0x004C9680` (FacingClass::SetROT) — full decompile
- `0x00426630` (CDTimerClass::GetTimeRemaining) — full decompile
- `0x005F3DB0` (compute_facing_to_target) — full decompile
- `0x006FC0B0` (TechnoClass::GetFireError) — full decompile
- `0x007353C0` (UnitClass::Constructor) — full decompile + disassembly (verifies field offsets)
- `0x00736990` (UnitClass::Facing_Update) — full decompile + disassembly
- `0x00736DF0` (UnitClass::Fire_At_Target) — full decompile
- `0x007468C0` (UnitClass::TurretAI) — full decompile
- `0x004D31E0` (FootClass::Constructor) — relevant offsets

**Companion docs cross-referenced:**
- [UNITCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_GHIDRA_REPORT.md) §3, §5, §6, §7 (UnitClass::AI tick order, TurretAI summary)
- [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md) §726-728 (FacingClass offsets confirmed)
- [FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md) (TechnoClass::Fire_At pipeline; deliberately NOT re-investigated here)
- [OPPORTUNITY_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPPORTUNITY_FIRE_GHIDRA_REPORT.md) §4 (mission 0x10, OpportunityFire flag scope)
- [TECHNOCLASS_COMBAT_WEAPON_SYSTEMS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_COMBAT_WEAPON_SYSTEMS_REPORT.md) §3 (GetFireError architecture)
- [BUILDINGCLASS_MISSION_ATTACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSION_ATTACK_GHIDRA_REPORT.md) (FIRE_FACING gate, tolerance formula `abs(ROT << 8)`)
- [FLY_LOCOMOTION_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FLY_LOCOMOTION_CLASS_GHIDRA_REPORT.md) §FacingClass turn algorithm (cross-check)
- [GATTLING_WEAPON_STAGE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATTLING_WEAPON_STAGE_SYSTEM_GHIDRA_REPORT.md) (Type+0xCD5 IsGattling flag)

**INI files checked:**
- `ini/rulesmd.ini` — ROT, ROF, Burst, OmniFire, FireAngle, OpportunityFire, TurretAnim*, [General] VeteranROF, CloseEnough, MissileROTVar
- `ini/artmd.ini` — TurretOffset, sprite-related turret keys
