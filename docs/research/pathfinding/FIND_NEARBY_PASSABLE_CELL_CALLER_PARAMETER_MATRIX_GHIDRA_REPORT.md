# Find Nearby Passable Cell Caller Parameter Matrix - Ghidra Report

**Target:** `FootClass::Find_Nearby_Passable_Cell @ 0x0056DC20`  
**Mode:** `/re-investigate` exhaustive-slice, scoped to caller parameter provenance.  
**Status:** PARTIAL: validator-facing parameter classes are covered; exact stack decode remains unresolved for a few very large/collapsed direct callers listed in Remaining Uncertainty.  
**Non-scope:** core spiral/ring search algorithm and mission-specific state machines.

## Executive Summary

Active in YR: Yes. Ghidra xrefs show standard-YR direct callers of `0x0056DC20`, and the function returns with `RET 0x3c`, proving 15 stack parameters after `this/out`. Decompiler output sometimes suppresses trailing zero arguments; rows below use decompiled constants plus call convention where trailing arguments are zero.

Active in YR: Yes. Callers configure the already-documented `CellRect` validators through speed type, zone id, MovementZone, bridge-aware flag, rect width/height, reject-any-overlay flag, and final occupancy flag. They do not configure validator required height/layer: every internal `CellRect::CheckPassability` call receives `-1`.

Active in YR: Yes. Final occupancy, when enabled by the caller, calls `CellRect::CheckOccupancy(rect, -1)`. That skips `Cell+0xDC` reservation checks; callers cannot enable reservation-layer filtering through `Find_Nearby_Passable_Cell`.

Active in YR: Yes. Most standard callsites use `1x1`, `reject_any_overlay=0`, `check_occupancy=0`, and `allow_bridge=1`. Larger rects are restricted to rally/site/start/production placement-like fallbacks. Overlay rejection is rare and was verified in slave deploy / death-parachute style rows, not as a global FNPC behavior.

## Verified Contract Inputs

| Parameter family | Active in YR | Verified behavior | Evidence |
|---|---:|---|---|
| Stack arity | Yes | `RET 0x3c` means 15 caller stack args, so omitted trailing zeros in pseudocode are display artifacts. | `0x0056E797/0x0056E7B3` return sequence. |
| Required height/layer | Yes | Not caller-controlled; FNPC passes `-1` to all `CellRect::CheckPassability` invocations. | FNPC calls at `0x0056DE0E`, `0x0056E024`, `0x0056E265`, `0x0056E467`. |
| Occupancy reservation layer | Yes | If caller enables final occupancy, FNPC passes `CellRect::CheckOccupancy(rect, -1)`, skipping `Cell+0xDC`. | FNPC internal call plus [CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md). |
| Search radius | Yes | Not caller-controlled; derived from the foot object's fields and capped at 32. | FNPC body already documented; no caller arg exists in 15-arg contract. |
| Target selection | Yes | Null/zero target means random/direct-list selection; non-null target means closest-to-target selection. | FNPC param 14 and callsites passing stack target cells or zero locals. |

## Caller Parameter Matrix

Notation: `H/L` is validator required height/layer; it is always `-1` through FNPC. `height` means FNPC origin/candidate +/-2 height gate. `occ-safety` is the per-cell occupant safety flag. `final-occ` is the `CellRect::CheckOccupancy(rect,-1)` flag.

| Callsite group | Active in YR | SpeedType | Zone id | MovementZone | H/L | Bridge-aware | Rect | Reject overlay | Other flags | Target mode | Evidence |
|---|---:|---|---|---|---|---|---|---|---|---|---|
| Common unit/object fallback: `BuildingClass::ReleaseDockedHarvester`, `FUN_0044D880`, `FUN_00458A80`, `FUN_0065E010`, chrono/warp helpers, team move/production scripts | Conditional/Yes by caller | Usually object `Type+0x67C`; harvester fallback verified `Type+0x67C`; some chrono rows `1` | `-1` | `0` | `-1` | `0` | `1x1` | `0` | `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | Usually null/random | `0x004597E3` assembly for docked harvester; decompiled `FUN_0044D880`, `FUN_00458A80`, `ChronoSphere__WarpUnitsAtCell`, team helpers. |
| Building rally point: `BuildingClass::SetRallyPoint` | Yes | `0`, or `4` for air-like, or `6` for naval-like building type cases | `MapClass::GetZoneID(building cell, movement_zone, target bridge flag)` | `0`, `9`, or `4` | `-1` | target cell bridge flag | `1x1` | `0` | `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | target/local null, selection by caller-provided rally target when present | Decompiled `0x00443860` constants and type offset branches. |
| House/base rally helpers: `HouseClass::Set_Rally_Point_Cell`, `HouseClass::AI_GroundRallyPoint`, `HouseClass::Recalc_Base_Center`, `FUN_0050C920` | Yes/Conditional | `1`, `0`, `0`, `0` respectively | mostly `-1` | `0` | `-1` | `0` | `1x1`, `5x5`, `1x1`, `4x4` | `0` | `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | null/random; AI ground rally offsets result by `+2,+2` | Decompiled `0x004FBF60`, `0x00509CD0`, `0x004FD150`, `0x0050C920`. |
| Building completion free unit / initial rally support: `BuildingClass::OnConstructionComplete` | Conditional | `5` for guard/rally support row; `2` for free-unit fallback row | first row `-1`; free-unit row uses `MapClass::GetZoneID(..., unit MovementZone, 0)` | `10` for first row; unit `Type+0x5B4` for free-unit row | `-1` | `0` | first row `5x5`; free-unit row `1x1` | first row `0`; free-unit first retry `1`, second retry `0` | first row `allow_bridge=1`; free-unit retry uses `height=1`, `occ-safety` first `0/1` pattern from constants, `final-occ=0` | target stack/local; then placement retry | Decompiled `BuildingClass__OnConstructionComplete` direct FNPC calls near `0x00446948` and `0x00446e..`. |
| AI/site placement helper: `FUN_005060B0` | Yes | `5` | `-1` | `0` | `-1` | `0` | `foundation_width+2` by `foundation_height+2` | `0` | `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | null/random | Decompiled `0x005060B0`; note it separately calls `CellRect::CheckOccupancy` directly outside FNPC. |
| Map/start/crate/wall overlay rows: `ScenarioClass::Gather_Start_Positions`, `MapClass::PlaceCrateAtRandomCell`, `WallOverlay_HeightAdjust` | Conditional | start `1`; crate/wall `5` on water else `1` | `-1` | `0` | `-1` | `0` | start `8x8`; crate/wall `1x1` | `0` | `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | null/random except wall passes zero local target | Decompiled `0x00688380`, `0x0056BD40`, `WallOverlay_HeightAdjust`. |
| Aircraft/fly fallback rows: `AircraftClass::Find_Nearest_Friendly_Airfield`, `FUN_0065E850`, `FlyLocomotionClass::{Descent_Step,Emergency_Relocate}` | Yes/Conditional | airfield `0`; drop `4`; fly descent/emergency `1` | airfield/object rows use `MapClass::GetZoneID(...,0,0)`; others `-1` | airfield/drop `0`; fly descent/emergency `9` | `-1` | `0` | airfield `3x3` or `1x1`; others `1x1` | `0` | mostly `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | null/random | Decompiled `0x0041A160`, `0x0065E850`, `0x004CE840`, `0x004CCFD0`. |
| Command/path/target correction: `FUN_004DC8C0`, `FUN_004DE1D0`, `FootClass::ClickedAction_Object`, `FootClass::Mission_Patrol`, `TechnoClass::Set_Destination`, `FUN_007447B0` | Yes/Conditional | object `Type+0x67C`, sometimes fixed `1` in bridge helper | computed zone via `MapClass::GetZoneID(...)` or `-1` on special branches | object `Type+0x5B4` adjusted in some branches (`6->0`, `9->7/0` style branches) | `-1` | current/target cell bridge flag or `0/1` constants | `1x1` | `0` | commonly `height=1`; `occ-safety` varies `0/1`; `allow_bridge` usually `1`, a few helper rows pass `0`; `final-occ=0` | null/random or closest-to-target stack cell | Decompiled functions show dynamic zone/movement and constants; `FUN_007447B0` has multiple bridge variants. |
| Scatter rows: `InfantryClass::Scatter`, `UnitClass::Scatter` | Yes | object `Type+0x67C` | `-1` | `0` | `-1` | current `OnBridge`/`this+0x23` | `1x1` | `0` | `height=1`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | non-null stack target / zero local depending branch | Decompiled `InfantryClass__Scatter`; `UnitClass__Scatter` call has `..., Type+0x67C, -1,0,(char)this+0x23,1,1,0,1,0,1,target,0,0`. |
| Hover/deploy/unload rows: `UnitClass::Mission_Deploy_Building`, `UnitClass::Mission_Harvest` | Conditional | water/hover deploy row `2`; passenger fallback uses passenger `Type+0x67C`; harvest fallback `2` | `-1` | `0` | `-1` | `0` | `1x1` | `0` | `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | mostly null/random | Decompiled `UnitClass__Mission_Deploy_Building`; prior harvester mission decompile. |
| Overlay-reject deploy rows: `SlaveManagerClass::FindDeployCell`; death parachute/infantry spill in `UnitClass::ReceiveDamage` | Conditional | slave `1`; death/spill `1` | slave `MapClass::GetZoneID(owner/building cell,0,0)`; death `-1` | `0` | `-1` | `0` | slave foundation `WxH`; death `1x1` | `1` | slave `height=0`, `occ-safety=0`, `allow_bridge=0`, `final-occ=0`; death `height=0`, `occ-safety=0`, `allow_bridge=1`, `final-occ=0` | target stack/local, not pure random | Decompiled `SlaveManagerClass__FindDeployCell`; `UnitClass__ReceiveDamage` call sets `uVar16=1` in reject-overlay slot. |
| Teleport locomotion rows: `TeleportLocomotionClass::Process`, `TeleportLocomotionClass::Update_Position`, `FUN_00729580` | Conditional | usually object `Type+0x67C` or fixed `1` | computed zone from current/target/bridge context; fallback can be `-1` | object `Type+0x5B4` adjusted (`9/2->0`, `3->5` in update branch); fallback row `6` in helper | `-1` | bridge flag from cell/context or `0` | `1x1` | `0` | mix of `height=0/1`, `occ-safety=0/1`, `allow_bridge=1`, `final-occ=0` | closest-to-target stack cell in bridge/teleport correction branches | Decompiled `TeleportLocomotionClass__Process`, `Update_Position`, and prior `FUN_00729580` decode. |
| Threat scan fallback: `FootClass::Greatest_Threat_Scan` | Conditional | object `Type+0x67C` | computed dynamic zone | dynamic MovementZone | `-1` | dynamic bridge/context | `1x1` | `0` | dynamic helper row; exact pseudocode variable mapping collapsed | closest-to-target/current threat cell | Decompiled `FootClass__Greatest_Threat_Scan`; FNPC call near end has dynamic locals from `MapClass::GetZoneID` and `Type+0x67C`. |
| Superweapon bridge/surface adjustments: `SuperClass::Launch` | Conditional | fixed `0` in decoded case rows | `-1` | `0` | `-1` | `0` | `1x1` | `0` | trailing zero flags inferred from `RET 0x3c` and decompiler omission | null/random | Batch decompile showed compact calls in launch cases; exact full stack decode remains uncertain. |

## Direct Caller Inventory

Active in YR: Yes/Conditional. `get_function_callers(0x0056DC20)` returned live standard executable callsites including aircraft airfield search, building exit/completion/rally/release, house rally/base helpers, map crate/start helpers, chrono/teleport helpers, fly locomotion helpers, foot command/path/threat/guard/patrol helpers, infantry/unit scatter, slave deploy, super launch, convoy scripts, and wall overlay adjustment. Some callers are conditional on normal gameplay features (AI scripts, superweapons, crates, slave manager, teleport locomotion, dock/release state).

## Negative Facts / Do Not Do

Active in YR: Yes. Do not add a caller-facing required-height/layer argument to the FNPC Rust API; FNPC always supplies `-1` to `CheckPassability`.

Active in YR: Yes. Do not model FNPC final occupancy as checking `Cell+0xDC`; FNPC passes `CheckOccupancy(rect,-1)`, and the validator report verifies that this skips the reservation mask.

Active in YR: Yes. Do not make search radius a caller parameter; callers provide rect dimensions, target mode, and validator flags, while radius is internal to FNPC.

Active in YR: Yes. Do not globally reject overlays in nearby-passable fallback. Most decoded rows pass `reject_any_overlay=0`; overlay rejection is caller-specific (`SlaveManagerClass::FindDeployCell`, some spill/parachute rows).

Active in YR: Yes. Do not call `param_13` "reject bridge" in Rust. The observed branch is allow-bridge polarity: nonzero allows bridge structural cells; zero rejects them.

## Implementation Handoff

- Verified behavior: FNPC callers configure a compact parameter object: `speed_type`, `zone_id`, `movement_zone`, `bridge_aware`, `rect_w/h`, `reject_any_overlay`, `height_check`, `occupant_safety_check`, `allow_bridge_cells`, `target_mode`, and `final_occupancy_check`. Rust delta: consolidate production spawn, aircraft drop, miner/refinery release, chrono/teleport, scatter, and path correction fallbacks onto one typed config. Affected surface: `sim` nearby-passable helpers and tests. Acceptance scenario: a `5x5` rally/site search and a `1x1` unit fallback choose different candidates from the same origin because the rect dims differ. Proposed test name: `nearby_passable_respects_caller_rect_and_target_mode`. Risk: Medium.
- Verified behavior: required validator height/layer and reservation layer are not caller-configurable through FNPC (`-1` in both passability and final occupancy paths). Rust delta: keep these as internal constants for FNPC, not public config fields. Affected surface: occupancy/pathfinding parity. Acceptance scenario: enabling FNPC final occupancy rejects real object/cell blockers but ignores a synthetic reservation-only `Cell+0xDC` marker. Proposed test name: `find_nearby_passable_final_occupancy_ignores_reservation_layer`. Risk: High if modeled incorrectly.
- Verified behavior: overlay rejection is caller-specific. Rust delta: default `reject_any_overlay=false`, with explicit true rows for slave deploy / verified overlay-reject spill placement only. Affected surface: slave deployment, infantry spill/parachute placement, production fallback. Acceptance scenario: a normal unit scatter may select an overlaid otherwise-passable cell, while slave deploy with the same origin skips it. Proposed test name: `slave_deploy_nearby_rejects_overlay_candidates`. Risk: Medium.

## Stale Doc Replacement Wording

Doc path: `docs/research/FIND_NEARBY_PASSABLE_CELL_GHIDRA_REPORT.md`

Replace the parameter table wording for `param_6`:

> `param_6` is the MovementZone / zone-matrix row selector used by `CellRect::CheckPassability` and related zone checks. It is not a generic "locomotor type"; SpeedType is already `param_4`.

Replace `param_13` wording:

> `param_13` is `allow_bridge_cells`: nonzero allows bridge structural cells, zero rejects cells with `CellClass+0x140 & 0x100`.

Replace the `CellRect::CheckOccupancy` summary:

> FNPC calls `CellRect::CheckOccupancy(rect, -1)` only when `param_16` is true. With `-1`, the validator skips `Cell+0xDC` reservation filtering and checks the other object/cell/playfield blockers documented in [CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md).

## Remaining Uncertainty

- Exact full 15-argument decode remains open for a few decompiler-collapsed direct callers: `BuildingClass::ExitObject_Main`, `FootClass::Find_Path`, some `SuperClass::Launch` cases, and two convoy target variants. Their decoded rows match existing parameter classes but should not be used as unique Rust test fixtures until assembly-push order is walked.
- `FootClass::Greatest_Threat_Scan` has a verified live FNPC call with dynamic zone/movement/bridge context, but the decompiler variable map near the call is too tangled for a clean per-argument row without assembly cleanup.
- `BuildingClass::OnConstructionComplete` free-unit retry rows are verified as direct FNPC users, but the exact semantic names for the two retry flag combinations should be validated against the local construction/free-unit path before writing high-level gameplay docs.

## Open Questions Log

OPEN: Do the unresolved giant direct callers introduce any new parameter combination beyond the rows above? Current evidence suggests no, but stack-walk verification is still needed for complete caller-by-caller audit.

## Evidence Index

- `FootClass::Find_Nearby_Passable_Cell @ 0x0056DC20`, `RET 0x3c`; internal `CheckPassability(...,-1,...)`; internal `CheckOccupancy(rect,-1)`.
- `CellRect::CheckPassability @ 0x0056E7C0`, `CellRect::CheckOccupancy @ 0x00586780`; details from validator report.
- Direct Ghidra callers from `get_function_callers(0x0056DC20)`.
- Decompiled callers: `BuildingClass__ReleaseDockedHarvester`, `BuildingClass__SetRallyPoint`, `BuildingClass__OnConstructionComplete`, `HouseClass__Set_Rally_Point_Cell`, `HouseClass__AI_GroundRallyPoint`, `HouseClass__Recalc_Base_Center`, `FUN_005060B0`, `MapClass__PlaceCrateAtRandomCell`, `ScenarioClass__Gather_Start_Positions`, `WallOverlay_HeightAdjust`, `FlyLocomotionClass__Descent_Step`, `FlyLocomotionClass__Emergency_Relocate`, `AircraftClass__Find_Nearest_Friendly_Airfield`, `InfantryClass__Scatter`, `UnitClass__Scatter`, `UnitClass__Mission_Deploy_Building`, `UnitClass__ReceiveDamage`, `TeleportLocomotionClass__Process`, `TeleportLocomotionClass__Update_Position`, `SlaveManagerClass__FindDeployCell`, convoy/script helpers, and chrono helpers.
