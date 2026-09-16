# GATE: Bridge A1 — Deck Height / GetGroundHeight Z-init — RESOLUTION

**Verdict:** CLOSED
**Date:** 2026-06-04
**Primary binary:** `gamemd.exe` (Yuri's Revenge 1.001, image base 0x00400000)
**Ghidra MCP: READ-ONLY** — no renames/labels/saves performed.
**Confidence:** HIGH (content + identity + binding). Every fact cites the live MCP call inline.

---

## 0. One-line resolved fact

A unit's Z on a HIGH (structural) bridge is `GetGroundHeight(unit.Coord) + DECK_OFFSET`, where
`DECK_OFFSET` is a **single runtime global `DAT_00AC13BC` (leptons) = 2 × per-level bridge
height `DAT_00AC13C8`** — added on top of the ground Z, not a per-cell value and **not** a
literal `+4`. The `+4` the gate asks about is a *separate, Level-unit* pathfinding seed
(`cell.Level + 4`) used only by `Can_Enter_Cell`/`CheckBridgeTraversal`, never by the Z math.

---

## 1. Confirmed function identities (not relying on labels)

| Address | Identity (verified from body) | Verification call |
|---|---|---|
| `0x00578080` | `CellClass::GetGroundHeight(int* coord)` — X/Y leptons → cell index → delegates ground-Z to `FUN_0047b3a0`. Writes/returns ground Z in **leptons**. | `get_function_by_address 0x00578080`; `decompile_function 0x00578080` |
| `0x0047b3a0` | Inner ground-Z projector (FPU/matrix, theater geometry). NOT the deck-offset arithmetic. | `decompile_function 0x0047b3a0` |
| `0x005F5FA0` | `FootClass::Set_Height_On_Bridge(this, z_offset)` — the canonical "snap unit Z to deck". | `get_function_by_address 0x005F5FA0`; `decompile_function 0x005F5FA0`; `disassemble_function 0x005F5FA0` |
| `0x005F5F60` | `ObjectClass::GetHeight(this)` — inverse: height-above-ground, subtracts deck offset when OnBridge. | `decompile_function 0x005F5F60`; `disassemble_function 0x005F5F60` |
| `0x005F6A80` | `ObjectClass::ShouldBeOnBridge` — deck-snap decision, threshold `DAT_00AC13C8 * 3`. | `decompile_function 0x005F6A80`; `disassemble_function 0x005F6A80` |
| `0x005F6B40` / `0x005F6B80` | `IsLowFlying` / wrappers — full deck height `DAT_00AC13C8 * 2`. | `decompile_function 0x005F6B40`, `0x005F6B80` |
| `0x00487D50` | `CellClass::GetEffectiveHeight` — Level-unit `Level + 4` (NOT leptons). | `get_function_by_address 0x00487D50`; `decompile_function 0x00487D50` |

---

## 2. The Z-on-bridge formula — assembly evidence (CLOSED)

`FootClass::Set_Height_On_Bridge @ 0x005F5FA0` (`disassemble_function 0x005F5FA0`):

```asm
005f5fa7  MOV  EDI,[ESP+0x18]          ; EDI = z_offset arg (param_2)
005f5fab  MOV  AL,[ESI+0x8c]           ; this->OnBridge flag
005f5fb3  JZ   0x005f5fbb              ; if not OnBridge, skip
005f5fb5  ADD  EDI,[0x00ac13bc]        ; z_offset += DAT_00AC13BC  (deck offset, leptons)
...
005f5fce  LEA  ECX,[ESI+0x9c]          ; &this->Coord (X@0x9C, Y@0xA0, Z@0xA4)
005f5ff6  CALL 0x00578080             ; EAX = CellClass::GetGroundHeight(Coord)
005f5ffb  ADD  EAX,EDI                ; EAX = ground_Z + z_offset
005f5fff  MOV  [ESI+0xa4],EAX         ; this->Coord.Z = ground_Z + z_offset
```

So: **`Coord.Z = GetGroundHeight(Coord) + z_offset`**, and when already OnBridge the
`z_offset` itself is bumped by `DAT_00AC13BC`. The deck offset lives in **leptons**, same
frame as `GetGroundHeight`'s output.

Inverse cross-check — `ObjectClass::GetHeight @ 0x005F5F60` (`decompile_function 0x005F5F60`):
```c
height = Coord.Z - GetGroundHeight(Coord);
if (OnBridge) height -= DAT_00ac13bc;   // remove the deck offset → height above deck
```
This confirms `DAT_00AC13BC` is exactly the additive deck offset.

---

## 3. What `DAT_00AC13BC` is — derivation (answers (a) and (b))

`get_xrefs_to 0x00AC13BC` → single WRITE at `0x005F3880`. Raw bytes (`read_memory 0x005F3850, 80`)
decode to:
```asm
005f3861  MOV  EAX,[0x00AC13C8]        ; EAX = per-level bridge height (leptons)
005f3866  LEA  ECX,[EAX*4 + 0]         ; ECX = perLevel * 4
005f3871  FILD dword [ESP]             ; float(perLevel * 4)
005f3875  FMUL qword [0x007E1738]      ; × 0.5    (0x007E1738 = 0.5, see below)
005f387b  CALL 0x007C5F00             ; ftol
005f3880  MOV  [0x00AC13BC],EAX        ; DAT_00AC13BC = ftol(perLevel * 4 * 0.5)
```
`read_memory 0x007E1738, 8` = `00 00 00 00 00 00 E0 3F` = IEEE-754 double **0.5**.

**Resolved arithmetic:** `DAT_00AC13BC = perLevel × 4 × 0.5 = perLevel × 2 = 2 × DAT_00AC13C8`.

- **(a)** It is **NOT** `round(src*4)` and **NOT** a per-cell value. The `*4 ... *0.5`
  byte shape is the gamemd idiom for `× 2`; the source `src` is the per-level bridge height
  `DAT_00AC13C8`, so the deck offset = exactly **2 per-level heights**. The only per-cell
  variation in a unit's bridge Z comes from `GetGroundHeight` (terrain), never from the offset.
- **(b)** There is **no `max(4)` clamp and no literal `4`** in the deck-Z path. The `×4`
  byte is an FPU pre-scale that the `×0.5` halves back to `×2`. Confirmed against the
  identical writer idiom for `DAT_0089E864 = 2 × DAT_0089E870` documented in
  `DAT_0089E864_BRIDGE_THRESHOLD_IDENTITY_GHIDRA_REPORT.md §3`.

Corroboration that `DAT_00AC13C8` is the per-level (LevelHeight) lepton step
(`decompile_function 0x005F6B40`, `0x005F6B80`, `disassemble_function 0x005F6A80`):
- `IsHighFlying`: `DAT_00AC13C8 * 2 <= height` → at/above deck top.
- `IsLowFlying`: `height < DAT_00AC13C8 * 2` → below deck top.
- `ShouldBeOnBridge`: bridge-edge ground-Z step detected when `DAT_00AC13C8 * 3 < |groundΔ|`
  (raw `LEA EDX,[EAX+EAX*0x2]` at `0x005F6AF3`).

`DAT_00AC13C8` is cold-0 in the static dump (`read_memory 0x00AC13C8, 4` = `00000000`) —
runtime/theater-initialized, same pattern as `DAT_0089E870` (nominal 104 leptons), so the
deck offset is **nominally 208 leptons = 2 × 104**, matching the AoE bridge threshold family.

---

## 4. The GetGroundHeight operand (answers (c))

`GetGroundHeight @ 0x00578080` (`decompile_function 0x00578080`) takes a `CoordStruct*`
(X@+0, Y@+4 leptons), converts to a cell index `(Y>>8)*0x200 + (X>>8)`, validates against the
cell array, then calls `FUN_0047b3a0` to compute the **ground** Z in leptons for that cell. It
returns ground-only Z — it does **not** itself add any bridge term.

**Operand a HIGH bridge adds on top:** the caller adds `DAT_00AC13BC` (= `2 × DAT_00AC13C8`
leptons) to `GetGroundHeight`'s result. Frame/units: both terms are **world Z in leptons**,
cell-grid frame (X/Y are cell*256+offset leptons). No level/lepton mixing occurs in this path.

---

## 5. The `+4` the gate mentions — separate Level-unit seed (do not conflate)

`CheckBridgeTraversal` (pathfinding, via `Can_Enter_Cell`) uses, for `height == -1` candidates:
```text
if target_cell.flags & 0x100:  local height = target_cell.Level + 4   # LEVEL units
```
(per [BRIDGE_JUMPJET_HEIGHT_MINUS_ONE_RUNTIME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_JUMPJET_HEIGHT_MINUS_ONE_RUNTIME_GHIDRA_REPORT.md) Executive Summary; and
`CellClass::GetEffectiveHeight @ 0x00487D50` = `Level + ((flags>>7)&1)*4`, Level units —
`decompile_function 0x00487D50`). This `+4` is a discrete **terrain Level index** (1
ElevationIncrement), used for layer/occupancy/cliff decisions — **not** the lepton deck Z.
Two parallel representations of the same physical deck: `+4 Levels` (pathfinding) vs
`+DAT_00AC13BC ≈ 208 leptons` (coordinate Z). They never mix in one comparison.

---

## 6. YR-active vs TS-legacy

**Active in YR — all of it.** `Set_Height_On_Bridge` (FootClass vtable +0x1CC),
`GetGroundHeight`, `GetHeight`, `ShouldBeOnBridge`, `IsHigh/LowFlying` are on live infantry/
vehicle bridge-traversal, parachute-landing, and air-layer paths in standard skirmish
(corroborated by `PARACHUTE_LANDING_BRIDGE_LAYER_SELECT...` and the locomotor reports). The
DropPod path that also touches this is TS-DEAD, but the deck-height helpers themselves are
live. No SpecialFlags/FogOfWar/subterranean gating on this Z math.

---

## 7. Rust handoff (unblocks bridge plan P4)

P4 should define **`BRIDGE_DECK_HEIGHT` (the deck Z offset) = `2 × bridge_per_level_height`
in LEPTONS** (nominally `2 × 104 = 208`), NOT a literal `4`, NOT `round(src*4)`, NOT per-cell.
A unit/object Z on a structural bridge must be computed as:

```
unit.Z = ground_height(cell)  +  BRIDGE_DECK_HEIGHT        // when OnBridge
```

where `ground_height` is the terrain Z helper (per-cell, varies with terrain) and
`BRIDGE_DECK_HEIGHT` is the single constant added on top. Keep the **Level-unit `+4`**
separate: it belongs only to pathfinding/`Can_Enter_Cell`/layer-selection
(`Level + 4`, = 1 ElevationIncrement), never to the coordinate-Z computation. If the per-level
height is theater-resolved at runtime in gamemd, P4 should source `bridge_per_level_height`
from the same place the AoE threshold uses (`DAT_0089E870` family) so all three consumers
(deck Z, AoE layer select, fly-height gates) share one value.

---

## 8. Sources (all this-session, read-only)

- `decompile_function`: 0x00578080, 0x0047b3a0, 0x005F5FA0, 0x005F5F60, 0x005F6A80, 0x005F6B40, 0x005F6B80, 0x00487D50
- `disassemble_function`: 0x005F5FA0, 0x005F5F60, 0x005F6A80
- `get_function_by_address`: 0x00578080, 0x0047b3a0, 0x005F5FA0, 0x00487D50, 0x005F3880 (no defined fn → raw bytes)
- `get_xrefs_to`: 0x00AC13BC, 0x00AC13C8
- `read_memory`: 0x005F3850(80), 0x005F37C0(64), 0x007E1738(8)=0.5, 0x00AC13BC(4)=0, 0x00AC13C8(4)=0
- Prior docs cross-checked: [DAT_0089E864_BRIDGE_THRESHOLD_IDENTITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/DAT_0089E864_BRIDGE_THRESHOLD_IDENTITY_GHIDRA_REPORT.md),
  [GETEFFECTIVEHEIGHT_PLUS4_UNIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GETEFFECTIVEHEIGHT_PLUS4_UNIT_GHIDRA_REPORT.md),
  [bridges/04-locomotion-height-tubes/BRIDGE_JUMPJET_HEIGHT_MINUS_ONE_RUNTIME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_JUMPJET_HEIGHT_MINUS_ONE_RUNTIME_GHIDRA_REPORT.md),
  [bridges/04-locomotion-height-tubes/BRIDGE_LOCOMOTOR_WALK_DROPPOD_TELEPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_LOCOMOTOR_WALK_DROPPOD_TELEPORT_GHIDRA_REPORT.md)
