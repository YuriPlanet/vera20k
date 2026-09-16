# CellClass Substrate CellRect Validator Contracts - Ghidra Research Report

**Address(es):** `0x0056E7C0` (`CellRect__CheckPassability`), `0x00586780` (`CellRect__CheckOccupancy`), support `0x004834A0`, `0x0047C550`, `0x0047C520`, `0x00578390`, callers `0x0056DC20`, `0x005060B0`  
**Investigation Mode:** exhaustive-slice, gap-focused consolidation over existing corrected Ghidra reports  
**Claimed Scope:** exact API/contract boundary for the two CellRect validators as they should map into a Rust-native CellClass substrate: argument meanings, blocker/passability order, return semantics, YR liveness, and what must come from CellClass-like substrate state versus helper-only Rust state.  
**Non-Scope:** `UnitClass::Can_Enter_Cell` runtime return-code tree, complete `Find_Nearby_Passable_Cell` candidate ordering, complete `CellClass+0xDC` writer lifecycle, exact RTTI name for object type `0x24`, and Rust implementation.  
**Confidence:** High for validator bodies and direct live callsites, based on corrected/audited Ghidra-backed reports; Medium for future Rust ownership shape because implementation design is deferred.  
**Active in YR:** Yes. `CheckPassability` is reached by live `FootClass__Find_Nearby_Passable_Cell @ 0x0056DC20`; `CheckOccupancy` is reached by that helper and live AI/site helper `FUN_005060B0`.

## Working Notes Gate

- Target question: What exact contracts do `CellRect__CheckPassability @ 0x0056E7C0` and `CellRect__CheckOccupancy @ 0x00586780` expose to a future native CellClass substrate?
- Non-goals: Do not implement Rust, do not redesign `Can_Enter_Cell`, do not complete all reservation writers, and do not investigate unrelated placement/pathfinding systems beyond caller evidence.
- Evidence needed to mark COMPLETE: decompile/disassembly-backed validator argument/order facts, live caller evidence, field-read contract, Rust touchpoint scan, negative facts, and at least one implementation handoff with test-name proposals.
- Stop conditions: Stop once both validator API boundaries, split responsibilities, source-state requirements, and migration-test implications are resolved or explicitly deferred with no open material question for this slice.

## 1. Overview

The two validators are adjacent but not interchangeable. `CheckPassability` validates each cell in a rectangle against terrain/zone/height/layer/occupation-byte logic through `CellClass__CheckCellPassability`; it does not do object-list rectangle occupancy or final playfield containment. `CheckOccupancy` validates a `CellRect` against object-list blockers, optional `CellClass+0xDC` reservation bits, selected cell-field blockers, and final four-corner playfield containment; it does not read SpeedType, MovementZone, LandType speed, zone IDs, bridge occupation bytes, or bridge object lists.

For a Rust-native CellClass substrate, this means the substrate needs explicit cell fields and object-list views, while the validators stay two separate query surfaces with different caller arguments and different skipped/active blockers.

## 2. Class Layout / Key Offsets

| Offset / item | Contract needed by CellClass substrate | Active in YR | Evidence |
|---|---|---|---|
| `CellStruct` top-left | `CheckPassability` takes packed signed 16-bit `x,y` plus explicit width/height stack args. | Yes | `0x0056E7E8..0x0056E7FA`, [CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md) |
| `CellRect` | `CheckOccupancy` reads four 32-bit fields `x,y,width,height`; low 16-bit signed coords are used for cell lookup. | Yes | `0x005867AC..0x005867CE`, `0x00578390` |
| cell lookup | Both validators use fixed 512-wide map indexing and dummy-cell fallback for out-of-range lookup. | Yes | `0x0056E7FF..0x0056E832`, `0x005867D4..0x00586812` |
| `CellClass+0x44` | Overlay type index. `CheckPassability` rejects it only when caller `reject_any_overlay != 0`; `CheckOccupancy` always requires `-1`. | Yes / Conditional | `0x0056E832..0x0056E83E`, `0x0058682D..0x00586832` |
| `CellClass+0x4C` | Reduced zone/object-ish cell field: used for zone systems elsewhere; in `CheckOccupancy`, any nonzero value blocks. | Yes | `0x00586833..0x00586838`, `CELLCLASS_STRUCT_GHIDRA_REPORT.md` naming conflict noted |
| `CellClass+0x11B` | Base cell level/height byte used by `0x004834A0` height/layer checks. | Yes | `0x004834EF..0x00483527` |
| `CellClass+0x11C` | Slope/special byte; `CheckOccupancy` rejects any nonzero value. | Yes | `0x0058683A..0x00586842` |
| `CellClass+0xDC` | Per-house/base placement reservation bitmask; checked only by `CheckOccupancy` when arg is not `-1`. | Conditional | `0x00586787..0x0058679D`, `0x0058681F..0x0058682B`, reservation lifecycle report |
| `CellClass+0xE4` | Ground object-list head scanned by `CheckOccupancy` helpers. | Yes | `0x0047C550`, `0x0047C520`; corrected thiscall note in validator report |
| `CellClass+0xE8` | Bridge/deck object-list head; not read by `CheckOccupancy`. | No direct use in this function | validator report absence plus bridge object-list report |
| `CellClass+0x124` | Ground occupation bitfield used by `CheckCellPassability`, not by `CheckOccupancy`. | Yes | `0x00483527..0x00483572` |
| `CellClass+0x128` | Bridge/deck occupation bitfield selected by bridge/height logic in `CheckCellPassability`, not by `CheckOccupancy`. | Conditional | `0x0048353C..0x0048354E` |
| `CellClass+0x140 bit 0x100` | Structural bridge flag used by passability height/layer selection and nearby bridge filtering; not read by `CheckOccupancy`. | Yes / Conditional | `0x004834FA..0x00483527`, `0x0056DE77..0x0056DE80` |

## 3. Core Logic

### 3.1 `CellRect__CheckPassability @ 0x0056E7C0`

Native contract:

```text
bool CheckPassability(
    CellStruct* top_left,
    int width,
    int height,
    int speed_type,
    int required_zone_id,
    int movement_zone,
    int required_height_or_level,
    int bridge_aware_zone_or_layer_arg,
    int reject_any_overlay
)
```

Material findings:

| Behavior | Active in YR | Evidence |
|---|---|---|
| Stack arity is nine 32-bit args (`RET 0x24`). | Yes | [CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md); full-arg decode report |
| Loop is rectangle-wide, x outer and y inner; signed `< width/height` means width/height <= 0 skips all cell checks and returns true. | Conditional on malformed caller dimensions | `0x0056E7CA..0x0056E87C` |
| Out-of-range cell lookups substitute the dummy cell; this wrapper has no final `MapClass__IsRectInPlayfield` call. | Yes | `0x0056E7FF..0x0056E832`; no xref/call to `0x00578390` in body |
| `reject_any_overlay != 0` rejects `CellClass+0x44 != -1` before `CheckCellPassability`. | Conditional on caller flag | `0x0056E832..0x0056E83E` |
| Per-cell passability is delegated to `CellClass__CheckCellPassability @ 0x004834A0` with `speed_type`, two zero occupation-mask modifiers, `required_zone_id`, `movement_zone`, `required_height_or_level`, and bridge/layer arg. | Yes | `0x0056E840..0x0056E859`; callee evidence `0x004834A0` |
| This function does not call `CheckOccupancy`, does not scan object lists, and does not read reservation bits. | Yes | full body `0x0056E7C0..0x0056E88B`; validator report xrefs |

`CellClass__CheckCellPassability @ 0x004834A0` sub-contract needed by this substrate:

| Behavior | Active in YR | Evidence |
|---|---|---|
| `speed_type == 4` immediately succeeds, skipping zone, height, occupation, wall-overlay, and speed-table checks. | Yes when Winged/Fly SpeedType is passed | `0x004834A7`, `0x004835FF` |
| If `required_zone_id != -1`, `MapClass__GetZoneID(cell, movement_zone, bridge_arg)` must equal it. | Conditional on real zone id | `0x004834BF..0x004834D8`; `0x0056D230` |
| `movement_zone` is the zone-map/matrix-row family; `speed_type` is separate and feeds the SpeedType/LandType table. | Yes | FNPC caller matrix; [ZONE_PASSABILITY_MATRIX_READERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_MATRIX_READERS_GHIDRA_REPORT.md); [SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md) |
| Explicit required height uses exact base level or bridge `level+4`; `-1` is unrestricted but still participates in bridge occupation-field selection. | Conditional; FNPC passes `-1` | `0x004834E1..0x00483527`; bridge occupancy report |
| Selected occupation byte is `+0x124` or `+0x128`; `CheckPassability` passes both ignore-mask flags as zero, so any selected remaining bit blocks. | Yes / Conditional on bridge selection | `0x00483527..0x00483572`; wrapper pushes zero args at `0x0056E854..0x0056E856` |
| Accepted wall-overlay cases force LandType to Clear before speed lookup; otherwise exact `g_SpeedType_LandType_Table[speed_type + LandType*9] == 0.0` rejects. | Conditional on wall overlay / non-bridge path | `0x00483583..0x004835F6`; speed-table report |

### 3.2 `CellRect__CheckOccupancy @ 0x00586780`

Native contract:

```text
bool CheckOccupancy(CellRect* rect, int reservation_layer_or_house_index)
```

Material findings:

| Behavior | Active in YR | Evidence |
|---|---|---|
| Stack arity is two args (`RET 0x8`). | Yes | [CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md) |
| `arg == -1` makes reservation mask zero; otherwise mask is `1 << (arg & 0x1F)`, computed once before scanning. Other negative values are not skips. | Yes / Conditional for non-`-1` callers | `0x00586787..0x0058679D`, `0x0058681F..0x0058682B` |
| Rectangle loop uses signed `< x+width` / `< y+height`; nonpositive dimensions skip blocker scan but still run final playfield check using `width-1` / `height-1` corners. | Conditional | `0x005867AC..0x00586880`; `0x00578390` |
| First blocker helper scans ground object list for RTTI `0x24`; corrected evidence says `FUN_0047C550` is `__thiscall`, with cell as implicit receiver and explicit arg `0`. | Yes | `0x00586812..0x0058681D`; `0x0047C550`; audit log 2026-05-28 |
| Reservation test rejects `(CellClass+0xDC & mask) != 0`, but FNPC calls with `-1` so ordinary nearby search skips reservations. | Conditional | `0x0058681F..0x0058682B`; FNPC callsites `0x0056DE9D`, `0x0056E0B3`, `0x0056E2F6`, `0x0056E4F8` |
| Requires `CellClass+0x44 == -1`, `+0x4C == 0`, and `+0x11C == 0`. | Yes | `0x0058682D..0x00586842` |
| Rejects if `Look_up_building_in_cell @ 0x0047C520` finds a `WhatAmI()==6` object on `CellClass+0xE4`. | Yes | `0x00586844..0x0058684D`; helper `0x0047C520` |
| On no blockers, returns `MapClass__IsRectInPlayfield(rect, 1)`, checking NW/NE/SW/SE corners with inclusive `x+width-1`, `y+height-1` extents. | Yes | `0x00586874..0x00586880`; `0x00578390` |
| Does not read `SpeedType`, `MovementZone`, LandType speed table, zone id, `+0x124`, `+0x128`, `+0xE8`, or `+0x140`. | Yes as negative fact | full body `0x00586780..0x00586893`; bridge object-list report |

Return semantics for both validators are boolean success: true/nonzero means the rectangle passed this validator; false/zero means a blocker, passability failure, or out-of-playfield result for `CheckOccupancy`.

## 4. INI Keys

Neither validator reads INI directly. Inputs are parsed and stored before the call.

| Key / data | Effect reaching the validators | Active in YR | Evidence |
|---|---|---|---|
| `SpeedType=` | Supplies `speed_type` to `CheckPassability`/`CheckCellPassability`; used by SpeedType/LandType table, not zone matrix rows. | Yes | `0x004834A0`; speed-table report; `ini/rulesmd.ini` content |
| `MovementZone=` | Supplies `movement_zone` / zone-map row family for zone IDs and matrix-built zone maps. | Yes | `0x0056D230`; zone matrix reader report; `ini/rulesmd.ini` content |
| Land speed table entries | Feed `g_SpeedType_LandType_Table[speed_type + LandType*9]`; exact zero blocks non-bridge selected path. | Yes | `0x004835DE`; speed-table report |
| `AIBaseSpacing=1` | Used by AI/site helper before `CheckOccupancy(expanded_rect, HouseClass+0x30)`, not by the validator itself. | Conditional | `FUN_005060B0 @ 0x00506694..0x005069DB`; `ini/rulesmd.ini:3132`, `ini/rules.ini:2602` |
| `Buildable=` | Not read by either CellRect validator; belongs to separate building-placement predicates. | No for this slice; Yes elsewhere | validator report; building placement predicate report |

## 5. Integration Points

| Caller / callee | Contract | Active in YR | Evidence |
|---|---|---|---|
| `FootClass__Find_Nearby_Passable_Cell @ 0x0056DC20` | Calls `CheckPassability` for candidates; optionally calls `CheckOccupancy(rect, -1)` when final occupancy flag is enabled. | Yes | `0x0056DE0E`, `0x0056E024`, `0x0056E265`, `0x0056E467`; occupancy callsites above |
| `FUN_005060B0` AI/site helper | Calls `CheckOccupancy(expanded_rect, HouseClass+0x30)`; also has alternate nearby-cell path. | Conditional in AI/site selection | `0x005069C0..0x005069DB`; callers from `BuildingClass__ExitObject_Main` and `HouseClass__AI_ChooseNextProduction` in validator report |
| `CellClass__CheckCellPassability @ 0x004834A0` | Owns passability's cell-local terrain/zone/height/occupation/speed decisions. | Yes | `0x0056E840..0x0056E859` call; body `0x004834A0` |
| `FUN_0047C550 @ 0x0047C550` | Ground-list RTTI `0x24` blocker scan for `CheckOccupancy`; cell is thiscall receiver. | Yes | `0x00586812..0x0058681D`; audit correction |
| `Look_up_building_in_cell @ 0x0047C520` | Ground-list first building object scan (`WhatAmI()==6`) for `CheckOccupancy`. | Yes | `0x00586844..0x0058684D`; supporting docs |
| A* main loop | Does not directly call these CellRect validators; uses Can_Enter_Cell virtuals. | No direct use | xref absence in validator report; `PATHFINDING_ASTAR_GHIDRA_REPORT.md` |

## 6. Current Rust Implementation Status

Current Rust has pieces, but not the native validator boundary:

| Rust surface | Observed status | Delta against contract |
|---|---|---|
| `src/sim/pathfinding/passability.rs:101` | Has a 13x8 matrix documented as MovementZone x reduced ZoneType, but still exposes compatibility `zone_layer_for_speed_type` and `is_passable_for_speed_type`. | Future CellRect passability must not collapse required-zone/MovementZone behavior into SpeedType row lookup. |
| `src/sim/pathfinding/zone_build.rs:428` | Terrain-aware zone rebuild uses `MovementZone` and `ResolvedTerrainCell.zone_type`. | Good substrate input for required-zone ID behavior, but not itself the per-call rectangle validator. |
| `src/sim/pathfinding/cell_entry.rs:119` and `:190` | Has native-shaped `CellEntryTerrainContext` and `CanEnterLayerContext` for Can_Enter_Cell split layers. | Useful but not a CellRect contract: FNPC passability has different args and no return-code tree. |
| `src/sim/occupancy.rs:92` | `OccupancyGrid` tracks dynamic occupants by movement layer and preserves list order. | It is not enough for `CheckOccupancy`: missing `+0xDC`, `+0x44`, `+0x4C`, `+0x11C`, dummy-cell scan behavior, and final playfield corners. |
| `src/sim/production/production_placement.rs:269` / `:363` | Building placement checks foundation cells, terrain/build blockers, bridge/slope, and structure overlap. | Separate placement predicate family; do not reuse as CellRect validator without preserving different blocker set and caller flags. |
| `src/sim/production/production_spawn.rs:201` / `:311` | Spawn selects preferred offsets and ring fallback using `spawn_cell_passable` and `cell_available_for_spawn`. | Does not expose binary FNPC `CheckPassability` + optional `CheckOccupancy(rect,-1)` split. |
| `src/sim/world/mod.rs:972` | Rebuilds `OccupancyGrid` after deserialization. | Covers dynamic object-list reconstruction only; not CellClass substrate fields or reservation bits. |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `CellRect__CheckPassability @ 0x0056E7C0` argument order and loop | verified | corrected validator report, full-arg decode report, `0x0056E7CA..0x0056E87C` | none for this contract |
| `CellClass__CheckCellPassability @ 0x004834A0` passability branches used by wrapper | verified | validator report, bridge object-list report, speed-table report | non-CellRect caller taxonomy out of scope |
| `CellRect__CheckOccupancy @ 0x00586780` blocker tree and return polarity | verified | corrected validator report, audit log entry, reservation lifecycle report | exact RTTI `0x24` class name deferred |
| FNPC validator call contract | verified for validator calls | xrefs/calls in validator report and caller matrix | full FNPC candidate ordering out of scope |
| AI/site `CheckOccupancy` reservation call | verified | `0x005069C0..0x005069DB`; reservation lifecycle report | complete `+0xDC` writer lifecycle out of scope |
| Rust passability/zone surfaces | touched-not-exhausted | local `rg` and file reads | implementation design deferred |
| Rust occupancy/substrate fields | touched-not-exhausted | local `rg` and file reads | substrate migration design deferred |
| Fresh live Ghidra MCP in this slot | deferred | no callable Ghidra tools exposed to this subagent; relied on corrected/audited Ghidra reports | parent can spot-check if desired |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-01 - What is the exact target question? -> The Rust substrate needs two separate CellRect validator contracts, not one generic occupancy/passability query.` (evidence: this report scope; validator report)
- `[RESOLVED] OQ-02 - What are the entry points? -> `0x0056E7C0` and `0x00586780`, with support `0x004834A0`, `0x0047C550`, `0x0047C520`, `0x00578390`.` (evidence: validator report)
- `[RESOLVED] OQ-03 - Is `CheckPassability` active in YR? -> Yes through `FootClass__Find_Nearby_Passable_Cell`.` (evidence: xrefs `0x0056DE0E`, `0x0056E024`, `0x0056E265`, `0x0056E467`)
- `[RESOLVED] OQ-04 - Is `CheckOccupancy` active in YR? -> Yes through FNPC and conditionally through AI/site helper.` (evidence: xrefs `0x0056DE9D`, `0x0056E0B3`, `0x0056E2F6`, `0x0056E4F8`, `0x005069DB`)
- `[RESOLVED] OQ-05 - Does passability include object-list occupancy? -> No; it only uses selected occupation bytes through `0x004834A0`.` (evidence: `0x00483527..0x00483572`; no call to `0x00586780`)
- `[RESOLVED] OQ-06 - Does occupancy include terrain passability? -> No SpeedType, MovementZone, LandType, zone id, or speed table read.` (evidence: full body `0x00586780..0x00586893`)
- `[RESOLVED] OQ-07 - What is `CheckOccupancy` arg2? -> Reservation bit index/house index; `-1` disables `+0xDC`.` (evidence: `0x00586787..0x0058679D`, `0x0058681F..0x0058682B`)
- `[RESOLVED] OQ-08 - Is `+0xDC` GapGen? -> No for checked code; GapGen uses `+0x78`, sensors use `+0x7C`.` (evidence: reservation lifecycle report)
- `[RESOLVED] OQ-09 - Does either validator read INI directly? -> No; SpeedType, MovementZone, AIBaseSpacing, and other inputs are upstream.` (evidence: validator bodies; INI grep)
- `[RESOLVED] OQ-10 - Are bounds semantics identical? -> No; `CheckPassability` lacks final playfield containment while `CheckOccupancy` calls `IsRectInPlayfield(rect,1)`.` (evidence: `0x0056E7C0..0x0056E88B`, `0x00586874..0x00586880`)
- `[RESOLVED] OQ-11 - Are zero/negative dimensions handled? -> Passability returns true after skipping checks; occupancy skips scan but still performs final playfield check.` (evidence: validator report)
- `[RESOLVED] OQ-12 - Should bridge object-list state feed `CheckOccupancy`? -> No direct `+0xE8`/`+0x128` read in this function.` (evidence: validator report; bridge object-list report)
- `[RESOLVED] OQ-13 - Which Rust files are current touchpoints? -> passability, zone_build, cell_entry, occupancy, production placement/spawn, world rebuild.` (evidence: local `rg` and file reads)
- `[RESOLVED] OQ-14 - What TS legacy gate controls these validators? -> No top-level TS/Fog gate found in the validated bodies; branches are content/caller-dependent rather than TS-disabled.` (evidence: corrected validator report)
- `[DEFERRED] OQ-15 - Exact class name behind RTTI `0x24`.` (category: out-of-scope; reason: blocker effect is sufficient for this CellRect contract; next-step-if-pursued: class RTTI enum audit)
- `[DEFERRED] OQ-16 - Complete writer lifecycle for `CellClass+0xDC`.` (category: requires-different-system-context; reason: read contract and negative GapGen fact are enough here; next-step-if-pursued: scenario/base-node/AI reservation writer audit)
- `[DEFERRED] OQ-17 - Exact Rust ownership/API names for the future substrate.` (category: out-of-scope; reason: this slot is research-only; next-step-if-pursued: implementation plan or verified-fix swarm)
- `[DEFERRED] OQ-18 - Fresh in-session decompiler re-read of every cited range.` (category: needs-runtime-debugger; reason: no callable Ghidra MCP endpoint was exposed to this subagent; next-step-if-pursued: parent spot-check `0x0056E7C0`, `0x00586780`, `0x004834A0`)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `CheckPassability` is a full-rectangle per-cell terrain/zone/height/occupation-byte validator with nine caller inputs and no final playfield check. | `0x0056E7C0`; `0x0056E840..0x0056E859`; `0x004834A0`; FNPC xrefs | Missing exact API; current Rust mostly combines `PathGrid`, zone helpers, and availability checks ad hoc | future CellClass substrate query, `src/sim/pathfinding/passability.rs`, `src/sim/pathfinding/zone_build.rs`, `src/sim/production/production_spawn.rs` | Add a substrate-backed rectangle passability query that threads SpeedType, required zone id, MovementZone, required height, bridge/layer arg, overlay reject, width, and height distinctly. | `cellrect_passability_uses_movement_zone_zone_id_and_speed_type_separately`: a candidate with matching SpeedType terrain but wrong required zone fails; same cell with `required_zone_id=-1` proceeds to speed/occupation checks. | Do not replace this with `PathGrid::is_walkable` or a SpeedType-derived matrix row. |
| `CheckOccupancy(rect,-1)` skips `CellClass+0xDC` but still rejects object-list blockers, `+0x44/+0x4C/+0x11C`, building lookup, and out-of-playfield rectangles. | `0x00586780`; `0x00586787..0x00586880`; FNPC occupancy callsites | Missing distinct validator; `OccupancyGrid` is dynamic occupants only | `src/sim/occupancy.rs`, future CellClass substrate fields, `src/sim/production/production_spawn.rs`, scatter/placement callers that route through FNPC | Add a rectangle occupancy/blocker validator separate from passability and from dynamic movement occupancy; preserve `-1` reservation skip. | `cellrect_occupancy_minus_one_skips_reservation_but_rejects_cell_blockers`: reservation-only rect passes with `-1`, but overlay/slope/building/out-of-playfield rejects. | Do not model `-1` as "skip occupancy"; it skips only reservation bits. |
| Non-`-1` `CheckOccupancy` arg checks `1 << (arg & 0x1F)` against `CellClass+0xDC`; AI/site helper passes `HouseClass+0x30`. | `0x00586787..0x0058679D`; `0x005069C0..0x005069DB`; reservation lifecycle report | No per-cell per-house reservation substrate identified | future AI/base site selection substrate, not normal FNPC | Keep base/site reservation bits as separate CellClass substrate state, not entity occupancy. | `cellrect_occupancy_house_reservation_blocks_same_house_only`: a cell with bit N blocks arg N, does not block `-1`, and aliases only through `arg & 0x1F`. | Do not merge `+0xDC` with GapGen, shroud, sensor, or movement reservations. |
| Bridge and layer inputs differ by validator: passability uses bridge flag/height to choose `+0x124` vs `+0x128`; occupancy does not read bridge list/bitfields. | `0x004834FA..0x00483572`; `0x00586780..0x00586893`; bridge object-list report | Current Rust `CanEnterLayerContext` can overfit this if reused directly | `src/sim/pathfinding/cell_entry.rs`, `src/sim/occupancy.rs`, future CellRect substrate | Expose selected occupation-byte behavior for passability without applying it to `CheckOccupancy`. | `cellrect_passability_bridge_occupation_bits_are_not_occupancy_rect_blockers`: bridge-deck occupation can fail passability while the same state alone is not a `CheckOccupancy` `+0xE8/+0x128` blocker. | Do not reuse the full Can_Enter_Cell layered object-list classifier as `CheckOccupancy`. |
| `Buildable=` and building placement predicates are not these validators. | no `Buildable` read in `0x0056E7C0`/`0x00586780`; separate placement reports | Rust production placement is a separate predicate family | `src/sim/production/production_placement.rs` | Keep building placement checks separate from CellRect FNPC validators unless a caller is verified to use the same binary path. | `cellrect_validators_do_not_require_buildable_flag`: terrain that is non-buildable but SpeedType-passable is evaluated by passability rules, not building placement rules. | Do not make `cell_placeable` the substrate implementation for FNPC passability/occupancy. |

### Negative Facts / Do Not Do

- Do not fuse `CheckPassability` and `CheckOccupancy`. Active in YR: Yes. Evidence: separate bodies/calls `0x0056E7C0` and `0x00586780`; FNPC gates occupancy separately.
- Do not derive passability's zone row from `SpeedType`. Active in YR: Yes. Evidence: `movement_zone` is the `MapClass__GetZoneID` row/source; SpeedType feeds `g_SpeedType_LandType_Table`.
- Do not make `CellClass+0xDC` dynamic occupancy or GapGen. Active in YR: Conditional for reservations, Yes as GapGen negative. Evidence: `0x00586787..0x0058682B`; GapGen writes `+0x78`, sensors `+0x7C`.
- Do not treat structural bridge flag `+0x140 bit 0x100` as blanket passability failure. Active in YR: Yes/Conditional. Evidence: `0x004834FA..0x00483572` uses it for height/layer/occupation selection.
- Do not make `CheckOccupancy` scan bridge/deck object list `+0xE8` or occupation bytes `+0x124/+0x128`. Active in YR: No direct use. Evidence: full body `0x00586780..0x00586893`; bridge object-list report.
- Do not use `Buildable=` or current `cell_placeable` as the FNPC CellRect validator. Active in YR: No for this slice. Evidence: no `Buildable` read in either validator; separate building-placement predicate docs.
- Do not assume both validators reject out-of-play rectangles the same way. Active in YR: Yes. Evidence: `CheckPassability` dummy-cell/no final bounds; `CheckOccupancy` final `0x00578390`.

### Stale Docs / Follow-up Docs

- `docs/research/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md`: no replacement needed after 2026-05-28 correction; keep the inline correction that `FUN_0047C550` is thiscall with cell as implicit receiver.
- `docs/research/FIND_NEARBY_PASSABLE_CELL_GHIDRA_REPORT.md`: replace any wording that calls `Cell+0xDC` `GapGenBitmask` with: "`CellClass+0xDC` is a per-house/base-placement reservation bitmask. `Find_Nearby_Passable_Cell` passes `-1` to `CellRect__CheckOccupancy`, so this reservation bitmask is skipped on that path; checked GapGen code writes `CellClass+0x78`, and sensor counts use `CellClass+0x7C`."
- `docs/research/PATHFINDING_ASTAR_GHIDRA_REPORT.md`: replace any `g_PassabilityMatrix[speed_type * 8 + ...]` wording with: "`ZonePassabilityMatrix` rows are `MovementZone`/reduced-zone rows; `SpeedType` belongs to the SpeedType/LandType speed table used by `CellClass__CheckCellPassability`."
- `docs/research/PATHFINDING_ASTAR_GHIDRA_REPORT.md`: replace "`CellRect__CheckOccupancy -- no other units blocking`" with: "`CellRect__CheckOccupancy @ 0x00586780` rejects ground-list RTTI `0x24`, optional `CellClass+0xDC` reservation bits, `CellClass+0x44/+0x4C/+0x11C`, ground-list building objects, and out-of-playfield rectangles; in FNPC the reservation arg is `-1`, so `+0xDC` is skipped."

## 10. Remaining Uncertainty

- Exact class name for RTTI `0x24` in `FUN_0047C550` remains deferred; blocker effect and ground-list use are enough for this contract.
- Complete live writer/clearer lifecycle for `CellClass+0xDC` remains deferred; constructor clear, resize preserve, readers, and GapGen negative fact are already covered by the reservation lifecycle report.
- This subagent did not receive callable Ghidra MCP tools. Load-bearing binary claims are therefore consolidated from the corrected/audited Ghidra reports listed below, especially the 2026-05-28 audit-confirmed validator report.
- Future Rust API ownership (`CellClass` substrate struct vs helper contexts) remains an implementation-design decision. This report only fixes the behavioral contract that any design must preserve.

## Sources

- `docs/research/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md`
- `docs/research/CELLRECT_CHECKPASSABILITY_0056E7C0_FULL_ARG_DECODE_GHIDRA_REPORT.md`
- [docs/research/CELLRECT_CHECKOCCUPANCY_00586780_FULL_BLOCKER_TREE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLRECT_CHECKOCCUPANCY_00586780_FULL_BLOCKER_TREE_GHIDRA_REPORT.md)
- [docs/research/CELLCLASS_0XDC_RESERVATION_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_0XDC_RESERVATION_LIFECYCLE_GHIDRA_REPORT.md)
- `docs/research/bridges/02-cell-state-layering-zones/BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md`
- [docs/research/AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) 2026-05-28 validator audit entry
- Prior evidence addresses cited by those reports: `0x0056E7C0`, `0x00586780`, `0x004834A0`, `0x0047C550`, `0x0047C520`, `0x00578390`, `0x0056DC20`, `0x005060B0`, `0x0056D230`
- INI checked: `ini/rulesmd.ini`, `ini/rules.ini`
- Rust scan: `src/sim/pathfinding/passability.rs`, `src/sim/pathfinding/zone_build.rs`, `src/sim/pathfinding/cell_entry.rs`, `src/sim/occupancy.rs`, `src/sim/production/production_placement.rs`, `src/sim/production/production_spawn.rs`, `src/sim/world/mod.rs`
