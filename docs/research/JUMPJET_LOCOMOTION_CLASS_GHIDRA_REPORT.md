# JumpjetLocomotionClass — Ghidra Report

**Binary:** gamemd.exe (Yuri's Revenge 1.001)
**Research date:** 2026-04-19
**CLSID:** `{92612C46-F71F-11D1-AC9F-006008055BB5}`
**ILocomotion vtable:** `0x007ECD68`
**Constructor:** `0x0054AC40`
**Scope:** JumpjetLocomotionClass is the flight locomotor used by Rocketeer, Siege Chopper, and other hover-capable air units. It implements the same `ILocomotion` COM interface as DriveLocomotionClass, FlyLocomotionClass, etc., but models a 6-phase takeoff→cruise→hover→land state machine with physical altitude handling distinct from FlyLocomotionClass (which used by aircraft-type units like Harrier, Black Eagle, Kirov).

This report consolidates the previously-scattered coverage (AIRCRAFTCLASS, ADDRESS_MAP, LAYER_CLASS, LOCOMOTION_MATH_AND_CONSTANTS) and fills in the unresearched details: the full state machine, the instance field map, the ILocomotion vtable layout, and the pointer-adjustment subtlety that governs how the class fields are addressed from method to method.

---

## 1. Units using JumpjetLocomotor

From `rules(md).ini` — 9 unit types declare `Locomotor={92612C46-F71F-11D1-AC9F-006008055BB5}`:

| Unit | Category | Notes |
|---|---|---|
| ROCK (Rocketeer) | Infantry | Allied GI-type that lifts into flight when deployed |
| SCHP (Siege Chopper) | Vehicle | Yuri air/ground transformable |
| HORV (Hornet from Aircraft Carrier) | Aircraft-spawn | Released by carrier Aircraft Carrier |

Plus a few base-RA2 ghosts that are dormant in retail YR.

Per [LAYER_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAYER_CLASS_GHIDRA_REPORT.md): "JumpjetLocomotion is used by 9 units" (includes Allied Paradrop helicopters whose visible flyer also uses this).

---

## 2. Instance layout (confirmed byte offsets)

**param_1 pointer discipline — critical.** ILocomotion is the SECOND virtual base in JumpjetLocomotionClass. When the `ILocomotion` vtable dispatches a call, MSVC adjusts `this` so the called method sees `this = instance + 4` (pointing at the ILocomotion sub-object). Internal helpers invoked directly from within the class receive the original `instance`. When reading the decompilation:

- If Ghidra signature is `int *param_1` with accesses like `param_1[n]` → likely called via ILocomotion vtable; `param_1 = instance + 4`. Add 4 to every offset to get the true instance offset.
- If Ghidra signature is `int param_1` with `*(… *)(param_1 + n)` → usually receives `instance` directly (confirmed in state handlers; see §5).

Verified from assembly: `0054aec0: MOV ESI, [ESP+0x28]` (Process stack-passed this = instance+4), then `LEA ECX, [ESI + -0x4]; CALL 0x0054b980` (state handler receives instance).

**True instance offsets** (relative to original `instance` pointer):

| Offset | Type | Field | Source |
|---|---|---|---|
| 0x00 | ptr | IUnknown vtable ptr | Constructor `*param_1 = &IUnknown_vtable` |
| 0x04 | ptr | ILocomotion vtable ptr (0x007ECD68) | Constructor `param_1[1] = …` |
| 0x08 | int | LocomotionClass base field (likely ref count) | Unused by Jumpjet logic |
| 0x0C | ptr | LinkedObject (TechnoClass\*) | State handlers use `[ESI+0xc]`; Process uses `param_1[2]` on (instance+4) |
| 0x18 | ptr | IPiggyback vtable ptr | Constructor `param_1[6] = …` |
| 0x28 | int (fixed?) | **Speed cache** (copy of TechnoType.Speed; used as speed-throttle baseline) | FUN_0054BA30 `[ESI+0x2c]` reads baseline |
| 0x2C | ? | CruiseHeight threshold OR cached speed | FUN_0054B980 copies 0x2C → 0x80 |
| 0x40 | Coord.X | **Destination X** | Ctor writes `NullCoord_Jumpjet_X`; state 4 resets |
| 0x44 | Coord.Y | Destination Y | Ctor `NullCoord_Jumpjet_Y` |
| 0x48 | Coord.Z | Destination Z | Ctor `NullCoord_Jumpjet_Z` |
| 0x4C | byte | **IsDirty / persist-state flag** | Ctor zeroes; vtable slot 4 (IsDirty) returns this byte |
| 0x50 | u32 | **State** (phase: 0–6) | Ctor `param_1[0x14] = 0`; state handlers write 1/2/3/4/5/6 |
| 0x54 | RateTimer | Turn-rate timer (16-bit tick counter at 0x4000) | Ctor sets via `RateTimer__Set` with 0x4000 |
| 0x70 | int | Pitch/velocity X (written 0 on takeoff) | State 0/3 zero this |
| 0x74 | int | Pitch/velocity Y | Same |
| 0x78 | double | Vertical velocity (Z) | FUN_0054BA30 `[ESI+0x78] = (double)...` |
| 0x7C | int | Vertical velocity high-dword + flag | State 3 writes `0x3FF00000` (double 1.0) when settling |
| 0x80 | int | **Current speed** (scaled from TechnoType.Speed) | State 0: `[ESI+0x80] = [ESI+0x2c]` sets cruise speed |
| 0x88 | int | (cleared in ctor, unknown use) | param_1[0x22] = 0 |
| 0x8C | int | (cleared in ctor) | param_1[0x23] = 0 |
| 0x90 | byte | "Landing-abort requested" flag | State 4 sets 1 when cell-occupancy forces retry |
| 0x91 | byte | (cleared, use unknown) | Ctor direct byte write |
| 0x94 | ptr | **Piggyback inner ILocomotion\*** (when piggybacked — e.g., transport IFV) | Destructor `[ESI+0x94]->Release()` |

Confidence: 0x00, 0x04, 0x0C, 0x18, 0x40-0x48, 0x50, 0x94 are **verified** from constructor + assembly. 0x28/0x2C distinction between "type speed" vs "current speed" is **inferred** — needs cross-check.

---

## 3. ILocomotion vtable (`0x007ECD68`)

Slot-by-slot, from memory dump at `0x007ECD68`. Slot index = byte offset / 4. First 3 slots are IUnknown; slots 3–7 are IPersistStream; slots 8+ are ILocomotion-specific.

| Byte off | Slot | Address | Name (confirmed / inferred) | Notes |
|---|---|---|---|---|
| 0x00 | 0 | 0x0054DFF0 | IUnknown::QueryInterface | |
| 0x04 | 1 | 0x0054E000 | IUnknown::AddRef | |
| 0x08 | 2 | 0x0054E010 | IUnknown::Release | |
| 0x0C | 3 | 0x0054AD30 | IPersistStream::GetClassID | |
| 0x10 | 4 | 0x0054AE50 | **IPersistStream::IsDirty** | confirmed — returns byte at instance+0x4C |
| 0x14 | 5 | 0x0054AE60 | IPersistStream::Load | |
| 0x18 | 6 | 0x0054D9B0 | **ILocomotion::Destination** | confirmed — returns instance-coord-0x40 when state≠0 else linked_obj→0x9C |
| 0x1C | 7 | 0x0055ABF0 | IPersistStream::Save / GetSizeMax | |
| 0x20 | 8 | 0x0055ABE0 | ILocomotion::Link_To_Object | 3-line stub likely |
| 0x24 | 9 | 0x0054DCC0 | **ILocomotion::Draw_Matrix** (voxel lean) | confirmed — builds 3x4 transform; applies tilt when TechnoType.CanLean & |pitch|/|bank| above threshold at 0x328/0x32C |
| 0x28 | 10 | 0x0055A7D0 | (base class; returns 0 likely) | |
| 0x2C | 11 | 0x0055ABD0 | | |
| 0x30 | 12 | 0x0055A8C0 | **LocomotionClass::Draw_Point** (base-class) | confirmed — returns {0, AdjustForZ(obj->GetHeight())} |
| 0x34 | 13 | 0x0055ABC0 | | |
| 0x38 | 14 | 0x0055ABA0 | | |
| 0x3C | 15 | 0x0055ABB0 | | |
| 0x40 | 16 | 0x0054AEC0 | **ILocomotion::Process** (per-tick AI) | confirmed — dispatches state machine (§5) |
| 0x44 | 17 | 0x0054B1C0 | ILocomotion::Move_To / Head_To_Coord | |
| 0x48 | 18 | 0x0054B4D0 | ILocomotion::Do_Turn | |
| 0x4C | 19 | 0x0054B6E0 | | |
| 0x50 | 20 | 0x0055AC20 | | |
| 0x54 | 21 | 0x0055AB90 | | |
| 0x58 | 22 | 0x0055A8F0 | | |
| 0x5C | 23 | 0x0055A910 | | |
| 0x60 | 24 | 0x0055A930 | | |
| 0x64 | 25 | 0x0055A940 | | |
| 0x68 | 26 | 0x0055AB70 | | |
| 0x6C | 27 | 0x0055AB80 | | |
| 0x70 | 28 | 0x0055AC10 | **empty stub** (void return) | confirmed — no-op method |
| 0x74 | 29 | 0x0054B8D0 | **ILocomotion::In_Which_Layer** | confirmed (§6) — returns 2/3/4 |
| 0x78 | 30 | 0x0055AC00 | | |
| 0x7C | 31 | 0x0055ACE0 | | |

Unlabeled slots are confirmed present in the vtable but their semantics are not verified; many (0x0055xxxx range) are shared base-class methods returning constants or forwarding to LocomotionClass/LinkedObject.

---

## 4. Constructor / destructor

- `JumpjetLocomotionClass::Constructor @ 0x0054AC40` — writes three vtables, calls base `LocomotionClass__Constructor` (sets LinkedObject = null at 0x0C), initializes destination coord at 0x40/0x44/0x48 to `NullCoord_Jumpjet_{X,Y,Z}` sentinel values, clears state (0x50 = 0), sets turn-rate timer (0x54) = 0x4000, calls `FUN_004c91e0(RulesClass+0x40C=TurnRate)` — likely initializes a facing/rate helper.
- `JumpjetLocomotionClass::~Destructor @ 0x0054AD00` — releases `[ESI+0x94]` (piggyback target), calls `LocomotionClass__Destructor`.
- `JumpjetLocomotionClass::`scalar-deleting-destructor` @ 0x0054DFA0` — same + optional `operator delete` via `FUN_007c8b3d`.

Constructor accesses `param_1[0x10/0x11/0x12]` (NullCoord), which are BYTE OFFSETS 0x40/0x44/0x48 because `param_1` here is `undefined4 *` and pointer arithmetic multiplies by 4. Previously documented correctly.

---

## 5. State machine (6 phases)

Dispatched by `Process @ 0x0054AEC0` via jump table at `0x0054B19C` indexed by instance+0x50. State values 0..6; state 6 has NO handler (falls through).

| State | Name (inferred) | Handler | Role |
|---|---|---|---|
| 0 | **Grounded / idle** | FUN_0054B980 | On ground; waits for target. Transitions → 1 when linked_obj receives a destination (`[linked_obj+0x560] != current cell coord`). Zeros velocity (0x70/0x74/0x78/0x7C), sets 0x80 = 0x2C (cruise-speed baseline) |
| 1 | **Lift off / ascend** | FUN_0054BA30 | Rising to CruiseHeight. Computes turn-rate via `Math__atan2(Δy, Δx)`. Cell-occupancy blocked → scatter (`Random__RandomRanged(0,7)`, pathfind retry). Transitions → 2 (decelerating cruise) when at altitude + at-destination, or 3 (approach) when still traveling |
| 2 | **Approach / decelerating cruise** | FUN_0054BD30 | Horizontal cruise toward destination. If destination reached → 3. If landing target is a passenger-pickup building/harvester → hold state. Transitions → 4 (descend) when close enough or onto cell the linked_obj occupies |
| 3 | **Horizontal cruise (long range)** | FUN_0054BFF0 | Long-range flight with speed ramping based on remaining distance (iVar9 = sqrt((dx)²+(dy)²)): |
| | | | • `iVar9 < 0x14` (≈ 20 leptons) → settle vertical (0x78 = 0, 0x7C = 1.0), transition → 4 |
| | | | • Below `0x20` threshold → half speed, maybe tilt |
| | | | • Below `0x20 × 2` → quarter speed |
| | | | • Above `(0x20 × 0x32) / 0x1c` → full Speed |
| | | | Wobble/sine heading applied via `DAT_007e44e8`. On target-cell landing-type 2/6 (beach/water) forces full speed to escape |
| 4 | **Descend / land** | FUN_0054C550 | Altitude bleeds off. At altitude 0: checks cell passability/subcell, rings landing sound (`RulesClass+0x48` = 0 (byte) landing-volume/delay), resets coord to NullCoord (0x40/0x44/0x48), state → 0, clears linked_obj flags 0x425/0x427, sets 0x6AE (landed). Aborts back to state 3 if forced-move requested |
| 5 | **Abort / emergency** | FUN_0054CA90 | Entered when destination becomes invalid mid-flight. Finds alternate cell via `(**linked_obj.vtable[0x48])` (Fetch_Current_Coord?), tries to find free cell or forces land immediately. Plays crash voice `0x117C`. Transitions → 6 if linked_obj already destroyed. State 4 resets linked_obj + nullifies destination |
| 6 | **Aborted (terminal)** | — (no handler) | Sentinel value; Process case-6 short-circuits (skips state handler, but still runs post-movement common code) |

**Entry flow in Process (FUN_0054AEC0)**:
1. Call `vtable[0x74]` (unknown ILocomotion method; probably `Is_Moving`) → stash result.
2. Call `vtable[0x10]` (IsDirty) — if dirty OR `vtable[0x80]` → run state machine.
3. Call `FUN_0054D0F0(instance)` — internal pre-step (likely piggyback integrator).
4. If linked_obj->0x90 ("needs display resubmit") → run rest of state.
5. If `FUN_0053a130()` true + state ∈ (1,2,3,4) → call `linked_obj.vtable[0x3DC]` (EnemyInRange check?).
6. If linked_obj->0x425 ("moving away from evac") + state ∉ (5,6) + linked_obj.vtable[0x1C8] > 0 (has altitude) → cell-arrival recompute: update destination from Fetch_Current_Coord, or if arrived force state=5 with 0x7C = -5 (drop/land) and — if rtti == 0xF (aircraft) — call vtable[0x558](0x22,0,0) (abort script 0x22).
7. `switch (state) { case 0..5: LEA ECX, [ESI-4]; CALL handler; }`.
8. Post-state: if linked_obj->0x83 (in-player-sight) and owner ≠ g_PlayerPtr → run shroud/fog reveal for the coord (gated on SpecialFlags & 0x1000 which is fog-of-war — **TS-legacy for retail YR** per CLAUDE.md; effectively disabled).
9. If linked_obj->0x41b cleared + owner ≠ g_PlayerPtr → call vtable[0x198] (Unshroud_For_Player).
10. If linked_obj->0x90 still set + `vtable[0x74]` result ≠ param_1 → `DisplayClass__Submit_Object(linked_obj)`.

---

## 6. In_Which_Layer @ 0x0054B8D0

Returns an altitude-layer enum (2 = Ground, 3 = Top_Low, 4 = Top_High) used by LayerClass for z-sorting. With `param_1 = instance + 4`:

```c
char JumpjetLocomotionClass::In_Which_Layer(this+4) {
    altitude = linked_obj->Get_Height();                // vtable[0x1C8]
    if (!linked_obj->InAir_0x8C) {                       // byte at ObjectClass+0x8C
        cell = MapClass::Get_Cell_At(linked_obj->coord); // uses linked_obj coord 0x9C/0xA0/0xA4
        if ((cell->CellFlags_0x140 & 0x100) != 0 &&      // 0x100 = "is water" flag
            altitude >= g_BridgeHeight_abc5dc &&
            !linked_obj->OnBridge_0x8D)                  // byte at ObjectClass+0x8D
        {
            altitude -= g_BridgeHeight_abc5dc;           // adjust for bridge-cell overhead
        }
    }
    if (!linked_obj->Is_Visible())                       // vtable[0x54]
        return 2;                                        // Ground layer (drawn before terrain)
    if (altitude == 0)
        return 2;                                        // Treated as grounded
    return (altitude >= instance+0x2C /* CruiseHeight or Speed cache */) ? 4 : 3;
}
```

Return codes `2/3/4` match the LayerClass layer indices. Confidence **high** on the code path; the `instance+0x2C` field remains labeled uncertain (CruiseHeight vs Speed-cache).

---

## 7. INI reader — `[JumpjetControls]` section

`RulesClass::Read_Jumpjet_Controls @ 0x006743D0` reads the `[JumpjetControls]` section of rulesmd.ini IF the section exists. All fields written to RulesClass instance offsets (NOT to JumpjetLocomotionClass):

| RulesClass off | Type | INI Key | Default | Used by |
|---|---|---|---|---|
| 0x40C | int | `TurnRate` | (INI default) | JumpjetLocomotionClass ctor (turn-speed) |
| 0x410 | int | `Speed` | | Runtime speed-cap |
| 0x418 | double (8 B) | `Climb` | | Vertical velocity up/down |
| 0x420 | int | `CruiseHeight` | | Target altitude for state 2/3 |
| 0x428 | double | `Acceleration` | | Speed ramp rate |
| 0x430 | double | `WobblesPerSecond` | | Horizontal wobble frequency |
| 0x438 | int | `WobbleDeviation` | | Horizontal wobble amplitude |

Padding observations (int vs double 8-byte aligned): 0x410→0x418 is 8 bytes (int + 4 B pad for double alignment); 0x420→0x428 is 8 B (int + 4 B pad for double); 0x430→0x438 is 8 B (double). Structure is cleanly packed for double alignment.

Section presence is gated by `FUN_00526810(PTR_s_JumpjetControls_007f0ce0)` (checks `Has_Section`). Returns 1 if read, 0 if section missing — skip-read leaves defaults in place.

---

## 8. Piggyback integration (0x94)

The Piggyback pattern: a transport unit can "borrow" another unit's Locomotor. When an IFV is piggybacked, its `JumpjetLocomotionClass` stores the piggyback-controlled inner ILocomotion* at instance + 0x94. Destructor releases it via `inner->Release()`. Per-tick update presumably happens in `FUN_0054D0F0` (called from Process step 3). IPiggyback vtable at instance+0x18 exposes the public interface.

Non-JumpjetLocomotor types with "CanPiggyback" flag (INI `Yes`) can pick up/drop a piggybacked locomotor via this mechanism — this is how the IFV morph system physically swaps locomotors when loading a Rocketeer (ground-drive → jumpjet-fly). Not confirmed here end-to-end; the cross-reference is noted for a future report on `IFV_MORPH_LOCOMOTION`.

---

## 9. Interactions with other systems

- **LayerClass** — In_Which_Layer feeds into z-order ([LAYER_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAYER_CLASS_GHIDRA_REPORT.md) §Layer mapping). Jumpjet-driven units are z-tested against altitude threshold (0x2C) — below it sort with ground top, above it sort with top-high.
- **FlyLocomotionClass** — shares AIRCRAFT-category units (Kirov, Nighthawk, Flak Track heli) but uses different physics. See [FLY_LOCOMOTION_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FLY_LOCOMOTION_CLASS_GHIDRA_REPORT.md).
- **UnitClass / FootClass::Process** — dispatches to locomotor.vtable[0x40] (Process) each tick (see FOOTCLASS_PATHFINDING_AND_MOVEMENT.md and [FOOTCLASS_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_AI_GHIDRA_REPORT.md)).
- **CellClass flag 0x100** = water cell — In_Which_Layer subtracts BridgeHeight on water cells.
- **CellClass LandType 2/6** = Water / Beach — state 3 forces full-speed when approaching (no hovering over water).

---

## 10. TS-legacy filter

Confirmed active in YR:
- All 6 state handlers run during normal skirmish gameplay.
- JumpjetControls INI parsing runs from RulesClass.

Flagged TS-legacy:
- **Fog-of-war branch in Process step 8** (`SpecialFlags & 0x1000`) — per CLAUDE.md, fog defaults OFF in YR. This path is dormant.
- Destructor-scalar-deleting-destructor (0x0054DFA0) uses `FUN_007c8b3d` (__purecall helper); standard MSVC compiler output, not gameplay-relevant.

No TS-only unit types found among the Jumpjet-locomotor consumers.

---

## 11. Gaps / open questions

1. **Field 0x28 vs 0x2C** — which is cached type-speed and which is the cruise-height threshold? In_Which_Layer reads 0x2C as an altitude comparator (suggests CruiseHeight cache), but state 0 copies 0x2C → 0x80 (suggests type-speed cache). Cannot both be true. One of the decompilation reads may be off. Needs cross-reference against `FUN_004c91e0` (called from ctor with TurnRate) and the LocomotionClass base ctor output.
2. **Unnamed vtable slots 11, 13–15, 20–28, 30–31** — most are shared base-class methods; needs table-walk of LocomotionClass vtable to identify which are overridden by Jumpjet. Low priority — doesn't block any simulation implementation.
3. **Wobble math** — `WobblesPerSecond` / `WobbleDeviation` not yet traced through Process. Likely a sine-driven heading adjustment applied in state 2 or 3. Needs targeted trace of `DAT_007e44e8` and similar floats in FUN_0054BFF0.
4. **Pickup/dropoff handoff** — Jumpjet ↔ Drive/Walk locomotor swap (Rocketeer entering IFV, Yuri mind-controlling a Rocketeer) flows through the Piggyback mechanism; needs a dedicated IFV_MORPH_LOCOMOTION report.

---

## 12. Confidence summary

| Claim | Confidence | Basis |
|---|---|---|
| Vtable address 0x007ECD68 | **Verified** | LOCOMOTION_MATH doc + memory dump |
| Ctor address 0x0054AC40 | **Verified** | Existing symbol + decompilation |
| Instance field 0x0C = LinkedObject | **Verified** | Assembly (`MOV ECX, [ESI+0xc]` in all state handlers) |
| Instance field 0x40/0x44/0x48 = Destination coord | **Verified** | Constructor + state 4 landing reset both write NullCoord sentinel here |
| Instance field 0x50 = State (0..6) | **Verified** | `MOV dword ptr [ESI + 0x50], 0x1` at 0054ba25 and equivalents |
| State 0 handler = 0x0054B980 | **Verified** | Jump table at 0x54b19c |
| State handlers called with instance (not instance+4) | **Verified** | `LEA ECX, [ESI + -0x4]` before each state call in Process |
| Process receives this+4 (ILocomotion slot) | **Verified** | x86 MI calling convention + `[ESI+0x8]` accessing linked_obj at true 0xC |
| In_Which_Layer @ 0x0054B8D0 | **Verified** (existing LAYER_CLASS doc + re-decompilation) |
| JumpjetControls INI offsets in RulesClass | **Verified** | ReadJumpjetControls decompilation |
| Process at vtable slot 16 (offset 0x40) | **Verified** | memory dump |
| Field 0x28 vs 0x2C semantics | **Inferred, conflicting evidence** | See §11.1 |
| Wobble implementation site | **Inferred** (likely FUN_0054BFF0 state 3) | Needs targeted trace |
| IFV piggyback handoff | **Inferred** | Based on IPiggyback vtable presence; not traced end-to-end |

---

*Generated 2026-04-19 via Ghidra MCP live decompilation.*

---

**Verified 2026-04-19** (independent re-check, second pass)

Re-checked the following key claims directly against the binary; all hold:

- **Unit count (§1):** confirmed 9 INI declarations of `Locomotor={92612C46-F71F-11D1-AC9F-006008055BB5}` in `ini/rulesmd.ini` (lines 3948, 4740, 8725, 10553, 10852, 10913, 11181, 11259, 27300). The "9 units" figure is exact, not approximate.
- **Pointer-adjustment subtlety (§2):** confirmed via `0054aec4: MOV ESI,[ESP+0x28]` (Process receives this+4) and `0054b043: LEA ECX,[ESI + -0x4]; CALL 0x0054b980` (state handler dispatch with instance). Same `LEA ECX,[ESI-0x4]` pattern at 0054b04d/b057/b061/b06b/b075 for states 1–5.
- **Jump table (§5):** confirmed `0054b03c: JMP dword ptr [EAX*0x4 + 0x54b19c]` indexed by `[ESI+0x4c]` (which is true offset 0x50 because Process sees this+4). Six handler addresses match the table.
- **Field 0x0C = LinkedObject:** confirmed independently from two angles — Process reads `[ESI+0x8]` (= instance+0x0C) at 0054af06 to access `linked_obj+0x21c`; state handlers read `*(int **)(param_1 + 0xc)` at the start of every handler.
- **Field 0x40/0x44/0x48 = Destination coord:** confirmed in constructor (`MOV [EAX], ECX/EDX/ECX` from 0xabc5a8/0xabc5ac/0xabc5b0 = g_NullCoord_Jumpjet_X/Y/Z) and re-confirmed in state 4 reset (`*this = g_NullCoord_Jumpjet_X` at 0054c8e1-area).
- **Field 0x50 = State:** confirmed via state-handler writes of small integers 1..6 to `*(undefined4 *)(param_1 + 0x50)` and via Process switch on `[ESI+0x4c]` (= instance+0x50).
- **JumpjetControls INI offsets in RulesClass (§7):** decompile of 0x006743D0 confirms TurnRate→0x40C, Speed→0x410, Climb→0x418 (double), CruiseHeight→0x420, Acceleration→0x428 (double), WobblesPerSecond→0x430 (double), WobbleDeviation→0x438. Reader is `param_1 = int` (direct byte offsets).
- **Vtable reads:** memory dump at 0x007ECD68 (256 B) and 0x007ECD44 (64 B) confirm slot-0..3 are IUnknown thunks (0x0054DFF0/E000/E010), slot-16 (offset 0x40) is Process at 0x0054AEC0, slot-29 (offset 0x74) is In_Which_Layer at 0x0054B8D0.
- **In_Which_Layer (§6):** decompile of 0x0054B8D0 confirms it reads `*(int *)(param_1 + 0x28)` as altitude comparator. Since this method is dispatched via ILocomotion vtable, `param_1 = instance + 4`, so the comparator field is true instance+0x2C — matching the report's §6 code. Same field is also read by state 0 (FUN_0054B980) and copied into instance+0x80; this remains the open §11.1 question (CruiseHeight cache vs SpeedCache reading), unchanged.
- **TS-legacy filter (§10):** the fog-of-war branch in Process is gated on `*DAT_00a8b230 & 0x1000` (SpecialFlags.FogOfWar) — confirmed dormant in default YR per CLAUDE.md. No change.

No corrections needed. Report is accurate and well-structured. Open questions in §11 remain valid; the 0x28-vs-0x2C ambiguity is genuinely under-determined by what's visible in Process / In_Which_Layer alone and would need a Link_To_Object trace to resolve.

---

# Round 3 Extension — Parachute / paradrop use case (2026-05-05)

Scope: focused extension covering the **ParachuteLocomotion** COM class (CLSID `{92612C46-F71F-11D1-AC9F-006008055BB5}`) and its relationship to JumpjetLocomotionClass, including the descent-rate application path for paradropped infantry. This was triggered by the [PARADROP_SUPERWEAPON_GHIDRA_REPORT.md] which had identified ParachuteLocomotion as a separate CLSID and inferred (incorrectly, in part) that it was a distinct class.

## R3.1 — Critical correction: there is NO separate ParachuteLocomotionClass

The CLSID `{92612C46-F71F-11D1-AC9F-006008055BB5}` documented in §1 of this report is **the only CLSID** for this locomotor. The PARADROP report's hypothesis of a "ParachuteLocomotion" CLSID distinct from JumpjetLocomotion was based on a misidentification — there is no second CLSID and no second class.

**Verification by raw disassembly of the class factory at `0x006C4190`** (which the registration code at `0x006BD198` registers under CLSID `0x007E9AC0` = `92612C46-...`):

```
006c41bc: PUSH 0x98              ; sizeof(JumpjetLocomotionClass) = 152 bytes
006c41c1: CALL 0x007c8e17        ; operator new
006c41c6: ADD ESP, 0x4
006c41c9: TEST EAX, EAX
006c41cb: JZ  0x006c41da
006c41cd: MOV ECX, EAX
006c41cf: CALL 0x0054ac40        ; ← JumpjetLocomotionClass::Constructor
```

`get_xrefs_to(0x0054AC40)` returns exactly one caller: `0x006C41CF in FUN_006C4190`. So the CLSID factory at `0x6C4190` is the *only* path to the constructor at `0x54AC40`, and it constructs the same C++ class that Rocketeer/Siege Chopper use.

**Implication:** the `Locomotor=` GUID on infantry types is identical to the GUID on Rocketeer's locomotor entry. The CLSID is just *the name of the locomotor class* — there is no parachute-specific subclass.

## R3.2 — How then does paradrop descent actually work?

Tracing what Drop_Payload actually does after Unlimbo (`AircraftClass::Drop_Payload @ 0x00415C60`, raw assembly):

```
00415de8: CALL [EAX + 0xe8]             ; passenger.Unlimbo(drop_pos)  (vtable+0xE8)
00415dee: TEST AL, AL
00415df0: JZ 0x00415eb1                 ; if Unlimbo failed, fail path
00415df6: LEA EDX, [EDI + 0x9c]         ; aircraft.Coord
00415dfc: PUSH 0x0
00415dfe: ...                            ; play ChuteSound (Rules+0x71C)
00415e21: CALL VocClass::PlayAt
00415e26-00415e74: ...                   ; set passenger.field_55C, frame, cell coords on aircraft
00415e93: MOV byte ptr [EDI + 0x6d3], 0x5 ; aircraft.LandingState = 5
```

**There is NO `Begin_Piggyback` call. No locomotor swap. No instance of JumpjetLocomotionClass is constructed for the dropped infantry.** The passenger's locomotor remains its base type (typically `WalkLocomotionClass` for Infantry).

This means the prior PARADROP-report Q23 finding ("ParachuteLocomotion is JumpjetLocomotion under a different CLSID, attached as piggyback to dropped infantry") is **half right and half wrong**:
- ✅ **Right**: there's only one class (the one documented in this report).
- ❌ **Wrong**: that class is NOT attached to dropped infantry. Paradropped units do NOT carry an instance of JumpjetLocomotionClass during descent.

## R3.3 — So what plays the "Paradrop" sequence on falling infantry?

`FootClass::Locomotion_AI @ 0x00520F40` does have a CLSID compare against `0x007E9AC0` (the JumpjetLocomotion CLSID block) and dispatches to sequence 0x17 ("Paradrop") when matched. Here's the logic:

```c
piVar4 = passenger.PrimaryLocomotor       // ObjectClass+0x19D
QueryInterface(piVar4, IPiggyback, &out)   // ask for IPiggyback interface
GetClassID(out, &out_clsid)
compare out_clsid against DAT_007E9AC0    // 16-byte memcmp
if (matches) {
  if (passenger.field_68d == 0) {
    if (passenger.speed <= 0)
      DoType(0x17, 0, 0)   // Paradrop sequence
    else
      DoType(0x18, 0, 0)   // ParadropMoving sequence
  }
}
```

The CLSID match fires when the passenger's primary locomotor exposes `IPiggyback` whose GetClassID returns the JumpjetLoco CLSID. **For Rocketeers (which have JumpjetLoco as their primary), this gate fires every tick** — and they play sequence 23 / 24 according to whether they're moving.

For Rocketeer, sequences 23 / 24 are mapped (via artmd `Paradrop=N,...`) to its **flight poses** (hovering / horizontal flight). For paradropped infantry like E1, those same sequences map to its **parachuting** sprite frames.

So the mechanism is: the gate is generic ("locomotor is JumpjetLocomotion-class"), and per-unit `Paradrop=` art entries customize what frame each unit shows in that mode.

But this still leaves open: **for paradropped infantry, the gate above won't fire**, because their locomotor is `WalkLocomotionClass`, NOT JumpjetLocomotion. So how do they get their Paradrop sequence shown?

Best hypothesis (unverified by this round): the dropped infantry is rendered with `Object+0x88` (parachute Anim) overlaid for the descent. The infantry's own sprite uses its standard "stand" frame; the parachute Anim provides the visual umbrella above. The artmd `Paradrop=N,1,0` line on each unit may ONLY be consumed for Rocketeer-class hover units, not for paradropped infantry. **This is now an open question (see R3.7).**

## R3.4 — How is altitude actually decremented during descent?

State 4 (`FUN_0054C550 @ 0x0054C550`, the "Land" state) was assumed to handle altitude bleed. After full re-examination of state 4, **it does not contain any altitude-decrement code**. State 4 only:
1. Reads cell at destination coord.
2. Detects water/beach landing zones and triggers scatter (`Random__RandomRanged(0,7)`) → state 3.
3. Detects sub-cell occupancy collisions (`CellClass::IsSubCellFree`) and sets `instance+0x90 = 1` (abort flag).
4. **When altitude == 0** (read via `linked_obj.vtable[0x1C8]` = GetHeight): runs the landing finalize sequence (reset destination to `g_NullCoord_Jumpjet`, set state = 0, set `linked_obj+0x6AE = 1` (landed flag), clear `linked_obj+0x425/+0x427`, optionally play ChuteSound).

**Altitude decrement happens elsewhere.** Best candidates:
- `LocomotionClass::Process` (the base-class virtual called inside Process step 3 — `FUN_0054D0F0`) which may handle altitude integration.
- `ObjectClass::AI` / `ObjectClass::Process` which is called by FootClass::Process on every tick.

Decompiling `FUN_0054D0F0` (the "internal pre-step" before state dispatch) would resolve this. Not done in round 3. **Now an open question (see R3.7).**

## R3.5 — Rules.ParachuteMaxFallRate consumer scan

Searched for byte-pattern access to `g_RulesClass_Instance + 0x7B8` (where ParachuteMaxFallRate is stored, verified in PARADROP report). Tested patterns:
- `8b 8e b8 07 00 00` (`MOV ECX, [ESI+0x7B8]`) — 1 match at `0x00717949`
- `8b 80 b8 07 00 00` (`MOV EAX, [EAX+0x7B8]`) — no matches
- `b8 07 00 00` (the offset bytes alone) — many matches, predominantly false positives

The single hit at `0x00717949` (inside `FUN_007178C0`, called from `TechnoClass::AI_Update` and `TechnoClass::ReceiveDamage`) reads from `param_1 + 0x7B0/4/8/C` — but `param_1` there is a **TechnoClass instance**, not RulesClass. The byte pattern matched coincidentally on different semantics. **TechnoClass+0x7B0..0x7BC is unrelated to ParachuteMaxFallRate.**

Conclusion: **`Rules.ParachuteMaxFallRate` has no easily-locatable consumer** via direct memory-offset access. Three possibilities, in decreasing likelihood:

1. The field is loaded into a different intermediate register (e.g., via `MOV EBX, g_Rules; MOV EAX, [EBX+0x7B8]` where the offset is encoded differently due to register choice).
2. The field is read via a wrapper function or accessor that masks the direct offset.
3. The field is **TS-legacy and unused in YR** (parsed but never consumed).

Possibility (3) is most worrying for parity. If `ParachuteMaxFallRate` is dead code, the actual descent rate must be hardcoded somewhere else. **This is now an open question (see R3.7).**

## R3.6 — Verified: §11.1 ambiguity (instance+0x28 vs +0x2C)

Round 1's open question §11.1 asked which of `instance+0x28` and `instance+0x2C` is the cached type-speed and which is the cruise-height cache. **Resolved this round as +0x2C = CruiseHeight cache.**

Verification chain:

1. **In_Which_Layer** (`FUN_0054B8D0 @ 0x0054B8D0`) is dispatched via the ILocomotion vtable, so `param_1 = instance + 4`. Its return value:
   ```c
   return (*(int *)(param_1 + 0x28) <= iVar2) + '\x03';
   ```
   With `param_1 = instance + 4`, this reads `*(int *)(instance + 0x2C)` and compares against `iVar2 = altitude`. Returns 3 if `altitude < instance+0x2C`, 4 otherwise. So **instance+0x2C is the altitude threshold** distinguishing "Top_Low" (3) from "Top_High" (4) — i.e., CruiseHeight.

2. **State 0 handler** (`FUN_0054B980 @ 0x0054B980`) is called with `param_1 = instance` (via `LEA ECX, [ESI-0x4]` from Process). It executes:
   ```c
   *(undefined4 *)(param_1 + 0x80) = *(undefined4 *)(param_1 + 0x2c);
   ```
   This copies `instance+0x2C` → `instance+0x80`. State 1 (Liftoff) then reads `instance+0x80` as the **altitude target** (climbs until current altitude reaches it).

The flow makes sense end-to-end: state 0 caches CruiseHeight as the target, state 1 uses that target to know when to stop ascending, and In_Which_Layer uses CruiseHeight (via instance+0x2C directly) to decide z-sort layer.

**§11.1 final answer:**
- `instance+0x28` — **still uncertain**, but likely the cached `Speed` (from RulesClass.JumpjetControls.Speed at `+0x410`) given it's adjacent to the CruiseHeight cache and the constructor likely caches both.
- `instance+0x2C` — **CruiseHeight cache** (from RulesClass.JumpjetControls.CruiseHeight at `+0x420`). VERIFIED.
- `instance+0x80` — **multi-purpose working field**:
  - State 0: set to instance+0x2C (target altitude for liftoff).
  - State 1: read as altitude target during climb; once reached, can be re-purposed.
  - State 5: forced to `-5` when destination becomes invalid (negative value as crash-descent rate?).
  - State 4 (after landing): set to 0 (no speed).

This overloading of `+0x80` is a classic in-place reuse pattern in optimized C++. The field's name in the original source was probably "WorkingValue" or similar.

## R3.7 — Round 3 Open Questions (newly raised)

These were not in the original §11; they emerge specifically from the parachute use-case investigation:

1. **Where does ObjectClass altitude get decremented for paradropped infantry?** Not in JumpjetLocomotion's state handlers. Not visible in Drop_Payload. Likely candidates: `LocomotionClass::Process` (base class), `ObjectClass::AI`, or `FootClass::Process`. Needs a targeted decompile of `FUN_0054D0F0` (the pre-state Process step) and/or InfantryClass::AI.

2. **Is `Rules.ParachuteMaxFallRate` actually consumed in YR?** Byte-pattern scan found no obvious consumer. May be (a) loaded indirectly, (b) read via an accessor, or (c) **TS-legacy dead field**. If (c), the actual descent rate is hardcoded elsewhere — finding it is critical for parity.

3. **For paradropped infantry, when/how is the Paradrop sequence (artmd `Paradrop=N,1,0`) actually shown?** The FootClass::Locomotion_AI gate matches on JumpjetLoco CLSID, which dropped infantry don't have. Either:
   - The visual is achieved purely via the parachute Anim attached at `Object+0x88` (the infantry plays its standard idle frame *under* the chute), and the per-infantry `Paradrop=` artmd line is unused for paradrop SW (only used by Rocketeer-class units).
   - OR there's a *second* Paradrop-sequence trigger I haven't found — perhaps gated on `IsParachuted` flag (`Object+0xF5` / byte `+0x3D4`).
   - This affects render fidelity. Test by inspecting what pose a paradropped GI shows in gamemd.exe.

4. **What is `FootClass+0x68d`?** It gates the FootClass::Locomotion_AI Paradrop-sequence dispatch. Likely "is permanent flier" (Rocketeer = no, paradropped GI = no) or "skip-paradrop-sequence" (Rocketeer = yes). Determining this confirms which units actually trigger the gate.

5. **Does state 4's "ChuteSound" playback at landing** apply to dropped infantry too, or is it Rocketeer-only? State 4 plays `Rules+0x48` (ChuteSound? or something else) during landing finalize — but only Rocketeer-class units run state 4. Dropped infantry, having no JumpjetLoco instance, never run state 4. So the landing thump for paradropped units must come from elsewhere (probably `Drop_Payload` directly, which we've already verified plays `VocClass::PlayAt(0)` with `Rules.ChuteSound = Rules+0x71C`).

6. **The IPiggyback path** (Rocketeer → IFV morph) is fully separate from the paradrop path. They share the JumpjetLocomotion class but use it differently:
   - Rocketeer: locomotor is the **primary** locomotor (constructed once at unit creation, never released).
   - IFV pickup: locomotor is **piggybacked** onto the IFV's DriveLocomotion via IPiggyback.
   - Paradropped infantry: **no JumpjetLocomotion instance involved at all**.

## R3.8 — Implications for Rust implementation

For our parachute implementation, the discovery that **paradropped infantry don't actually use JumpjetLocomotion** simplifies things considerably:

- ❌ Don't model "ParachuteLocomotion" as a piggyback override on the infantry's locomotor.
- ✅ Model parachute as a **per-entity altitude integrator** living on the InfantryClass / ObjectClass layer, not on the locomotor layer.
- ✅ The descent rate is whatever value gamemd actually applies — possibly NOT `Rules.ParachuteMaxFallRate=-3` (need to find the real source). Best fallback: use that INI value as a starting point, but flag it for runtime verification against the original.
- ✅ The parachute Anim (Rules+0xBBC = `PARACH`) is attached at `Object+0x88` and rendered above the infantry's standard idle sprite. Detach + delete on landing via `ObjectClass::DetachParachute @ 0x005F6DA0`.
- ⚠️  Per-infantry artmd `Paradrop=N,1,0` lines may be dead for paradrop SW — the visual is the parachute Anim + standard idle pose. Only Rocketeer-class units consume those lines via the FootClass::Locomotion_AI gate. **Verify in-game.**
- ✅ JumpjetLocomotionClass *is* relevant for our project, but only for Rocketeer / Siege Chopper / Hornet implementations (covered by the rest of this report).

## R3.9 — Round 3 confidence summary

| Claim | Confidence | Basis |
|---|---|---|
| ParachuteLocomotion CLSID == JumpjetLocomotion CLSID (single class) | **Verified** | Disassembly of factory + xref of constructor |
| Factory `0x6C4190` calls `JumpjetLocomotionClass::Constructor` at `0x54AC40` | **Verified** | Direct disassembly |
| Drop_Payload does NOT install a Jumpjet/Parachute locomotor | **Verified** | Drop_Payload disassembly post-Unlimbo shows no Begin_Piggyback or locomotor construction |
| State 4 contains no altitude-decrement logic | **Verified** | Re-decompiled state 4 in full; the only altitude reference is the `if (altitude == 0)` finalize gate |
| `instance+0x2C` = CruiseHeight cache | **Verified** | In_Which_Layer dispatch + state 0 copy-to-+0x80 chain |
| `instance+0x80` is multi-purpose / state-overloaded | **Verified** | State 0 sets to CruiseHeight; state 5 sets to -5; state 4 sets to 0 |
| Paradrop sequences (artmd 23/24) are gated by Locomotor CLSID match in FootClass::Locomotion_AI | **Verified** | Decompile of FootClass::Locomotion_AI |
| Rules.ParachuteMaxFallRate has no clear consumer in scan | **Verified** (negative finding) | Byte-pattern scan returned no matches; alternate encodings possible but not found |
| Real descent-rate source for paradropped infantry | **Unknown** | Listed as open question R3.7.1 |
| `FootClass+0x68d` semantics | **Unknown** | Listed as open question R3.7.4 |

---

*Round 3 extension generated 2026-05-05 via Ghidra MCP live decompilation.*

---

# Round 4 Extension — Paradrop descent integrator (2026-05-05)

Scope: **resolves all three open questions raised by Round 3.** This round identifies (a) the actual altitude-decrement function for paradropped (and any non-Jumpjet falling) units, (b) the runtime consumer of `Rules.ParachuteMaxFallRate`, and (c) the real visual-sequence trigger for paradropped infantry. Two of Round 3's major conclusions are **corrected** here.

## R4.1 — Headline corrections to Round 3

Two Round-3 statements turn out to be wrong:

1. **R3 said:** "Sequence 0x17/0x18 (`Paradrop`/`ParadropMoving`) is the gate that fires for paradropped GIs ... but only when their primary locomotor IS JumpjetLoco — which dropped infantry don't have, so the artmd `Paradrop=N,1,0` line is dead for paradrop SW."
   **Truth:** sequences 0x17 (23) and 0x18 (24) are NOT `Paradrop`/`ParadropMoving`. They are `Hover` and `Fly` — the **Rocketeer-only flight poses**. The `Paradrop` sequence has index **0x21 (33)**, and it IS triggered for paradropped infantry — by `InfantryClass::Unlimbo` (overridden vtable[0xE8], at unlabelled `0x00521760`), not by FootClass::Locomotion_AI. So the artmd `Paradrop=N,1,0` line is **alive and consumed** for paradropped GIs.

2. **R3 said:** "`Rules.ParachuteMaxFallRate` (Rules+0x7B8) has no easily-locatable consumer ... possibly TS-legacy dead field."
   **Truth:** it IS read at runtime — at `0x005F3FCB` inside `ObjectClass::AI`, with encoding `8B 89 B8 07 00 00` (`MOV ECX, [ECX+0x7B8]`). Round 3's byte scan checked register variants `8b 88` (EAX-base, ECX-target) and `8b 8e` (ESI-base, ECX-target) but missed `8b 89` (ECX-base, ECX-target). The `g_RulesClass_Instance` global pointer lives at absolute `0x008871E0`; the compiler emits `MOV ECX,[0x008871E0]; MOV ECX,[ECX+0x7B8]`.

The corrected mechanism is documented in the rest of this round.

## R4.2 — Sequence pointer table at `0x008555C8`

The Sequence-name → enum-index mapping comes from a 32-bit pointer array (sequence-name strings) at base `0x008555C8`. Stride = 4. Indices verified by reading the table backward from the only labelled string xref (`Paradrop` at `0x008256D8`, table slot at `0x0082564C`, index = `(0x0082564C - 0x008555C8)/4 = 33`). Each entry is just a `const char *` to a static string in `.rdata`; the strings occupy three different sub-regions (`0x008256xx`, `0x0081BAF0`, `0x0081DBBC`, `0x00816E44`) so any earlier "follow the strings region" approach would have missed them. The full enum confirmed by table walk:

| Index | Name | Notes |
|---|---|---|
| 0 | `Ready` | |
| 1 | `Guard` | string at `0x00816E44` (separate region) |
| 2 | `Prone` | |
| 3 | `Walk` | |
| 4 | `FireUp` | |
| 5 | `Down` | |
| 6 | `Crawl` | |
| 7 | `Up` | |
| 8 | `FireProne` | |
| 9 | `Idle1` | |
| 10 | `Idle2` | |
| 11 | `Die1` | |
| 12 | `Die2` | |
| 13 | `Die3` | |
| 14 | `Die4` | |
| 15 | `Die5` | |
| 16 | `Tread` | |
| 17 | `Swim` | |
| 18 | `WetIdle1` | |
| 19 | `WetIdle2` | |
| 20 | `WetDie1` | |
| 21 | `WetDie2` | |
| 22 | `WetAttack` | |
| **23** | **`Hover`** | string at `0x0081DBBC` — **NOT `Paradrop`** |
| **24** | **`Fly`** | string at `0x0081BAF0` — **NOT `ParadropMoving`** |
| 25 | `Tumble` | |
| 26 | `FireFly` | |
| 27 | `Deploy` | |
| 28 | `Deployed` | |
| 29 | `DeployedFire` | |
| 30 | `DeployedIdle` | |
| 31 | `Undeploy` | |
| 32 | `Cheer` | |
| **33** | **`Paradrop`** | the sequence used during paradrop descent |
| 34 | `AirDeathStart` | |
| 35 | `AirDeathFalling` | |
| 36 | `AirDeathFinish` | |

**Implication:** the artmd Sequence keys `Hover=…`, `Fly=…`, `Paradrop=…`, `Tumble=…`, `Cheer=…` etc. all map to specific indices in this enum. Rocketeer's per-unit Sequence definition (e.g., `[ROCKSequence] Hover=…, Fly=…`) is dispatched via FootClass::Locomotion_AI's CLSID gate (R3.3) into indices 23/24. Standard infantry's per-unit Sequence (e.g., `[GISequence] Paradrop=…`) is dispatched via InfantryClass::Unlimbo into index 33. **Two completely separate visual-trigger paths** that share no code — the prior R3 conflation was wrong.

## R4.3 — InfantryClass::Unlimbo at `0x00521760` (unlabelled)

The vtable[0xE8] slot of InfantryClass (vtable base `0x007EB058`, slot `0x007EB140` confirmed = `0x00521760`) holds the function that Drop_Payload actually calls for an Infantry passenger. Ghidra never recognised it as a function; the body lives between `0x00521760` and `0x005217B5`. Disassembly (annotated):

```
00521760: MOV  EAX, [ESP+4]              ; arg = drop coord (CoordStruct*)
00521764: PUSH ESI
00521765: MOV  ESI, ECX                  ; this = Infantry
00521767: PUSH EAX                       ; push coord
00521768: CALL 0x005f5940                ; ObjectClass::Unlimbo (parachute version, R4.5)
0052176d: TEST AL, AL                    ; success?
0052176f: JZ   0x005217b4                ; on fail → return AL=0
00521771: MOV  ECX, [ESI+0x21C]          ; some Infantry sub-object pointer
00521777: CALL <FUN_0050AD30>            ; FUN_50AD30 (not labelled — likely ZoneClass::Find_Spot)
0052177c: TEST AL, AL
0052177e: JZ   0x00521790                ; ZoneClass returned 0 → mission 15 path
00521780: MOV  EDX, [ESI]                ; vtable
00521782: PUSH 0
00521784: PUSH 5                         ; mission 5
00521786: MOV  ECX, ESI
00521788: CALL [EDX+0x1E8]               ; vtable[0x1E8] = Assign_Mission(5, 0)
0052178e: JMP  0x0052179e
00521790: MOV  EAX, [ESI]
00521792: PUSH 0
00521794: PUSH 0xF                       ; mission 15
00521796: MOV  ECX, ESI
00521798: CALL [EAX+0x1E8]               ; vtable[0x1E8] = Assign_Mission(0xF, 0)
0052179e: MOV  EDX, [ESI]                ; vtable
005217a0: PUSH 0                         ; arg3 = 0
005217a2: PUSH 1                         ; arg2 = 1
005217a4: PUSH 0x21                      ; arg1 = 33 = Paradrop sequence
005217a6: MOV  ECX, ESI
005217a8: CALL [EDX+0x558]               ; vtable[0x558] = DoType(33, 1, 0)
005217ae: MOV  AL, 1
005217b0: POP  ESI
005217b1: RET  4
005217b4: MOV  AL, 1                     ; (also success path? — ret with AL=1 even on inner-fail; but the CALL above set AL to 0... bug or intentional?)
005217b6: POP  ESI
005217b7: RET  4
```

**Wait — the bottom of the function returns `AL=1` UNCONDITIONALLY** (the JZ at 0x52176F goes to a label that also sets `AL=1`). So Drop_Payload sees success even if the inner ObjectClass::Unlimbo failed. That is a real engine quirk — the parachute Unlimbo's success bit is effectively ignored at the InfantryClass level, the Infantry stays unlimbo'd at the original coord regardless. **Tiny detail worth preserving for parity:** the engine's behaviour is "always succeed" at this level.

(Edge interpretation: the JZ-target at 0x5217B4 only does `MOV AL,1; POP; RET 4`. So inner-fail still returns success. This means a paradrop that "should have failed" — e.g., onto an unpassable cell — still gets reported back to Drop_Payload as success. Drop_Payload then proceeds with VocClass::PlayAt(ChuteSound), sets aircraft+0x6D3 to LandingState=5, etc. The Infantry simply ends up at its (pre-unlimbo) origin. This explains why botched paradrops in gamemd never produce a "drop failed" code path — they silently self-recover. )

The three calls done after a successful inner-Unlimbo:
- **`vtable[0x1E8]`** = `Assign_Mission(idx, force)` — mission 5 = `Mission_Move` (when ZoneClass found a target spot) OR mission 15 = `Mission_Guard_Area` (fallback).
- **`vtable[0x558]`** = `DoType(seq, force, restart)` — sets the Infantry's Doing field at `Infantry+0x6C4` to **33 (= `Paradrop` sequence per R4.2)**.

The `MOV ECX,[ESI+0x21C]` before the `FUN_0050AD30` call uses the Infantry's `+0x21C` = ZoneClass-or-similar pointer (zone manager); the result decides which fallback mission to assign. This is the only branch in the function and only gates the mission, not the sequence.

## R4.4 — `ObjectClass::AI` at `0x005F3E70` — the descent integrator

This is the per-tick function that bleeds altitude for any in-air Object (paradropped infantry, freefall units, vehicles thrown by IronCurtain, AirDeath aircraft, etc.). It runs from `TechnoClass::AI_Update` (which any Techno's per-tick chain goes through). Annotated disassembly of the descent block (from `0x005F3F11` through `0x005F3FFA`):

```
; ── early-out: if not InAir, skip the descent block entirely ──
005f3f11: MOV  AL, [ESI+0x8D]            ; +0x8D = InAir flag
005f3f17: TEST AL, AL
005f3f19: JZ   0x005f4151                ; not InAir → skip

; ── compute new Z ──
005f3f1f: MOV  EAX, [ESI]
005f3f23: CALL [EAX+0x78]                ; vtable[0x78] = Get_Layer  (cache "old layer" in EBP)
005f3f2a: MOV  EBP, EAX
005f3f2c: CALL [EDX+0x1D0]               ; vtable[0x1D0] = some "GetZ" virtual; returns int
005f3f32: MOV  ECX, [ESI+0x2C]           ; +0x2C = current descent rate (signed int leptons/tick)
005f3f35: MOV  EDI, EAX                  ; EDI = base Z from vtable[0x1D0]
005f3f37: MOV  AL,  [ESI+0x74]           ; +0x74 = "marks ground cells" flag
005f3f3a: ADD  EDI, ECX                  ; EDI = base + rate (rate is negative → Z decreases)
005f3f3c: TEST AL, AL
005f3f3e: JZ   0x005f3f60                ; if not marking → write Z directly

; ── if currently marking ground cells, unmark/remark around the Z write ──
005f3f40: CALL [EAX+0x124]               ; vtable[0x124](0)  — Mark(MARK_UP) = unmark
005f3f4e: PUSH 1
005f3f52: MOV  [ESI+0xA4], EDI           ; +0xA4 = Z coord — store new value
005f3f58: CALL [EDX+0x124]               ; vtable[0x124](1)  — Mark(MARK_DOWN) = remark
005f3f5e: JMP  0x005f3f66

005f3f60: MOV  [ESI+0xA4], EDI           ; (no marking) write Z

; ── landing detection ──
005f3f66: MOV  EAX, [ESI]
005f3f6a: CALL [EAX+0x1C8]               ; vtable[0x1C8] = Get_Height (altitude above ground)
005f3f70: TEST EAX, EAX
005f3f72: JG   0x005f3fa4                ; altitude > 0 → not landed yet
005f3f7a: CALL [EDX+0x1CC]               ; vtable[0x1CC] = Set_Height(0) — clamp altitude to 0
005f3f86: MOV  byte ptr [ESI+0x8D], 0    ; clear InAir
005f3f8d: CALL [EAX+0x18C]               ; vtable[0x18C](2) = post-land hook (arg 2 = ground layer index)
005f3f93: MOV  EAX, [ESI+0x88]           ; +0x88 = attached parachute Anim pointer
005f3f99: TEST EAX, EAX
005f3f9b: JZ   0x005f3fa4
005f3f9d: MOV  byte ptr [EAX+0x195], 0   ; AnimClass+0x195 = 0 → tells anim to wind down (R4.6)

; ── update descent rate (chooses parachute mode vs free fall) ──
005f3fa4: MOV  AL, [ESI+0x81]            ; +0x81 = "skip-rate-update" flag (set when re-entering layer)
005f3faa: TEST AL, AL
005f3fac: JNZ  0x005f4146                ; if set → DisplayClass::RemoveFromLayer + return

005f3fb2: MOV  AL, [ESI+0x84]            ; +0x84 = "has attached anim" flag (R4.5) = parachute mode
005f3fb8: TEST AL, AL
005f3fba: JZ   0x005f3fd7                ; +0x84 == 0 → free-fall branch

; ── parachute branch: integer accel = -1/tick, clamp to Rules.ParachuteMaxFallRate ──
005f3fbc: MOV  EDI, [ESI+0x2C]           ; current rate
005f3fbf: DEC  EDI                       ; rate -= 1
005f3fc0: MOV  [ESI+0x2C], EDI           ; store back
005f3fc3: MOV  ECX, [0x008871E0]         ; ECX = g_RulesClass_Instance
005f3fc9: MOV  EAX, EDI
005f3fcb: MOV  ECX, [ECX+0x7B8]          ; ★ ECX = Rules.ParachuteMaxFallRate (-3 default)
005f3fd1: CMP  EAX, ECX
005f3fd3: JG   0x005f3ffa                ; if rate > MaxFallRate → keep (no clamp)
005f3fd5: JMP  0x005f3ff8                ; else → clamp to MaxFallRate

; ── free-fall branch: float accel = -1.4/tick, clamp to Rules.NoParachuteMaxFallRate ──
005f3fd7: FILD dword ptr [ESI+0x2C]      ; rate → FPU stack
005f3fda: FSUB qword ptr [0x007EF248]    ; rate -= 1.4   (constant at 0x7EF248 = double 1.4)
005f3fe0: CALL 0x007C5F00                ; Math__ftol → EAX = (int)rate
005f3fe5: MOV  [ESI+0x2C], EAX           ; store back
005f3fe8: MOV  EDX, [0x008871E0]
005f3fee: MOV  ECX, [EDX+0x7BC]          ; ★ ECX = Rules.NoParachuteMaxFallRate (-100 default)
005f3ff4: CMP  EAX, ECX
005f3ff6: JG   0x005f3ffa
005f3ff8: MOV  EAX, ECX                  ; clamp to MaxFallRate
005f3ffa: MOV  [ESI+0x2C], EAX           ; final rate
```

After this block, vtable[0x78] = Get_Layer is called again and compared to the cached EBP — if the layer changed, `DisplayClass::Submit_Object` is invoked to re-add the unit to the new layer. Then a final tail check at `0x005F4062` handles the **Bouncer-on-water** case (RTTI=4 + AnimType+0x354 — Bullet/Meteor specific, **unrelated to parachute**).

### Field map confirmed by this disassembly

| Object offset | Type | Purpose | Verified by |
|---|---|---|---|
| `+0x2C` | int (signed) | **Current descent rate (leptons/tick)** — accumulated, clamped to `Rules.ParachuteMaxFallRate` or `Rules.NoParachuteMaxFallRate` | All reads/writes in this block |
| `+0x74` | byte | "Marks ground cells" gate — non-zero triggers `vtable[0x124](0)/(1)` (Mark UP/DOWN) wrap around the Z write. For paradrop infantry mid-air this is 0 (no occupancy), so no mark wrap | `MOV AL,[ESI+0x74]` + branch |
| `+0x81` | byte | "Skip rate-update" flag — when set, function does `DisplayClass::RemoveFromLayer + return` after the Z write. Used by some layer-transition paths | `MOV AL,[ESI+0x81]` |
| **`+0x84`** | byte | **"Has attached Anim" flag = parachute-fall-mode gate** — set to 1 by `AnimClass::SetOwnerObject` when an anim is attached, cleared to 0 when the LAST attached anim detaches. Doubles as the parachute-vs-freefall toggle in this function | `MOV AL,[ESI+0x84]` + R4.5 |
| `+0x88` | ptr | Attached parachute Anim pointer (cleared by `ObjectClass::DetachParachute` when anim destructs) | `MOV EAX,[ESI+0x88]` |
| `+0x8D` | byte | InAir flag (set by parachute Unlimbo, cleared on landing) | early-out + landing-write |
| `+0xA4` | int | **Z coord** (absolute) | `MOV [ESI+0xA4], EDI` (Z write) |

### Vtable slot map (from this function)

| Slot offset | Inferred name | Purpose |
|---|---|---|
| `+0x78` | `Get_Layer` | Returns current draw-layer index; called twice (before + after Z write) for change detection |
| `+0x124` | `Mark` | `Mark(0)` = unmark cells; `Mark(1)` = mark cells. Called as wrap when `+0x74 != 0` |
| `+0x18C` | `Layer-change-on-land` | Called with arg `2` after landing — likely re-registers Object on ground layer |
| `+0x1C8` | `Get_Height` | Returns altitude above ground (already documented in R3, layer logic) |
| `+0x1CC` | `Set_Height` | Clamps altitude; called with `0` on landing |
| `+0x1D0` | `Get_Z` (or `Get_Z_With_Bridge`) | Returns the Object's Z value used as the base for `newZ = base + rate`. For paradropped infantry this returns the current Z, so the integration is effectively `Z += rate` per tick |

### Magic constants

| Address | Value | Purpose |
|---|---|---|
| `0x008871E0` | g_RulesClass_Instance | global pointer to RulesClass; loaded twice (`MOV ECX,[0x8871E0]` and `MOV EDX,[0x8871E0]`) inside this function |
| `0x007EF248` | double `1.4` (raw bytes `66 66 66 66 66 66 F6 3F` = `0x3FF6666666666666`) | Free-fall acceleration magnitude (leptons/tick²). Subtracted from rate per tick when in free fall (NOT parachute mode) |

## R4.5 — `AnimClass::SetOwnerObject` at `0x00424B50` flips Object+0x84

The parachute-mode gate at `Object+0x84` is **not** a TS-era "IsParachuted" flag, and is **not** set by Drop_Payload or by the spawner. It is set by `AnimClass::SetOwnerObject` whenever ANY anim is attached to the Object, and cleared when the LAST anim detaches:

```
00424b50: MOV  EBP, [ESI+0xCC]           ; ESI = anim, EBP = current owner
...
00424bb6: MOV  byte ptr [EBP+0x84], 0    ; ★ clear old_owner+0x84 — only if no other anim
                                          ; shares this owner (pre-loop counts shared anims)
...
00424c30: MOV  byte ptr [EDI+0x84], 1    ; ★ set new_owner+0x84 = 1 (EDI = param_2 = new owner)
00424c37: MOV  [ESI+0xCC], EDI           ; anim+0xCC = owner backref
```

**Implications:**

- `Object+0x84 = "has at least one attached AnimClass"`. The loop at `0x00424B7F..0x00424BA9` walks `g_AnimClass_Array` to count anims sharing this owner; only when zero remain does it clear the byte.
- The fall-mode gate in ObjectClass::AI (R4.4) reads this byte. So **as long as ANY anim is attached** (not just a parachute) the unit falls in "parachute mode". For non-paradrop scenarios this is moot — the `+0x8D` InAir gate filters first — but in principle, attaching any anim to a falling unit would reduce its descent rate. (Probably not exploited by any vanilla content.)
- The "last-anim removal" must happen when the parachute Anim destructs, otherwise `+0x84` stays at 1 after landing and any subsequent free-fall would erroneously start in parachute mode. Anim destruction triggers `SetOwnerObject(NULL)` (the detach branch at top of the function), which decrements the share-count and clears `+0x84` once empty.

The PARADROP report's `Object+0x3D4` byte (called "IsParachuted" by the spawner) is a **different** field, set at spawn time in `FUN_0065E660` via `*(undefined1 *)(piVar3 + 0xf5) = 1` (where `piVar3` is `int*`, so byte offset = `0xF5 * 4 = 0x3D4`). Round-3 conflated the two; they are unrelated.

`TechnoClass::Unlimbo` at `0x006F6CA0` separately writes a third nearby byte: `*(undefined1 *)((int)param_1 + 0x3D5) = MapClass::Is_Cell_In_Playfield(coord)` — flag for "spawned inside playfield". Three independent flags in three adjacent bytes (`+0x3D4`, `+0x3D5`, `+0x3D6` likely).

## R4.6 — Anim `+0x195` "force-end" mechanism

When ObjectClass::AI detects landing (altitude ≤ 0), it does (line `0x005F3F9D`):
```
MOV byte ptr [EAX+0x195], 0     ; EAX = parachute Anim pointer
```

This zeroes the AnimClass `+0x195` byte. From `AnimClass::AI` (`0x00423AC0`), `+0x195` is the "remaining loops" counter — initialised at anim-start from `AnimType+0x2C4` and decremented per loop completion. Setting it to 0 forces the anim to enter terminate path on its next per-tick AI step — `AnimClass::AI` at the bottom of the function checks `byte ptr [param_1+0x195]` and only runs continuation logic when non-zero; when it reaches 0, the anim's "Next" anim chain is followed (possibly to none → self-destruct).

So the landing sequence is:
1. ObjectClass::AI writes `Anim+0x195 = 0` and clears Object's `InAir` flag.
2. Next tick, AnimClass::AI sees `+0x195 == 0` → completes the terminate sequence.
3. AnimClass destructor calls broadcast detach `FUN_00710410(this)`.
4. FUN_00710410 (`0x00710410`) walks Object pointer fields and clears matching ones — including the parachute pointer `Object+0x88` via `ObjectClass::DetachParachute` (`0x005F6DA0`).
5. The same destruct path also calls `AnimClass::SetOwnerObject(NULL)` (detach branch), which decrements the share-count on `Object+0x84` and clears it to 0 if no other anim remains.

The Anim's lingering 1-tick lifetime AFTER `InAir = 0` is a tiny but real timing detail — for one tick, the unit has `InAir == 0` and `Object+0x88 != NULL` simultaneously (the parachute is still rendered briefly above the now-grounded infantry). Worth preserving for parity.

## R4.7 — Descent-rate timeline (tick-by-tick)

Putting the integrator together for a paradropped GI dropped from drop altitude `Z₀`:

| Tick | rate (`+0x2C`) before | Z (`+0xA4`) before | rate after DEC | Z after | clamp action |
|---|---|---|---|---|---|
| 0 (Unlimbo) | 0 | Z₀ (= aircraft altitude) | — | Z₀ | — |
| 1 | 0 | Z₀ | -1 | Z₀ + 0 = Z₀ | -3 < -1 → keep (no clamp) |
| 2 | -1 | Z₀ | -2 | Z₀ - 1 | -3 < -2 → keep |
| 3 | -2 | Z₀ - 1 | -3 | Z₀ - 3 | -3 < -3 false → clamp to -3 |
| 4 | -3 | Z₀ - 3 | -4 → clamp -3 | Z₀ - 6 | -3 < -4 false → clamp to -3 |
| n (n≥4) | -3 | Z₀ - 3·(n-3) | -3 (steady) | Z₀ - 3·(n-2) | clamped each tick |

The integrator's **base** (`vtable[0x1D0]` return) is added to `rate`, so `Z_new = Z_base + rate`. The first integrator tick (tick 1) does **not move** the unit at all (rate is still 0 from the prior frame). Tick 2 moves it by -1 lepton; tick 3 by -2; tick 4 onward by -3. **Total descent over the first 4 ticks: 0 + 1 + 2 + 3 = 6 leptons (one-twelfth of a cell).** Steady state thereafter: 3 leptons/tick = 1/64 of a cell/tick = 1 cell every ~21.3 ticks (~1.4 seconds at 15 fps).

**Edge case — re-paradrop within a session:** the `+0x2C` rate field is **never explicitly reset to 0**. Once it's been clamped to -3 and the Infantry lands (InAir=0), `+0x2C` retains -3. If the same Infantry re-enters in-air state later (e.g., Iron Curtain throw, or hypothetically re-paradrop), it falls at -3 immediately with no ramp-up. (For paradrop SW this only matters for re-used cargo Infantry, which doesn't happen in practice — but it's a real engine behavior.)

**Edge case — infantry has any other attached anim during fall:** if the unit happens to have `Object+0x88` (or +0x12C/+0x1D4/+0x2C8/+0x130) populated by any anim attached via `SetOwnerObject` before falling, `+0x84 == 1` → it falls at parachute rate (-3) regardless. For freefall (e.g., aircraft destroyed mid-air with no chute), `+0x84 == 0` and the fall accelerates by 1.4 leptons/tick² toward NoParachuteMaxFallRate.

**Edge case — `Math__ftol` rounding (free-fall):** the free-fall path computes `rate = (int)((float)rate - 1.4)`. After 1 tick: `(int)(0 - 1.4) = -1`. After 2: `(int)(-1 - 1.4) = -2` (truncation toward zero, but for negatives this rounds toward -inf in MSVC's standard ftol, so result is -3). Need to verify the exact rounding mode of `Math__ftol` (`0x007C5F00`). Standard MSVC `_ftol2` rounds toward zero on x86 with default control word, but RA2 may have set a different mode. **For parity, treat free-fall accel as `(int)trunc(rate - 1.4)` until verified otherwise.**

## R4.8 — Visual sequence for paradropped GIs (Q3 answer)

Re-check of `InfantryClass::DoType_Sequencer` at `0x00520AE0`:

```c
case 0x21:
  break;                                  // hold the Paradrop sequence — no per-tick transition
...
// after the switch:
if (param_1[0x1B1] == 0x21 && (char)param_1[0x8D] == 0) {
  // currently in Paradrop sequence AND InAir = 0 → reset to Ready
  vtable[0x558](0, 1, 0);                 // DoType(0 = Ready, force, restart)
}
```

`Infantry+0x6C4` (`param_1[0x1B1]`) is the Doing field. So:
1. InfantryClass::Unlimbo (R4.3) sets Doing = 33 (`Paradrop`).
2. Each tick during descent, DoType_Sequencer hits case 33 → no-op (anim continues frame-stepping per the `param_1[0x3E] < sequence_length` early-skip-to-end-of-function).
3. When ObjectClass::AI lands (InAir → 0), the next call to DoType_Sequencer fires the post-switch fallback → DoType(0 = Ready).

**The `Paradrop` artmd sequence (per-unit `Paradrop=N,1,0`) IS rendered during paradrop descent. The infantry sprite plays its parachute frames; the parachute Anim (Rules+0xBBC = PARACH) renders above as overlay. Both are active. The R3 hypothesis that "the artmd Paradrop= line is unused for paradrop SW" is wrong.**

## R4.9 — Call chain summary (Drop_Payload → descent → land)

```
AircraftClass::Drop_Payload @ 0x00415C60
  └─ passenger.vtable[0xE8](drop_coord)
        = InfantryClass::Unlimbo @ 0x00521760 (unlabelled, vtable[0xE8] of InfantryClass at 0x007EB058)
        ├─ ObjectClass::Unlimbo (parachute) @ 0x005F5940
        │   ├─ Sets InAir flag: byte [Infantry+0x8D] = 1
        │   ├─ vtable[0xD8](coord, 0x80) = Unlimbo_LowLevel — places on cell, returns success
        │   ├─ vtable[0x1B4](coord) = Set_Coord (Z = aircraft altitude from drop_coord.Z)
        │   ├─ if vtable[0x2C]() == 8 (Bullet RTTI) → AnimClass::Constructor(Rules+0xBB8 = BombParachute)
        │   │   else → AnimClass::Constructor(Rules+0xBBC = Parachute = "PARACH")
        │   ├─ AnimClass::SetOwnerObject(Infantry) @ 0x00424B50
        │   │   └─ sets [Infantry+0x84] = 1 (parachute fall mode enabled)
        │   ├─ Infantry+0x88 = ParachuteAnim*
        │   ├─ ParachuteAnim+0xD4 = vtable[0x1E4]() (some color/owner field)
        │   └─ ParachuteAnim+0xFC = vtable[0x1BC]()→[+0x10A]   (some short id)
        ├─ FUN_0050AD30(Infantry+0x21C)  — find dest spot
        │   └─ on success → vtable[0x1E8](5, 0) = Assign_Mission(Move)
        │      on fail   → vtable[0x1E8](0xF, 0) = Assign_Mission(Guard_Area)
        └─ vtable[0x558](0x21, 1, 0) = DoType(33 = "Paradrop", force, restart)
              └─ Infantry+0x6C4 = 33 (Doing = Paradrop)

per tick from TechnoClass::AI_Update → ObjectClass::AI @ 0x005F3E70:
  if (Infantry+0x8D /* InAir */) {
    Z = vtable[0x1D0]() + Infantry+0x2C        (Infantry+0xA4 = Z)
    if (vtable[0x1C8]() /* GetHeight */ <= 0) {
      vtable[0x1CC](0)                          (SetHeight 0)
      Infantry+0x8D = 0                         (InAir off)
      vtable[0x18C](2)                          (post-land hook)
      ParachuteAnim+0x195 = 0                   (force anim end)
    }
    if (Infantry+0x84 /* parachute mode */) {
      Infantry+0x2C--                            (rate -= 1)
      clamp(Infantry+0x2C, Rules.ParachuteMaxFallRate=-3)
    } else {
      Infantry+0x2C = (int)((float)Infantry+0x2C - 1.4)
      clamp(Infantry+0x2C, Rules.NoParachuteMaxFallRate=-100)
    }
  }

per tick from InfantryClass::AI → InfantryClass::DoType_Sequencer @ 0x00520AE0:
  if (Doing == 33) {
    if (frame_counter < sequence_length) hold;   (anim plays normally)
    else break; → post-switch:
    if (Doing == 33 && !InAir) DoType(0 = Ready); (landed → reset)
  }

at landing + 1 tick (from AnimClass::AI):
  ParachuteAnim+0x195 reads as 0 → anim wraps up loops → calls Next chain → self-destructs:
  └─ AnimClass destructor → broadcast detach FUN_00710410 @ 0x00710410:
       └─ ObjectClass::DetachParachute @ 0x005F6DA0  →  Infantry+0x88 = 0
       └─ AnimClass::SetOwnerObject(NULL) → if no other anim attached to Infantry: Infantry+0x84 = 0
```

## R4.10 — Round-3 open questions, resolved

| R3 Question | Resolution |
|---|---|
| **R3.7.1** — Where is altitude decremented for paradropped infantry? | `ObjectClass::AI` at `0x005F3E70`, lines `0x005F3F32`-`0x005F3F60`. Z written at `Object+0xA4` per tick. |
| **R3.7.2** — Is `Rules.ParachuteMaxFallRate` consumed in YR? | **YES.** Read at `0x005F3FCB` (encoding `8B 89 B8 07 00 00`). NoParachuteMaxFallRate also read at `0x005F3FEE`. Round-3 byte scan missed both. |
| **R3.7.3** — How is the Paradrop sequence triggered for paradropped GIs? | By `InfantryClass::Unlimbo` at `0x00521760` calling `vtable[0x558](33, 1, 0)` = `DoType(33 = "Paradrop")`. Held by the case-33 branch in `InfantryClass::DoType_Sequencer`. Reset to Ready on landing. **The artmd `Paradrop=N,1,0` line IS consumed for paradropped GIs** — Round-3 hypothesis was wrong. |
| **R3.7.4** — `FootClass+0x68d` semantics | Still uncertain. From FootClass::Locomotion_AI: it gates the JumpjetLoco-CLSID-matched DoType(Hover/Fly) dispatch. From InfantryClass::AI: it's reset to 0 by the Doing=27/28/29/30/31 (Deploy* / Undeploy / Cheer) handler. Likely a "is currently in special pose" or "skip-locomotion-anim-this-tick" gate. **Not blocking parachute parity.** |
| **R3.7.5** — Does state 4's ChuteSound apply to dropped infantry? | **No.** Confirmed: dropped infantry never run JumpjetLocomotion's state 4. The chute sound for them is played directly by Drop_Payload (`VocClass::PlayAt(0)` post-Unlimbo, with sound resolved from `Rules+0x71C = ChuteSound` per PARADROP report). |
| **R3.7.6** — IPiggyback path completely separate from paradrop path | Confirmed. Paradropped GIs have no `JumpjetLocomotionClass` instance at all; the parachute-mode flag is the AnimClass attachment, not a piggyback locomotor. |

## R4.11 — Tiny details to preserve for parity

Each of these is a thing the engine actually does that a casual implementation would miss. They're listed in priority of "how visibly wrong the implementation will look without it":

1. **3-tick descent ramp.** Rate ramps 0 → -1 → -2 → -3 over the first three integrator ticks (NOT instant -3). Implementer must initialise rate at 0 and decrement per tick.
2. **Parachute Anim renders for one tick after InAir clears.** Landing flow is: ObjectClass::AI clears InAir + flags Anim for end → next tick AnimClass::AI runs the wind-down → anim destructor fires DetachParachute. So there's a 1-tick window where InAir==0 but the chute is still visually attached. Don't synchronously remove the parachute on landing.
3. **InfantryClass::Unlimbo always returns success** (AL=1) regardless of inner ObjectClass::Unlimbo result. Drop_Payload sees success even if cell-place failed. Implementer who returns the actual success bit will diverge from gamemd's "silent recovery" behaviour.
4. **DoType(Paradrop=33) is set at Unlimbo time, NOT in DoType_Sequencer.** The Sequencer only HOLDS the sequence (case 33 → no-op) and resets it on landing. The trigger is inside the Unlimbo path.
5. **Free-fall accel = double 1.4** (not 1, not 2). Constant at `0x007EF248`. Truncate-toward-zero or truncate-toward-negative-infinity? — needs `Math__ftol` rounding-mode verification before locking parity.
6. **Mark/unmark wrap on Z write only when `Object+0x74 != 0`.** Paradrop infantry mid-air has `+0x74 == 0` so no mark wrap. Once landed, marking re-engages. Don't unconditionally call Mark/Unmark.
7. **Two distinct fields share neighborhood and confused prior reports:**
   - `Object+0x84` = "has attached anim" / parachute fall mode (set by AnimClass::SetOwnerObject)
   - `Object+0x3D4` = "IsParachuted" historical-name flag (set by spawner, used elsewhere)
   - `Object+0x3D5` = "Is_Cell_In_Playfield" (set by TechnoClass::Unlimbo)
   - `Object+0x8D` = InAir flag (set by parachute Unlimbo, gates ObjectClass::AI descent block)
   None of `+0x3D4`, `+0x3D5` are involved in altitude integration. Don't conflate.
8. **vtable[0x1D0] used as the Z base** (not direct read of `Object+0xA4`). For Infantry these are equivalent, but for transported units or units on bridges, vtable[0x1D0] may return Z with bridge adjustment. Implement as a virtual call to preserve correctness for non-Infantry callers.
9. **Sequence indices 23/24 = `Hover`/`Fly` (Rocketeer-only), NOT Paradrop.** Sequence 33 = `Paradrop`. Distinct sequences with similar concepts. Implementer who reads only the Round-3 finding will pick the wrong indices for a Rocketeer-style flier vs paradropped GI.
10. **g_RulesClass_Instance is a pointer global at `0x008871E0`** (not the RulesClass instance directly). Two indirections to read `Rules.ParachuteMaxFallRate`: `MOV reg,[0x8871E0]; MOV reg,[reg+0x7B8]`. This is why a byte-pattern scan for `MOV reg,[base+0x7B8]` only catches the SECOND instruction — and the relevant one happens to use ECX-base / ECX-target ModR/M (`0x89`), which Round 3's scan didn't try.
11. **Bouncer-on-water tail block at `0x005F4062`** — totally unrelated to parachute. Bullet RTTI (`vtable[0x2C]() == 4`) + AnimType+0x354 (Bouncer flag) triggers spawn of `Rules+0xBC4` (water-splash anim chain). Preserve this for Meteor-style bullet behaviour, but it is NOT part of the parachute system.
12. **Object+0x84 is shared between "any anim attached" and "parachute fall mode"** — the engine doesn't distinguish. Attaching ANY anim makes a falling unit fall slowly. (Unlikely to be exploited, but technically true.)

## R4.12 — Round 4 confidence summary

| Claim | Confidence | Basis |
|---|---|---|
| ObjectClass::AI at `0x005F3E70` is the descent integrator | **Verified** | Direct disassembly of altitude-write block |
| Descent rate field at Object+0x2C, Z at Object+0xA4 | **Verified** | Disassembly + cross-check with PARADROP report's coord layout |
| Rules.ParachuteMaxFallRate (Rules+0x7B8) read at 0x005F3FCB | **Verified** | Disassembly: `8B 89 B8 07 00 00` |
| Rules.NoParachuteMaxFallRate (Rules+0x7BC) read at 0x005F3FEE | **Verified** | Disassembly: `8B 8A BC 07 00 00` |
| Free-fall accel constant = 1.4 (double) at 0x007EF248 | **Verified** | Memory read: `66 66 66 66 66 66 F6 3F` = `0x3FF6666666666666` = 1.4 |
| g_RulesClass_Instance global pointer at 0x008871E0 | **Verified** | `MOV ECX,[0x008871E0]` in disassembly |
| Object+0x84 = parachute fall-mode (set by AnimClass::SetOwnerObject) | **Verified** | Disassembly of SetOwnerObject at 0x424BB6 / 0x424C30 |
| Sequence 33 = "Paradrop" string at 0x008256D8 | **Verified** | Pointer table walk + string read |
| Sequence 23 = "Hover", Sequence 24 = "Fly" | **Verified** | String reads at 0x0081DBBC / 0x0081BAF0 |
| InfantryClass vtable at 0x007EB058 | **Verified** | InfantryClass::Constructor disassembly |
| InfantryClass::Unlimbo at 0x521760 (vtable[0xE8]) | **Verified** | vtable read at 0x007EB140 = 0x00521760 |
| DoType(0x21=33) called at 0x521760 end | **Verified** | Disassembly + DoType_Sequencer case 33 hold logic |
| InfantryClass::Unlimbo always returns AL=1 | **Verified** | Both branches at 0x5217AE / 0x5217B4 set AL=1 |
| Anim+0x195 = 0 forces anim termination | **High** (Inferred from AnimClass::AI structure) | Read of +0x195 in AI loop body; landing-side write at 0x5F3F9D |
| FootClass+0x68d semantics | **Low** (still uncertain) | Multiple unrelated reads; not blocking |
| `Math__ftol` rounding mode (truncate vs nearest) | **Low** | Not verified — need to disassemble 0x007C5F00 |

---

*Round 4 extension generated 2026-05-05 via Ghidra MCP live decompilation.*

