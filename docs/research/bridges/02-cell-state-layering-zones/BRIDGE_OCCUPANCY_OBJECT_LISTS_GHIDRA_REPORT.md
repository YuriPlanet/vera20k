# Bridge Occupancy and Object Lists - Ghidra Research Report

**Addresses:** `0x0047E8A0`, `0x0047EA90`, `0x007441B0`, `0x00744210`, `0x005F60A0`, `0x005F6120`, `0x0073F0A0`, `0x004834A0`, `0x0047DD70`
**Confidence:** High for CellClass fields, list/bit selection, UnitClass cell-entry behavior, and bridge-collapse object handling. Medium for non-ground locomotor writer inventory because only the normal drive/walk/ship families were fully traced in parent reports.
**Active in YR:** Yes. These functions are live in standard Yuri's Revenge movement, placement, pathfinding, object marking, and bridge-collapse flows. `TubeClass`/low-bridge tunnel handling is conditional and separate from the high-bridge dual-list machinery.

## Summary Verdict

`gamemd.exe` uses two independent per-cell object-list heads and two independent occupancy bitfields:

- Ground object list: `CellClass+0xE4` (`FirstObject`)
- Bridge/deck object list: `CellClass+0xE8` (`AltObject`)
- Ground occupancy bits: `CellClass+0x124`
- Bridge/deck occupancy bits: `CellClass+0x128`

The object lists and occupancy bits are related but not selected from one single source. `CellClass::AddContent` / `RemoveContent` choose `+0xE4` versus `+0xE8` from the caller's list-layer argument, normally `ObjectClass+0x8C` (`OnBridge`). `ObjectClass::Mark_Occupation` and `Clear_Occupation` choose `+0x124` versus `+0x128` from the object's Z compared against ground height plus a bridge Z threshold. `UnitClass::Can_Enter_Cell` also has a split decision: it chooses the object list before `CheckBridgeTraversal`, then may re-snapshot the occupancy bits after `CheckBridgeTraversal`.

Player-visible result: ground/under-bridge occupants and bridge-deck occupants do not block each other in the normal cell-entry/object-list scan. They can occupy the same 2D map cell on different lists. Bridge collapse handles the two lists differently: `FirstObject` occupants are damaged with C4Warhead semantics, while `AltObject` occupants are sent through `DropIn` and survive by snapping down to ground level.

## Verified CellClass Field Map

| Offset | Type | Meaning | Evidence | Confidence | Active in YR |
|---:|---|---|---|---|---|
| `+0x54` | `int` | Ground infantry owner / secondary occupancy metadata used by `Can_Enter_Cell` ground snapshot | `UnitClass::Can_Enter_Cell @ 0x0073F0A0` reads `cell+0x54` with ground `+0x124` snapshot | Medium | Yes |
| `+0x58` | `int` | Bridge infantry owner / secondary occupancy metadata used by `Can_Enter_Cell` bridge snapshot | `UnitClass::Can_Enter_Cell @ 0x0073F0A0` switches to `cell+0x58` with `+0x128` | Medium | Yes |
| `+0xE4` | `ObjectClass*` | Ground object-list head (`FirstObject`) | `CellClass::AddContent @ 0x0047E8A0`, `RemoveContent @ 0x0047EA90`, `UnitClass::Can_Enter_Cell @ 0x0073F0A0` | High | Yes |
| `+0xE8` | `ObjectClass*` | Bridge/deck object-list head (`AltObject`) | Same functions; selected when list flag is nonzero | High | Yes |
| `+0x124` | `uint32` | Ground occupation bitfield | `Mark_Occupation`, `Clear_Occupation`, `CheckCellPassability`, `Can_Enter_Cell` | High | Yes |
| `+0x128` | `uint32` | Bridge/deck occupation bitfield | Same functions; bridge-layer mirror | High | Yes |
| `+0x140` | `uint32` | Cell flags. Bridge-relevant: `0x80` bridge overlay/effective height, `0x100` structural bridge, `0x200` bridgehead, `0x400` rail, `0x800` orientation | `GetEffectiveHeight @ 0x00487D50`, `Can_Enter_Cell`, `CheckBridgeTraversal @ 0x004D9C60`, `Unlimbo @ 0x005F5940` | High | Yes |

Constructor evidence: `CellClass::Constructor @ 0x0047BBF0` initializes `+0xE4` and `+0xE8` to null, initializes the relevant occupation bytes to zero, and clears low flag bits at `+0x140`. Existing `CELLCLASS_STRUCT_GHIDRA_REPORT.md` identifies the struct size as `0x148`.

## Occupation Bit Layout

Both `+0x124` and `+0x128` share the same layout.

| Mask | Meaning | Verified writer/reader |
|---:|---|---|
| `0x04`, `0x08`, `0x10` | Infantry subcell bits | Prior `PlaceInfantryInCell` / subcell docs; `CheckCellPassability` masks them |
| `0x20` | Vehicle / moving object occupation reservation | `ObjectClass::Mark_Occupation @ 0x007441B0`, `Clear_Occupation @ 0x00744210` |
| `0x40` | Placed object / building-style occupation bit | `ObjectClass::Mark_Put @ 0x005F60A0`, `Mark_Remove @ 0x005F6120` |

`CellClass::CheckCellPassability @ 0x004834A0` proves the masks:

- If `IgnoreInfantry` is true, it applies `mask &= 0xE0`, clearing infantry subcell bits and keeping vehicle/building bits.
- If `IgnoreVehicle` is true, it applies `mask &= 0x5F`, clearing the vehicle bit while keeping infantry/building bits.
- Any remaining nonzero bit blocks the cell.

## Verified Writers

### `CellClass::AddContent @ 0x0047E8A0`

Verified binary behavior:

1. If the stack list-layer argument is `0`, read/write `this+0xE4`.
2. If the list-layer argument is nonzero, read/write `this+0xE8`.
3. Null object returns immediately.
4. If `object->WhatAmI() == 6` and the selected list is nonempty, append the object at the selected list tail and set `object+0x30 = 0`.
5. Otherwise, prepend to the selected list by writing `object+0x30 = old_head`, then updating either `+0xE4` or `+0xE8`.
6. It inserts when `old_head == null` or `old_head->NextObject != object`; it skips insertion only when `old_head != null` and `old_head->NextObject == object`. This is not a general full-list duplicate check.
7. After list insertion, visible/active objects call their vtable `+0xF0` mark routine. For buildings, the coordinates passed to that virtual mark are the cell center `(x*0x100+0x80, y*0x100+0x80, z=0)`; for non-buildings it passes the object's own current coordinate fields.

Active in YR: Yes.

### `CellClass::RemoveContent @ 0x0047EA90`

Verified binary behavior:

1. Null object does nothing.
2. The same list-layer argument selects `+0xE4` or `+0xE8`.
3. If the selected head is the object, the head is replaced by `object+0x30`.
4. Otherwise it walks the selected list until a predecessor whose `NextObject` is the object is found.
5. It clears `object+0x30 = 0` after a successful head/predecessor removal.
6. If the object is not found, it still reaches the post-removal virtual mark call path after `LAB_0047EAF7`; the decompile shows no assert or failure return.
7. Post-removal visible/active objects call vtable `+0xF4` clear routine, using the same building-center versus object-coordinate split as AddContent.

Active in YR: Yes.

### `ObjectClass::Mark_Occupation @ 0x007441B0`

Verified decompile:

```text
cell = CellClass::Get_Cell_At(coords)
ground_z = CellClass::GetGroundHeight(coords)
if ground_z + DAT_00B1D0AC <= coords.Z
   and (cell.Flags & 0x100) != 0:
    cell[0x128] |= 0x20
else:
    cell[0x124] |= 0x20
```

Tiny detail: this writer requires both the Z threshold and the structural bridge flag `0x100`. A high-Z object over a non-bridge cell sets ground `+0x124`, not bridge `+0x128`.

Active in YR: Yes.

### `ObjectClass::Clear_Occupation @ 0x00744210`

Verified decompile:

```text
cell = CellClass::Get_Cell_At(coords)
ground_z = CellClass::GetGroundHeight(coords)
if ground_z + DAT_00B1D0AC <= coords.Z:
    cell[0x128] &= ~0x20
else:
    cell[0x124] &= ~0x20
```

Tiny detail: unlike `Mark_Occupation`, this clear routine does not check `cell.Flags&0x100`. This is load-bearing for collapse/destruction cleanup: if bridge flags have already been cleared while the object Z still reflects the bridge layer, the old bridge occupation bit can still be cleared.

Active in YR: Yes.

### `ObjectClass::Mark_Put @ 0x005F60A0` and `Mark_Remove @ 0x005F6120`

These set/clear `0x40` with a similar but not identical predicate:

- Compare `coords.Z >= ground_z + DAT_00AC13BC`.
- If true, fetch the cell and require `(cell.Flags&0x100) != 0`.
- With the bridge flag present, write `+0x128`; otherwise write `+0x124`.

`Mark_Remove` mirrors the same flag-gated predicate for `0x40`, unlike `Clear_Occupation` for `0x20`.

Active in YR: Yes.

### `ObjectClass::DropIn @ 0x005F4160`

Verified sequence:

1. Set falling/bomb bytes at `+0x8D` and `+0x8F`.
2. `Mark(0)` through vtable `+0x124`.
3. `DisplayClass::RemoveFromLayer`.
4. Clear `ObjectClass+0x8C` (`OnBridge`) to `0`.
5. `DisplayClass::Submit_Object`.
6. `Mark(1)`.
7. If the object has an attached locomotor/cargo state, run extra cleanup or vtable `+0xF4`.

Active in YR: Yes. This is the path used for bridge-deck objects during `BlowUpBridge`.

### `ObjectClass::Unlimbo @ 0x005F5940`

Verified placement rule:

- If target cell has `Flags&0x100`, set `ObjectClass+0x8C = 1`.
- If the same cell lacks `Flags&0x200`, return failure after the byte write.
- For Techno objects, zone and `CellClass::CheckCellPassability` are called with the object-local `OnBridge` byte as the zone/passability layer argument.

Tiny detail: failed Unlimbo into a bridge structural non-bridgehead cell can leave `OnBridge=1` even though placement returns failure. This side effect is real; whether any standard caller observes it remains open.

Active in YR: Yes.

## Verified Readers

### `UnitClass::Can_Enter_Cell @ 0x0073F0A0`

This is the main verified movement legality reader for vehicle cell entry.

Initial list-layer decision, before `CheckBridgeTraversal`:

```text
if (cell.Flags & 0x100) == 0:
    object_list_layer = ground
elif height_param != -1 and abs(height_param - cell.Level) < 2:
    object_list_layer = ground
else:
    object_list_layer = bridge
```

Initial ground occupancy snapshot:

- Always snapshots `cell+0x124` low byte and `(cell+0x124 >> 5) & 1`.
- Also snapshots `cell+0x54`.

Then it calls vtable `+0x1B0`, resolved in this context to `CheckBridgeTraversal @ 0x004D9C60`. That helper may update the passed `height_param`.

Post-`CheckBridgeTraversal` bridge occupancy overwrite:

```text
if height_param != -1
   and (cell.Flags & 0x100) != 0
   and height_param == cell.Level + 4:
    use cell+0x128 low byte
    use cell+0x58
    use (cell+0x128 >> 5) & 1
```

Then the object-list loop chooses only one list:

```text
if object_list_layer == ground:
    obj = cell+0xE4
else:
    obj = cell+0xE8
while obj:
    classify occupant
    obj = obj+0x30
```

Tiny detail: list-layer and occupancy-bit-layer can disagree in edge cases because the list-layer byte is chosen before `CheckBridgeTraversal`, while the occupancy bits may be overwritten after it. The binary does not re-select `+0xE4`/`+0xE8` after `CheckBridgeTraversal`.

Active in YR: Yes.

### `CellClass::CheckCellPassability @ 0x004834A0`

This lower-level passability reader uses required height plus an explicit OnBridge/list argument:

- If `RequiredHeight == cell.Level`, a structural bridge cell blocks when the OnBridge argument is false.
- If `RequiredHeight != cell.Level`, it requires `Flags&0x100` and `RequiredHeight == cell.Level + 4`.
- For occupation bits, it uses `+0x128` only when `(RequiredHeight == -1 or RequiredHeight == cell.Level+4) and Flags&0x100`; otherwise it uses `+0x124`.
- Speed table `g_SpeedType_LandType_Table` is skipped when the chosen occupancy layer is bridge (`bVar2=true` in the decompile).

Active in YR: Yes.

### Object-list utility readers

- `CellClass::Scatter_Objects @ 0x00481670` selects `+0xE4` or `+0xE8` from its bridge-layer parameter, walks the selected list, collects up to 10 objects, then scatters in collected list order.
- `CellClass::Find_Nearest_Object @ 0x0047C3D0` selects `+0xE4` or `+0xE8` from its bridge-layer parameter and never checks the other layer.
- `CellClass::BlowUpBridge @ 0x0047DD70` explicitly walks both lists, with different effects.

Active in YR: Yes.

## Occupancy Behavior by Scenario

### Unit under bridge

Verified facts:

- A unit is treated as under/ground in `Can_Enter_Cell` when `abs(height_param - cell.Level) < 2` or the cell lacks `Flags&0x100`.
- `Can_Enter_Cell` then scans `cell+0xE4`.
- It snapshots ground `+0x124` unless the post-traversal height is exactly `cell.Level+4`.
- `CellClass::CheckCellPassability` blocks a `RequiredHeight == cell.Level` request on a structural bridge only if the explicit OnBridge argument is false.

Result: under-bridge movement reads the ground list and ground bits. Bridge-deck occupants on `+0xE8` are not scanned by the normal under-bridge object-list loop.

### Unit on bridge

Verified facts:

- A unit at bridge/deck height selects `cell+0xE8` in `Can_Enter_Cell`.
- Bridge occupancy bits are `cell+0x128` when the post-traversal height equals `cell.Level+4`.
- `AddContent` and `RemoveContent` use `ObjectClass+0x8C` as their normal list selector via Techno enter/exit callers (`TechnoClass__EnterCell_AddToMultiCells @ 0x005683C0`, `ExitCell_RemoveFromMultiCells @ 0x005687F0`, assembly verified in prior [BRIDGE_OBJECT_ONBRIDGE_FIELD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_OBJECT_ONBRIDGE_FIELD_GHIDRA_REPORT.md)).

Result: bridge-deck movement reads and writes the bridge list and bridge bits. Ground occupants on `+0xE4` are not scanned by the normal bridge-deck list loop.

### Transition / ramp

Verified facts:

- `CheckBridgeTraversal @ 0x004D9C60` enforces bridge entry/exit height rules.
- Height-diff `4` is the bridge transition case.
- Entering bridge requires structural bridge and bridgehead flags in the relevant branch.
- Normal drive/walk/ship movement updates `ObjectClass+0x8C` after old-cell removal and coordinate update, before new-cell insertion. This was verified in [BRIDGE_OBJECT_ONBRIDGE_FIELD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_OBJECT_ONBRIDGE_FIELD_GHIDRA_REPORT.md) at `DriveLocomotionClass::Process_Drive_Track`, `WalkLocomotionClass::ProcessMovement`, and ship counterparts.

Important order:

1. Remove from old cell using old `OnBridge`.
2. Move coordinates.
3. Evaluate the bridge transition predicate.
4. Update `OnBridge` if the predicate fires.
5. Add to new cell using new `OnBridge`.

Result: a transition can remove from the ground list and insert into the bridge list, or vice versa. A single move operation with one layer argument is not equivalent when the layer changes at the boundary.

### Bridge collapse

`CellClass::BlowUpBridge @ 0x0047DD70` is the key verified object-list behavior:

```text
if not map editor:
    for obj in this.FirstObject (+0xE4):
        next = obj.NextObject
        obj.Take_Damage(&obj.Health, 0, Rules.C4Warhead, 0, 1, 1, 0)

    for obj in this.AltObject (+0xE8):
        next = obj.NextObject
        obj.DropIn()       // vtable +0xEC

    push cell into global death-list attempt (dead TS legacy per runtime report)
    maybe spawn debris / bridge explosion anims
```

Tiny details:

- The ground-list loop snapshots `next = obj+0x30` before calling damage.
- The bridge-list loop snapshots `next = obj+0x30` before calling `DropIn`.
- The function does not damage `AltObject` entries with C4Warhead. They are dropped in.
- `DropIn` clears `ObjectClass+0x8C` before re-submitting and marking.

Active in YR: Yes, except the global death-list push is confirmed TS-legacy/dead in [BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md).

### Falling / debris

Two different mechanisms matter:

- Falling bridge-deck objects: `BlowUpBridge` calls `DropIn` on every `AltObject`. `DropIn` clears `OnBridge`, removes/re-submits display-layer membership, and marks the object again. This is not a damage/despawn path.
- Bridge debris/explosion visuals: `BlowUpBridge` uses `RulesClass` debris lists (`MetallicDebris`, `BridgeExplosions`) after the list walks. These are animation/world-effect spawns, not CellClass object-list occupants in the same sense as units. The detailed RNG and list labels are covered by [BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md).

Result: falling units are relayered through `DropIn`; debris visuals do not use the high-bridge unit occupancy lists as blockers.

### Blocked movement

Verified facts:

- `Can_Enter_Cell` scans exactly one selected object list.
- `CheckCellPassability` reads exactly one selected occupation bitfield.
- `CellClass::Scatter_Objects` scatters only one selected list.
- A* soft-block maps in the Rust implementation mirror this with separate ground/bridge blocker maps.

Result: normal movement blockage is layer-local. There is no verified binary behavior where a ground object under a high bridge blocks a bridge-deck unit through the standard object-list scan, or where a bridge-deck unit blocks a ground/under-bridge unit through that scan.

### Low bridge / TubeClass

Verified facts:

- `CellClass::IsLowBridgeCell @ 0x00484AB0` returns true when `TubeIndex (+0x116)` is in range and `LandType (+0xEC) == 10`.
- `CellClass::GetTubeAtCell @ 0x00484F20` returns `g_TubeArray[TubeIndex]` when the signed tube index is valid.
- `UnitClass::Can_Enter_Cell` has tube checks before and after neighbor update. If direction is `8`, it requires a tube at the cell and nonzero tube endpoint data, then returns clear.

Interpretation: low bridge / tunnel movement uses TubeClass-specific gates. It shares `CellClass` storage and some high-level passability functions, but the high-bridge dual object-list behavior verified here is driven by `Flags&0x100`, `OnBridge`, and height. `TubeClass` is not the high-bridge `+0xE8` deck-list selector.

Active in YR: Conditional on tunnel/low-bridge cells; not the normal high-bridge deck/underpass path.

## Comparison to Current Rust

Verified current Rust state from the requested files:

- `src/sim/occupancy.rs` has one `OccupancyGrid` with layer-tagged entries. This is structurally equivalent to `FirstObject`/`AltObject` and preserves independent per-layer order.
- `OccupancyGrid::add` prepends non-buildings within the selected layer and appends structures within the selected layer, matching `CellClass::AddContent`.
- `src/sim/pathfinding/core.rs` has `LayeredEntityBlockMap` split into ground and bridge maps, matching the binary's one-list-at-a-time soft-block behavior.
- `src/sim/pathfinding/core.rs::can_enter_layer_context` represents the binary split between object-list layer and occupancy-bit layer.
- `src/sim/movement/movement_occupancy.rs::resolve_runtime_can_enter_layers` has tests that explicitly allow object-list layer and occupancy-bit layer to disagree.
- `src/sim/movement/movement_bridge.rs` represents persistent `on_bridge` separately from locomotor/path layer and matches the verified transition predicate shape.
- `src/sim/world/bridge_orchestrator.rs` kills non-bridge-layer occupants and drops in bridge-layer occupants, matching `BlowUpBridge` at the gameplay-outcome level.

Confirmed differences / risks:

1. `OccupancyGrid::rebuild` derives the occupancy layer from `locomotor.layer`, not `GameEntity::on_bridge`. Binary cell-list membership is normally selected by `ObjectClass+0x8C`, not the path layer. This can rebuild bridge-ramp edge states into the wrong list when `loco.layer` and `on_bridge` intentionally disagree.
2. `movement_step.rs::process_cell_crossings` calls `occupancy.move_entity(..., active_layer, ...)` before `resolve_cell_transition_bridge_state`. That inserts into the old/path layer before the post-transition `OnBridge` state is known.
3. The drive-track cell-jump path in `movement_tick.rs` resolves bridge state before moving occupancy, but still moves occupancy using `active_layer` / path layer rather than the post-transition `on_bridge` list selector. On ramp ticks where path layer and `OnBridge` disagree, this can still select the wrong list.
4. `bridge_orchestrator.rs::drop_in_bridge_deck_entities` clears `on_bridge` and `bridge_occupancy` and sets locomotor layer to ground, but the helper as scanned does not relayer the persistent `OccupancyGrid` entry from bridge to ground. Binary `DropIn` removes/re-submits/marks around the `OnBridge=0` write.
5. Rust has no explicit model of separate `+0x124` versus `+0x128` bitfields; it approximates them with layer-filtered occupants and subcells. The current `CanEnterLayerContext` split covers many cases, but any future reservation/bit-only logic must preserve the binary's object-list-layer versus occupancy-bit-layer mismatch.

## Confirmed Parity Gaps

| Gap | Binary fact | Current Rust risk |
|---|---|---|
| Rebuild layer source | Cell-list layer is `ObjectClass+0x8C` (`OnBridge`) for normal object add/remove | `OccupancyGrid::rebuild` uses `locomotor.layer` |
| Transition insertion timing | Old-cell removal uses old `OnBridge`; new-cell insertion uses updated `OnBridge` | `movement_step.rs` moves occupancy before bridge update |
| Path layer vs list layer | `loco.layer` / path height can disagree with `OnBridge` on ramps | Some move paths pass `active_layer` to `move_entity` |
| DropIn relayering | `DropIn` clears `OnBridge` before re-submitting and marking | Rust clears entity state but does not visibly relayer occupancy in `drop_in_bridge_deck_entities` |
| Clear asymmetry | `Clear_Occupation` does not require `Flags&0x100` for bridge bit clear | Rust has no direct equivalent; future bitfield/reservation work must preserve this |

## Implementation Implications

These are implications for future work, not code changes made in this investigation:

- Treat `on_bridge` as the authoritative object-list selector for normal ground/bridge cell-list membership.
- Keep path/terrain layer and object-list layer separate. `Can_Enter_Cell` proves the binary can use bridge object list with ground occupancy bits, and vice versa.
- Model bridge collapse as two list walks with different effects: ground list gets C4Warhead damage, bridge list gets `DropIn`.
- Preserve `Clear_Occupation`'s no-bridge-flag asymmetry if a bitfield-style reservation model is added.
- Low bridge / tube logic should not be folded into high-bridge `AltObject` semantics without a separate TubeClass investigation.

## Remaining Open Questions

1. `DAT_00B1D0AC` and `DAT_00AC13BC` are runtime-initialized bridge Z thresholds. Static memory reads are zero in the cold image. Existing docs identify them as bridge Z offset / height-step family globals, but a focused initializer audit would pin their exact load-time formulas and whether they can diverge by locomotor family.
2. Non-drive writer families for `ObjectClass+0x8C` remain only partially scoped here. Parent reports list teleport, hover, jumpjet, carryall, aircraft, and animation writers. Each should be verified before porting those locomotor-specific placement paths.
3. Failed `Unlimbo` into structural non-bridgehead cells writes `OnBridge=1` before returning failure. No normal YR caller consequence was proven in this pass.
4. The post-loop infantry-crush fallback noted in `BRIDGE_SYSTEM.md` should be rechecked if infantry-on-bridge crush parity becomes a target; this report focused on the main list/bit selection.

## Sources

Fresh Ghidra decompilation in this pass:

- `0x0047BBF0` - `CellClass::Constructor`
- `0x0047E8A0` - `CellClass::AddContent`
- `0x0047EA90` - `CellClass::RemoveContent`
- `0x007441B0` - `ObjectClass::Mark_Occupation`
- `0x00744210` - `ObjectClass::Clear_Occupation`
- `0x005F60A0` - `ObjectClass::Mark_Put`
- `0x005F6120` - `ObjectClass::Mark_Remove`
- `0x005F5850` - `ObjectClass::Mark`
- `0x005F4160` - `ObjectClass::DropIn`
- `0x005F5940` - `ObjectClass::Unlimbo`
- `0x005F6A70` - `ObjectClass::ShouldBeOnBridge`
- `0x0073F0A0` - `UnitClass::Can_Enter_Cell`
- `0x004834A0` - `CellClass::CheckCellPassability`
- `0x004D9C60` - `CheckBridgeTraversal`
- `0x0047DD70` - `CellClass::BlowUpBridge`
- `0x00481670` - `CellClass::Scatter_Objects`
- `0x0047C3D0` - `CellClass::Find_Nearest_Object`
- `0x00484AB0` - `CellClass::IsLowBridgeCell`
- `0x00484F20` - `CellClass::GetTubeAtCell`
- `0x00486750` - `CellClass::IsBridge`
- `0x00487D50` - `CellClass::GetEffectiveHeight`
- `0x005683C0` - `TechnoClass__EnterCell_AddToMultiCells`
- `0x005687F0` - `TechnoClass__ExitCell_RemoveFromMultiCells`

Existing verified reports checked:

- `docs/research/BRIDGE_SYSTEM.md`
- `docs/research/BRIDGE_OBJECT_ONBRIDGE_FIELD_GHIDRA_REPORT.md`
- `docs/research/BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md`
- `docs/research/CELLCLASS_STRUCT_GHIDRA_REPORT.md`
- [docs/research/CELL_OCCUPATION_MARKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELL_OCCUPATION_MARKING_GHIDRA_REPORT.md)
- [docs/research/OBJECTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_GHIDRA_REPORT.md)

Rust files compared:

- `src/sim/occupancy.rs`
- `src/sim/movement/movement_occupancy.rs`
- `src/sim/movement/movement_bridge.rs`
- `src/sim/movement/movement_step.rs`
- `src/sim/movement/movement_tick.rs`
- `src/sim/pathfinding/core.rs`
- `src/sim/world/bridge_orchestrator.rs`

INI/data checked:

- `ini/rulesmd.ini`, `ini/rules.ini`, `ini/artmd.ini`, `ini/art.ini` were searched for bridge, tube, layer, and occupancy-related keys. No INI key changes the verified CellClass dual-list field layout; bridge runtime behavior is driven by cell flags, height, `OnBridge`, map bridge records, and object/warhead rules.
