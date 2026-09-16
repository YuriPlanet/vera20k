# Building Can Enter Cell Placement Passability Body - Ghidra Research Report

**Address(es):** `0x00449440` (`BuildingClass::Can_Enter_Cell` wrapper), `0x0047C620` (`Cell_passability_building_placement`), `0x00716150` (building-type placement virtual)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** `BuildingClass::Can_Enter_Cell @ 0x00449440` and only the placement/passability helper path it uses for building/static placement and occupied cells.
**Non-Scope:** full `UnitClass::Can_Enter_Cell`, full `CellClass` passability taxonomy, bridge traversal internals, wall visual placement, AddOccupy/RemoveOccupy hidden occupancy writers, and runtime unit movement around building foundations except for the boundary statement.
**Confidence:** High for the wrapper body and placement helper boundary; Medium for exact human-readable names of some type/cell fields inherited from prior reports.
**Active in YR:** Yes for the wrapper as a BuildingClass vtable slot and for the helper/type placement path in active placement/deploy validation. Not active as a normal runtime unit pathing predicate.

## 1. Overview

`BuildingClass::Can_Enter_Cell @ 0x00449440` is a boolean-to-Can_Enter_Cell-code adapter around building placement predicates. It returns only `0` when the underlying placement predicate accepts and `7` when it rejects; it never returns unit soft-block codes `1..6`.

This function should not be wired into runtime unit pathing around buildings. Runtime unit pathing dispatches on the moving object (`UnitClass`, `InfantryClass`, etc.) and handles building occupants through those movers' `Can_Enter_Cell` implementations; the BuildingClass wrapper belongs to building/static placement checks.

## 2. Class Layout / Key Offsets

| Field / slot | Meaning in this slice | Active in YR | Evidence |
|---|---|---|---|
| BuildingClass vtable `+0x1AC` | Points to `0x00449440`; BuildingClass-side Can_Enter_Cell slot | Yes | Prior direct vtable read at `0x007E4068`; confirmed body decompile `0x00449440` |
| BuildingClass `+0x520` | `BuildingTypeClass*` | Yes | `0x00449440` loads `*(this+0x520)` before both branches |
| BuildingClass `+0x21C` | Owner/house pointer passed to placement predicates | Yes | `0x00449440` pushes/forwards `*(this+0x21C)` to helper/type virtual |
| BuildingClass `+0x74` | Active/construction gate for the special direct-helper branch | Conditional | `0x00449440` requires `type+0x408 != 0 && this+0x74 != 0` |
| BuildingTypeClass `+0x408` | Enables special direct `Cell_passability_building_placement` branch | Conditional | `0x00449440` tests `type+0x408` |
| BuildingTypeClass `+0x67C` | Speed/zone argument passed as helper arg 1 in the special branch | Conditional | Assembly `0x00449468` pushes `type+0x67C` |
| BuildingTypeClass vtable `+0xA8` | Type-level placement virtual, decompiled at `0x00716150` for building types | Yes | `0x004494A3` indirect call; `0x00716150` walks foundation list and calls `0x0047C620` |

## 3. Core Logic

`0x00449440` first copies the queried coordinate from the caller argument packet at `args+0x24` into a stack local. It then loads the building's type from `this+0x520`.

Branch A, active-construction/special branch:

1. If `type+0x408 != 0` and `building+0x74 != 0`, it reads owner/house from `building+0x21C` and speed/zone-like value from `type+0x67C`.
2. It calls `MapClass::Get_CellClass @ 0x005657A0` on the coordinate; assembly at `0x0044947B..0x00449482` moves the returned `CellClass*` into `ECX`.
3. It calls `Cell_passability_building_placement @ 0x0047C620` with that cell as `this`, plus the `type+0x67C`, `BuildingTypeClass*`, and owner/house arguments.
4. It maps the helper's nonzero/zero result to `0`/`7` using `NEG AL; SBB EAX,EAX; AND AL,0xF9; ADD EAX,7`.

Branch B, default branch:

1. It calls the building type's vtable `+0xA8` with the target coordinate and `building+0x21C`.
2. The verified building-type implementation at `0x00716150` gets the foundation offset list from vtable `+0x90`, walks until sentinel `(0x7FFF,0x7FFF)`, calls `MapClass::Get_CellClass` for each foundation cell, then calls `Cell_passability_building_placement @ 0x0047C620` for building types (`WhatAmI()==7`).
3. `0x00449440` maps the type virtual's boolean result to `0`/`7` with the same instruction sequence as Branch A.

`0x0047C620` itself is a placement predicate. For ordinary building placement it rejects visible/hard object blockers, `CellClass+0x124 & 0x3F`, blocking `CellClass+0x140` bits `0x100`/`0x400`, nonzero `CellClass+0x11C` slope, blocking overlays, and non-buildable land/speed table results, with special wall/laser-fence/ToTile exceptions documented in `CELL_PASSABILITY_BUILDING_PLACEMENT_FLAGS_GHIDRA_REPORT.md`.

## 4. INI Keys

| Key | Effect in this slice | Active in YR | Evidence |
|---|---|---|---|
| `Foundation=` | Supplies the base foundation offset list walked by `0x00716150`; cells then go through `0x0047C620` | Yes | `0x00716150` vtable `+0x90` walk; prior placement validator report |
| `Buildable=` | Used by `0x0047C620` when speed type argument is `-1` | Yes | `0x0047C620` reads byte at `0x0089EA60 + landType*0x24` |
| `WaterBound=` / `Naval=` and speed table values | Can switch placement from `Buildable=` to speed/iso-tile checks through the helper/type path | Conditional | `0x0047C620`; stock naval yards set water/naval flags |
| `AddOccupyN=` / `RemoveOccupyN=` | Not consumed by `0x00449440`, `0x00716150`, or `0x0047C620` placement acceptance | No for placement validation | Prior `BUILDING_PLACEMENT_VALIDATOR_FOUNDATION_HEIGHT_OCCUPY_GHIDRA_REPORT.md` |

## 5. Integration Points

| Integration point | Status | Active in YR | Evidence |
|---|---|---|---|
| BuildingClass vtable `+0x1AC` binding | Verified | Yes | [BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md) direct read `0x007E4068 -> 0x00449440`; current decompile |
| Building type placement virtual `+0xA8` | Verified | Yes | `0x004494A3` call; `0x00716150` decompile |
| Per-cell placement helper `0x0047C620` | Verified | Yes | `0x00449482`, `0x00716209`, and existing preview/execution caller docs |
| Runtime unit A* around building occupants | Boundary only | Yes, but not through `0x00449440` | Unit/Infantry vtable `+0x1AC` bindings point to `0x0073F0A0`/`0x0051BF90`, not BuildingClass, in prior hierarchy report |

## 6. Current Rust Implementation Status

Rust currently separates some placement checks in `src/sim/production/production_placement.rs` and static movement blocking in `src/sim/pathfinding/core.rs`, but `PathGrid` is also used by runtime A*.

Relevant surfaces:

| Rust area | Current status vs this finding |
|---|---|
| `src/sim/production/production_placement.rs:267` `evaluate_building_placement` | Correct high-level home for `0x00716150`/`0x0047C620`-style placement decisions, but not a binary-ordered predicate |
| `src/sim/production/production_placement.rs:361` `cell_placeable` | Uses resolved terrain/build blockers and structure overlap; should be compared to placement helper taxonomy, not unit Can_Enter_Cell |
| `src/sim/pathfinding/core.rs:1469` `block_building_movement_cells` | Correct runtime static-blocking surface; should be driven by mover-side Unit/Infantry building occupant semantics, not `BuildingClass::Can_Enter_Cell` |
| `src/sim/pathfinding/core.rs:508` A* comments/neighbor expansion | Runtime unit pathing should keep using mover Can_Enter_Cell-equivalent behavior; do not substitute the BuildingClass placement wrapper |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `BuildingClass::Can_Enter_Cell @ 0x00449440` full body | verified | Ghidra decompile and assembly context | none |
| Special branch `type+0x408 && building+0x74` | verified | `0x00449451..0x00449482` | semantic names for fields are medium confidence |
| Boolean-to-code mapping | verified | `0x00449487..0x00449490`, `0x004494A9..0x004494B2` | none |
| Default type vtable `+0xA8` path | verified | `0x00449493..0x004494A3`; decompile `0x00716150` | exact vtable owner census out-of-scope |
| `Cell_passability_building_placement @ 0x0047C620` inputs/outputs | verified for boundary | Ghidra decompile; prior placement flags report | full taxonomy delegated to existing report |
| Runtime unit pathing relationship | verified as negative boundary | vtable bindings: Unit/Infantry use `0x0073F0A0`/`0x0051BF90`; BuildingClass uses `0x00449440` | no runtime debugger trace needed for this boundary |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-1 - What does 0x00449440 return? -> Only 0 or 7; helper/type nonzero maps to 0, zero maps to 7.` (evidence: `0x00449487..0x00449490`, `0x004494A9..0x004494B2`; Active in YR: Yes)
- `[RESOLVED] OQ-2 - Does 0x00449440 itself perform unit-style occupant classification? -> No; it delegates to placement predicates and has no code paths for soft return codes 1..6.` (evidence: `0x00449440` decompile; Active in YR: Yes)
- `[RESOLVED] OQ-3 - What is the default helper path? -> BuildingType vtable +0xA8, decompiled at 0x00716150, walks base foundation offsets and calls 0x0047C620 per cell for building types.` (evidence: `0x004494A3`, `0x00716150`; Active in YR: Yes)
- `[RESOLVED] OQ-4 - What is the special helper path? -> If type+0x408 and building+0x74 are nonzero, wrapper directly calls 0x0047C620 on the queried cell using type+0x67C, type pointer, and owner/house.` (evidence: `0x00449451..0x00449482`; Active in YR: Conditional)
- `[RESOLVED] OQ-5 - Should this influence runtime unit pathing cells around buildings? -> No. Runtime unit pathing dispatches on moving Unit/Infantry classes, not BuildingClass, and building occupant behavior belongs in those mover predicates/static blocker surfaces.` (evidence: vtable binding report plus current wrapper body; Active in YR: Yes as negative boundary)
- `[DEFERRED] OQ-6 - Exact names/default producers for BuildingType+0x408 and +0x67C.` (category: requires-different-system-context; reason: not needed to prove placement-helper boundary; next-step-if-pursued: trace BuildingTypeClass constructor/INI fields around those offsets)
- `[DEFERRED] OQ-7 - Every direct/callsite xref to 0x00449440 beyond vtable dispatch.` (category: bounded-cost-too-high; reason: available Ghidra tools did not expose xref enumeration; vtable binding plus body are sufficient for this slice; next-step-if-pursued: use xref tool/read-only script if available)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| BuildingClass `+0x1AC` returns only `0` or `7` by wrapping placement predicates, not unit occupant soft codes | `0x00449487..0x004494B2` | none observed for unit pathing; unchecked for exact placement taxonomy | `src/sim/production/production_placement.rs::cell_placeable`; `src/sim/pathfinding/core.rs::block_building_movement_cells` | Keep BuildingClass placement logic out of runtime unit A*; use it only as a placement/static validation reference | `building_can_enter_cell_not_used_for_runtime_unit_pathing_blocks` | Do not model building-occupied cells in unit A* by calling a BuildingClass-style 0/7 placement predicate |
| Default wrapper path delegates to type vtable `+0xA8`, which walks base foundation cells and calls `0x0047C620` | `0x004494A3`, `0x00716150` | partial: Rust validates rectangular foundation and terrain/overlap, but not exact ordered `0x0047C620` taxonomy | `src/sim/production/production_placement.rs::evaluate_building_placement` and `cell_placeable` | Treat exact `0x0047C620` parity as placement validator work, separate from movement blockers | `placement_uses_buildable_column_not_pathgrid_walkable_for_ordinary_land_cells` | Do not let runtime `PathGrid::is_walkable` become the source of truth for ready-building placement when resolved terrain/buildable data is available |
| Runtime cells around placed buildings are governed by mover-side Can_Enter_Cell (`UnitClass`/`InfantryClass`) and static blocker data, not this wrapper | Prior vtable report: Unit `0x0073F0A0`, Infantry `0x0051BF90`, Building `0x00449440`; current body has only placement calls | current Rust delta: unchecked for every building edge case, but surface exists | `src/sim/pathfinding/core.rs::block_building_movement_cells`; `src/sim/pathfinding/cell_entry.rs` | Continue implementing HasBib/NumberImpassableRows/contact/bunker behavior in mover-side pathing surfaces | `unit_pathing_uses_mover_building_occupant_rules_not_building_placement_can_enter` | Do not collapse placement blocked cells and unit movement blocked cells into one universal building Can_Enter_Cell result |

### Stale Docs / Follow-up Docs

- `docs/research/BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md` lines 394-397 should be narrowed. Replacement wording:
  "This function is a BuildingClass vtable `+0x1AC` placement/static predicate wrapper. The verified helper path is BuildingType vtable `+0xA8` / `Cell_passability_building_placement @ 0x0047C620`; do not cite engineer capture or runtime unit pathing as verified uses unless separately traced."

## 10. Negative Facts / Do Not Do

- Do not port `0x00449440` as the runtime unit pathing predicate for building-occupied cells. Evidence: moving Unit/Infantry classes have different vtable `+0x1AC` targets; Active in YR: Yes.
- Do not expect return codes `1..6` from BuildingClass `+0x1AC`. Evidence: both return sites use the same boolean-to-`0/7` mapping; Active in YR: Yes.
- Do not use AddOccupy/RemoveOccupy as placement acceptance inputs for this wrapper/helper path. Evidence: `0x00716150` walks base foundation list; prior placement report shows Add/Remove are hidden occupancy writers only; Active in YR: No for placement validation.
- Do not infer `PathGrid::is_walkable` is equivalent to `0x0047C620`. Evidence: helper uses `Buildable=`/speed table, overlays, cell flags, slope, and object scans; Active in YR: Yes.
- Do not treat `BuildingClass` vtable `+0x1B0` as a bridge sub-check for buildings. Evidence: prior direct read shows `+0x1B0 -> 0x004264D0` return-zero stub; Active in YR: Yes.

## 11. Remaining Uncertainty

- Exact semantic names and INI/default producers for `BuildingTypeClass+0x408` and `+0x67C` remain unresolved, but their control-flow role in `0x00449440` is verified.
- Full xref inventory for `0x00449440` beyond vtable binding was not enumerated with the available read-only toolset.

## Sources

- Ghidra decompile: `BuildingClass::Can_Enter_Cell @ 0x00449440`
- Ghidra assembly context: `0x00449440..0x004494B2`
- Ghidra decompile: `Cell_passability_building_placement @ 0x0047C620`
- Ghidra decompile: building-type placement virtual `0x00716150`
- Existing docs: [BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md), `CELL_PASSABILITY_BUILDING_PLACEMENT_FLAGS_GHIDRA_REPORT.md`, `BUILDING_PLACEMENT_VALIDATOR_FOUNDATION_HEIGHT_OCCUPY_GHIDRA_REPORT.md`, [SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md)
- Rust scan: `src/sim/production/production_placement.rs`, `src/sim/pathfinding/core.rs`, `src/rules/object_type.rs`, `src/rules/terrain_rules.rs`
