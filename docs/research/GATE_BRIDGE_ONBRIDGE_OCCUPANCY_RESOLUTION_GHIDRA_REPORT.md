# GATE Bridge A2 — OnBridge Occupancy Representation: Resolution Report

**Gate:** Bridge A2 (occupancy-substrate part of the bridge slice).
**Verdict:** **CLOSED** for all three open questions (a), (b), (c).
**Confidence:** High. Every load-bearing fact below is read from the function body and the
exact callsite assembly this session, not from labels.
**Active in YR:** Yes. The verified chain is the normal Walk/Drive locomotor cell-crossing
path; no TS-only gate sits around it. (Low-bridge/TubeClass is a separate mechanism and is
NOT the high-bridge deck-list selector — out of scope here.)

---

## Confirmed function identities (verified this session)

| Address | Identity | Verification |
|---|---|---|
| `0x0047E8A0` | `CellClass::AddContent(this; object, layerByte)` — `__thiscall`, two stack args, `RET 0x8` | decompile_function 0x0047E8A0 |
| `0x0047EA90` | `CellClass::RemoveContent(this; object, layerByte)` — same shape, `RET 0x8` | decompile_function 0x0047EA90 |
| `0x005683C0` | `TechnoClass::EnterCell_AddToMultiCells` | get_function_by_address + disassemble_function 0x005683C0 |
| `0x005687F0` | `TechnoClass::ExitCell_RemoveFromMultiCells` | get_function_by_address + disassemble_function 0x005687F0 |
| `0x0043F180` | TechnoClass cell-mark dispatcher (vtable+0x134): `switch(mode)` add/remove/facing | decompile_function 0x0043F180; get_xrefs_to 0x005683C0 / 0x005687F0 |
| `0x005F5850` | `ObjectClass::Mark` (vtable+0x124 base) → dispatches to vtable+0x134 | decompile_function 0x005F5850 |
| `0x007441B0` | `ObjectClass::Mark_Occupation` (occupancy-bit setter) | decompile_function 0x007441B0 |
| `0x00744210` | `ObjectClass::Clear_Occupation` (occupancy-bit clearer) | decompile_function 0x00744210 |
| `0x0075AEC0` | `WalkLocomotionClass::ProcessMovement` (cell-crossing sequencer) | get_function_by_address + disassemble_function 0x0075AEC0 |
| `0x004B0F20` | `DriveLocomotionClass::Process_Drive_Track` | get_function_by_address 0x004B0F20 |

### CellClass field offsets (verified via get_struct_layout `CellClass`, size 328 = 0x148)

| Offset (dec / hex) | Field | Role |
|---|---|---|
| 228 / `0xE4` | `FirstObject` | ground object-list head |
| 232 / `0xE8` | `AltObject` | bridge/deck object-list head |
| 283 / `0x11B` | `Level` (signed byte) | terrain level used by the `-4` transition predicate |
| 292 / `0x124` | `OccupationFlags` | ground occupancy bitfield |
| 296 / `0x128` | `AltOccupationFlags` | bridge/deck occupancy bitfield |
| 320 / `0x140` | `Flags` | cell flags; `&0x100` = structural bridge |

`ObjectClass+0x30` = next-pointer within the selected cell list (decompile shows `obj[0xc]`
as int* = byte +0x30). `ObjectClass+0x8C` = `OnBridge` byte.

---

## (a) `Object+0x8C` (OnBridge) IS the object-list LAYER selector — CLOSED

`CellClass::AddContent` and `RemoveContent` each take a `char` layer byte as a stack arg and
select `FirstObject` (+0xE4) when the byte is `0`, `AltObject` (+0xE8) when nonzero
(decompile_function 0x0047E8A0 / 0x0047EA90: `if (layerByte == 0) head = this->FirstObject;
else head = this->AltObject;`).

The byte the normal Techno enter/exit helpers push is exactly `[object+0x8C]`. Verified at
the callsites (disassemble_function 0x005683C0 / 0x005687F0):

```
; EnterCell -> AddContent (object in EDI, cell in EAX)
005684b1: MOV DL, byte ptr [EDI + 0x8c]   ; DL = object->OnBridge
005684b9: PUSH EDX                          ; layer byte
005684ba: PUSH EDI                          ; object
005684bb: CALL 0x0047e8a0                   ; CellClass::AddContent

; ExitCell -> RemoveContent (symmetric)
005688e1: MOV DL, byte ptr [EDI + 0x8c]   ; DL = object->OnBridge
005688e9: PUSH EDX
005688ea: PUSH EDI
005688eb: CALL 0x0047ea90                   ; CellClass::RemoveContent
```

So the cell object-list LAYER an occupant inserts into / removes from is selected by
`ObjectClass+0x8C` (OnBridge), sampled at the exact call site (no deferred recompute inside
CellClass). **Confirmed.**

---

## (b) How on-bridge occupancy is stored/counted — CLOSED: TWO separate layers + a separate bitfield

There are **two independent representations**, both per-cell, both two-layered:

1. **Object-list membership = two separate intrusive linked lists per cell.** Ground list head
   `FirstObject` (+0xE4) and bridge/deck list head `AltObject` (+0xE8), linked through
   `Object+0x30`. An occupant is on exactly ONE of the two lists, chosen by its `OnBridge`
   byte (see (a)). This is the "which list" representation. AddContent prepends non-buildings
   to the selected head and appends `WhatAmI()==6` buildings to the selected tail; RemoveContent
   walks only the selected list (decompile 0x0047E8A0 / 0x0047EA90).

2. **Occupancy BITS = two separate bitfields per cell.** Ground `OccupationFlags` (+0x124) and
   bridge/deck `AltOccupationFlags` (+0x128). These carry the vehicle/infantry/placed-object
   reservation bits (e.g. `0x20` = moving-vehicle reservation) that block movement. The bit
   LAYER is selected NOT by OnBridge but by object **Z height vs ground** plus the structural
   bridge flag (decompile_function 0x007441B0):

   ```
   cell    = Get_CellClass_At_Coord(coords)
   groundZ = GetGroundHeight(coords)
   if (groundZ + DAT_00b1d0ac <= coords.Z) && (cell.Flags & 0x100):
       cell.AltOccupationFlags (+0x128) |= 0x20      ; bridge layer
   else:
       cell.OccupationFlags   (+0x124) |= 0x20       ; ground layer
   ```

   `Clear_Occupation` (decompile_function 0x00744210) clears `0x20` by the Z threshold ALONE
   (it does NOT re-check `Flags&0x100`): `if (groundZ + DAT_00b1d0ac <= coords.Z) clear +0x128
   else clear +0x124`. This asymmetry is load-bearing for collapse cleanup (the bridge flag may
   already be gone while the object Z still reflects the deck).

`DAT_00b1d0ac` is a runtime-initialized bridge-Z threshold global; read_memory 0x00b1d0ac
returns `00000000` in the cold image (set at load time, as prior docs note).

**So: occupancy is NOT one flag-per-occupant.** It is (i) a separate per-cell list LAYER
(+0xE4/+0xE8) selected by `OnBridge`, and (ii) a separate per-cell occupancy BITFIELD LAYER
(+0x124/+0x128) selected by Z-height. The two layer selectors are independent and can disagree
at ramp boundaries — that mismatch is a real, verified gamemd behavior, not an approximation.
(Separate, unrelated: the WhatAmI==6 building hidden-occupancy COUNTER at `Cell+0x100`,
incremented/decremented in the EnterCell/ExitCell helpers under the `[Building+0x520]+0x1766`
CanHideThings-style gate — that is a building footprint counter, not the unit object lists.)

---

## (c) Add/remove ORDER on layer transition — CLOSED: remove-old, write-OnBridge, add-new

Verified in the Walk cell-crossing block (disassemble_function 0x0075AEC0, the
`0x0075C117..0x0075C1AE` body). The locomotor calls the object's mark routine `vtable+0x124`
with `0` (remove) then `1` (add), and writes `OnBridge` between them:

```
0075c11a: PUSH 0x0
0075c11e: CALL [EAX + 0x124]          ; Mark(0) -> ExitCell REMOVE from OLD cell  (OnBridge still OLD)
0075c12e: CALL [EDX + 0x1b4]          ; coordinate update -> move to new coord
0075c154: MOVSX EDX,[ESI + 0x11b]     ; src.Level
0075c15b: MOVSX EDI,[EAX + 0x11b]     ; dst.Level
0075c162: SUB EDX,0x4                 ; src.Level - 4
0075c16a: CMP EDI,EDX                 ; dst.Level == src.Level - 4 ?
0075c16e: TEST [EAX+0x140],0x100      ; dst structural bridge ?
0075c179: MOV byte ptr [obj+0x8c],0x1 ; OnBridge = 1  (SET; entering deck)
0075c180/0x188: ...                   ; clear-branch guards (dst lacks 0x100, src has 0x100)
0075c193: MOV byte ptr [obj+0x8c],0x0 ; OnBridge = 0  (CLEAR; stepping off)
0075c1a1: CALL [EDX + 0x1cc]          ; per-cell processing
0075c1aa: PUSH 0x1
0075c1ae: CALL [EAX + 0x124]          ; Mark(1) -> EnterCell ADD to NEW cell  (OnBridge now NEW)
```

The `vtable+0x124` mark routine routes to the cell-list dispatcher `FUN_0043F180`
(decompile_function 0x0043F180): `switch(mode){ case 0: ExitCell_RemoveFromMultiCells(...);
case 1/3: EnterCell_AddToMultiCells(...); case 2: facing/turret }`. Both helpers read
`[object+0x8C]` at their AddContent/RemoveContent callsites (see (a)).

**Therefore the order is exactly:**
1. Remove from OLD cell list using the **old** `OnBridge` (Mark(0) at 0x0075C11E).
2. Update coordinates.
3. Evaluate the transition predicate (`dst.Level == src.Level − 4` && `dst.Flags&0x100` → set;
   `!dst.Flags&0x100` && `src.Flags&0x100` → clear); write the **new** `OnBridge`.
4. Insert into NEW cell list using the **new** `OnBridge` (Mark(1) at 0x0075C1AE).

Old-cell removal observes the pre-transition layer; new-cell insertion observes the
post-transition layer. The set branch has priority and skips the clear write. Destination
bridge-flag alone does NOT set OnBridge (the exact `−4` level relation is required), so
ground→ramp and body→ramp do not change the byte. **Confirmed.** (DriveLocomotionClass
mirrors this; prior `HIGH_BRIDGE_ONBRIDGE_OCCUPANCY_TRANSITION_SEQUENCE` report verified the
drive blocks at `0x004B17CC` remove, `0x004B1830`/`0x004B184A` set/clear, `0x004B25B1` add.)

---

## YR-active vs TS-legacy

All of the above is live in standard YR Walk/Drive movement: `WalkLocomotionClass::Process`
calls `ProcessMovement`; `DriveLocomotionClass::Process` calls `Process_Drive_Track`; no
TS-only flag gates the verified sequence. Low-bridge / TubeClass uses a separate
`TubeIndex`/`LandType==10` gate and is NOT the high-bridge `+0xE8` deck-list selector — do not
fold it into this. The `Cell+0x100` building hidden-occupancy counter is gated by a
CanHideThings-style flag (`[Building+0x520]+0x1766`), default-on for stock buildings, and is a
separate footprint counter, not the unit lists.

---

## Rust handoff (for bridge plan P5 occupancy repr, and P6 collapse which depends on P5)

P5 must model on-bridge occupancy as **two separate per-cell layers selected by the entity's
persistent `on_bridge` byte (NOT the locomotor/path layer)**, and keep the occupancy-BIT layer
selection (height-based) conceptually independent from the object-LIST layer selection
(`on_bridge`-based) — they are allowed to disagree at ramp boundaries. On a cell crossing,
P5 must: (1) remove the occupant from the old cell on the **old** `on_bridge` layer; (2) apply
the transition predicate (`dst.Level == src.Level − 4 && dst bridge` → set; `dst not-bridge &&
src bridge` → clear) to compute the **new** `on_bridge`; (3) insert into the new cell on the
**new** layer. Do not pre-claim the bridge list from the destination bridge flag alone, and do
not use the A* path layer as the list selector. Within a layer, preserve AddContent order:
non-structures prepend, structures append. (Current Rust already sources the list layer from
`GameEntity::occupancy_list_layer()`/`on_bridge` and projects `on_bridge` before insertion —
P5 should formalize/keep this and add strict old-layer-remove / new-layer-add tests; P6
collapse then walks the two layers with different effects: ground list killed, bridge/deck list
relayered down via DropIn.)

---

## Sources (this session, all read-only)

- decompile_function: `0x0047E8A0`, `0x0047EA90`, `0x007441B0`, `0x00744210`, `0x005F5850`, `0x0043F180`
- disassemble_function: `0x005683C0`, `0x005687F0`, `0x0075AEC0`
- get_function_by_address: `0x005683C0`, `0x005687F0`, `0x004B0F20`, `0x0075AEC0`
- get_xrefs_to: `0x005683C0`, `0x005687F0`
- get_function_callers: `TechnoClass__EnterCell_AddToMultiCells`
- get_struct_layout: `CellClass`
- read_memory: `0x00b1d0ac` (cold = 0; runtime-init threshold)
- Prior docs cross-checked: `bridges/02-cell-state-layering-zones/BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md`, [bridges/02-cell-state-layering-zones/HIGH_BRIDGE_ONBRIDGE_OCCUPANCY_TRANSITION_SEQUENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/HIGH_BRIDGE_ONBRIDGE_OCCUPANCY_TRANSITION_SEQUENCE_GHIDRA_REPORT.md), [CELLCLASS_SUBSTRATE_LIVE_OBJECT_LIST_WRITERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_SUBSTRATE_LIVE_OBJECT_LIST_WRITERS_GHIDRA_REPORT.md)
