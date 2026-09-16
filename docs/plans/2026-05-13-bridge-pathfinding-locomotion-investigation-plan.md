# Bridge Pathfinding & Locomotion (Complete Domain) — Investigation Plan

> **For Claude:** This plan scopes a comprehensive `/re-investigate` pass across the entire bridge ↔ movement domain in gamemd.exe. The scope is **Very Large** (~45 investigation items) and **MUST be executed in batched phases**, not single-session. Each phase ends with a checkpoint; if a phase reveals scope drift, the plan is revised before continuing.

**Topic:** Every function, struct field, and code path where bridges interact with pathfinding, movement layers, locomotor logic, and zone connectivity in gamemd.exe (Yuri's Revenge).
**Scope Size:** **Very Large** — 45 items, 7 phases, ~5–6 working days of `/re-investigate` work
**Est. Effort:** ~30–40 hours total at the depths called out below; ~15–30 min per FULL function, ~5–10 min per MEDIUM, ~2–5 min per LIGHT
**Prior Research:** 30+ docs in `docs/research/` plus 2026-05-11 and 2026-05-12 disparity scans (see §2). One stale doc identified.
**Expected Output:** Per-item research doc in `docs/research/` per item, PLUS the synthesis `BRIDGE_PATHFINDING_LOCOMOTION_OVERVIEW.md` after all items close. Plus `/verify-doc` of [PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md).
**Next Pipeline Step:** Synthesis doc → `/brainstorm` for any divergence found; existing implementation work continues unblocked.

---

## 0. Reading Order

The executor should:
1. Read this plan in full before starting Phase 1.
2. Read the two recent disparity scans (`docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md` and `docs/gap-scans/2026-05-12-disparity-scan-bridges.md`) — these are the most up-to-date statements of what's verified vs unverified.
3. Treat all prior research docs as **starting points**, not ground truth. The user's directive is explicit: re-verify every binary claim fresh. Prior audit findings are leads, not facts.

---

## 1. Goal

Produce a binary-verified, complete research record for bridge pathfinding and locomotion in gamemd.exe so the Rust port can:

- Match every observable bridge-crossing behavior (entry, exit, height bump, layer choice, A* legality, zone connectivity, occupancy split, layer-aware AoE) tick-for-tick and pixel-for-pixel against the original.
- Distinguish TS-legacy code paths (TunnelLocomotion, fog-gated branches) from live YR paths and not implement the dead surface.
- Identify all parity divergences between the binary and the current Rust implementation, ranked by player-visibility × trigger frequency.

The deliverable must answer, with binary evidence (Ghidra address + decompilation snippet or `read_memory` byte dump):

1. What is the complete set of code paths that consult bridge state during movement and pathfinding?
2. Which fields of CellClass, PathfinderClass, MapClass, FootClass, and each LocomotionClass subclass are read or written during bridge interactions, with exact offsets and types?
3. What formulas, constants, and bit-flags gate every bridge-related decision, with verified literal values from `.rdata` and confirmed callers?
4. Which paths are reachable in a normal YR skirmish (live), which are TS-inherited dead branches, and what gates separate them?
5. Where does the current Rust implementation diverge from the verified binary behavior, and at what player-visible severity?

---

## 2. Prior Research Inventory

### Bridge core (read first)

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| `BRIDGE_SYSTEM.md` | Cell flags, height arithmetic, dual occupancy, damage states overview, A* multipliers, CheckBridgeTraversal, AoE routing, TooBigToFitUnderBridge, g_BridgeZ_Offset_Ship, tunnel notes | HIGH | None internal |
| `BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md` | A* bridge gates, diff-1 SlopeIndex, two-pass Can_Enter_Cell, cost multipliers (0x42ACF0), zone precheck, bridge audio | HIGH w/ deferred items | **STALE: `CliffBackImpassability` claim is wrong** — it IS implemented in Rust per 2026-05-12 audit |
| `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` | 18-state damage machine, NS/EW axis, 4-path dispatcher, BridgeStrength RNG | HIGH | Bridgehead 4-step is open |
| [BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md) | Repair hut dispatch, walker spawn, overlay reverse, EVA cue | HIGH | Entire pipeline missing from Rust |
| [BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md) | Lifecycle post-load, path-grid refresh, zone graph updates, endpoint-active flags | HIGH | None |
| `LAT_RETRIGGER_AND_BRIDGE_DAMAGE_VARIANT_GHIDRA_REPORT.md` | ToggleBridgePavement bit 0x2000, 8-neighbor flood, TMP +0x24 bit 0x04 | HIGH | Damaged-variant flag missing from Rust |
| [PHASE_F_BRIDGE_DAMAGE_DISPATCH_VERIFICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/PHASE_F_BRIDGE_DAMAGE_DISPATCH_VERIFICATION.md) | Damage dispatcher 4-path verification | HIGH | None |
| `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` | Damage-state → variant lookup, animation sequencing | HIGH | Render-only |

### Pathfinding core

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| `PATHFINDING_ASTAR_GHIDRA_REPORT.md` | Find_Path orchestrator (0x42c900), AStar main loop (0x429a90), node expansion, edge cost, neighbor walkability | HIGH | Hierarchical detail; bridge-specific cost shaping |
| [PATHFINDERCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATHFINDERCLASS_GHIDRA_REPORT.md) | Singleton, heaps, zone vtable, UpdateBridgePassability @ 0x42ACF0 | HIGH | None |
| `PATHFINDING_CELL_ENTRY_VERIFICATION_REPORT.md` | Can_Enter_Cell vtable integration | MEDIUM | Two-pass Phase-6 bridgehead override re-reads cell+0x128 — open |
| [PATHFINDING_STANDALONE_FUNCTIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATHFINDING_STANDALONE_FUNCTIONS_GHIDRA_REPORT.md) | Zone system, neighbor walkers, path cost table | HIGH | None |
| [UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md) | Phases 1–12, return codes 0–7, bridge pre-check (Phase 1), crushable logic | HIGH (2026-05-12 corrections applied) | Phase-6 two-pass open |
| [PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md) | Path smoothing, speed ramping | HIGH | **Will be /verify-doc target in STEP 3** |

### Cell state & zones

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) | Cell layout, dual lists (+0xE4/+0xE8), zone fields, bridge height arithmetic | HIGH | Prior audit found inverted ternary in perpendicular-walk — verify GetZoneID fresh |
| [CELL_OCCUPATION_MARKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELL_OCCUPATION_MARKING_GHIDRA_REPORT.md) | Occupancy set/clear in ground + bridge lists | HIGH | None |
| [TIMER_CLASSES_AND_ZONE_MAP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIMER_CLASSES_AND_ZONE_MAP_GHIDRA_REPORT.md) | Zone connectivity, bridge marking (0x100000/0x200000) | HIGH | None |
| [ZONE_INCREMENTAL_DIVERGENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ZONE_INCREMENTAL_DIVERGENCE_GHIDRA_REPORT.md) | Incremental updates, bridge spawn/destroy propagation | MEDIUM | `bridge_kind` field missing in Rust |
| [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md) | Zone passability truth table | HIGH | None |

### Locomotion

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| `FOOTCLASS_PATHFINDING_AND_MOVEMENT.md` | FootClass layout, NavCom, path lifecycle | HIGH | None |
| [DRIVE_LOCOMOTION_CLASS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_LOCOMOTION_CLASS.md) | Drive COM object, Process_Movement, Process_Drive_Track, bridge transition flag | HIGH | Reactive height heuristic in Rust vs binary's planned-step layer |
| [DRIVE_TRACK_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_TRACK_SYSTEM.md) | TurnTrack[72], RawTrack[16], sub-step interp | HIGH | None |
| [DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md) | Process_Drive_Track @ 0x4b26b0, ramp detection asm sites | HIGH | None |
| [DRIVE_SHARP_TURN_FALLBACK_RE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_SHARP_TURN_FALLBACK_RE.md) | Sharp-turn recovery | MEDIUM | None |
| `JUMPJET_LOCOMOTION_CLASS_GHIDRA_REPORT.md` | JumpJet locomotor; bridge passability | HIGH | Bridge interaction not explicit — re-verify in this plan |
| `HOVER_LOCOMOTION_CLASS_GHIDRA_REPORT.md` | Hover locomotor | HIGH | Bridge interaction not explicit — re-verify |
| [TELEPORT_LOCOMOTION_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TELEPORT_LOCOMOTION_DEEP_DIVE.md) | Chrono locomotor | HIGH | None |
| [SHIP_VS_DRIVE_LOCOMOTION_COMPARISON.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHIP_VS_DRIVE_LOCOMOTION_COMPARISON.md) | 95% identical, 6 concrete differences (bridge Z-offset is one) | HIGH | None |
| [AIRCRAFTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AIRCRAFTCLASS_GHIDRA_REPORT.md) | Aircraft pathfinding, bridge clearance | HIGH | `FlyBridgeHeight` clearance not implemented; INI key not parsed |
| [NAVAL_ZONE_LEGALITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/NAVAL_ZONE_LEGALITY_GHIDRA_REPORT.md) | Ship passability, LowBridge water cells (LandType=Tunnel(10)) | HIGH | None |
| [NAVAL_SYSTEM_RESEARCH.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVAL_SYSTEM_RESEARCH.md) | Naval movement | MEDIUM | g_BridgeZ_Offset_Ship init not decomp'd |
| `TOO_BIG_TO_FIT_UNDER_BRIDGE_GHIDRA_REPORT.md` | Semantic question (eviction vs nav-block) | MEDIUM | Open |
| [LOCOMOTION_MATH_AND_CONSTANTS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOCOMOTION_MATH_AND_CONSTANTS.md) | Bridge Z offsets, height constants | HIGH | None |

### Recent disparity scans (in-repo, treat as live spec)

| Doc | Date | Key findings |
|-----|------|--------------|
| `docs/gap-scans/2026-05-12-disparity-scan-bridges.md` | 2026-05-12 | ~85% parity. Missing: repair (CRITICAL), audio, bridgehead 4-step, pavement variant, SlopeIndex gate, two-pass Can_Enter_Cell |
| `docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md` | 2026-05-11 | 8 confirmed gaps: AoE-layer (HIGH), drive reactive heuristic, A* bridgehead gate, height-diff 2/3 acceptance, cost multipliers, layer-aware occupancy pre-decision, TooBigToFitUnderBridge semantics, bridge_kind missing |
| `docs/gap-scans/2026-05-08-disparity-scan-pathfinding.md` | 2026-05-08 | General pathfinding parity |

### Cross-doc contradictions surfaced

1. **`BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md` is stale**: it claims `CliffBackImpassability` is NOT implemented in Rust, but it IS at `src/app_init.rs:308-319` and consumed at `src/map/resolved_terrain.rs:809-858`. The synthesis doc must note this and recommend update.
2. **[CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md)** — prior audit flagged an inverted ternary in `GetZoneID @ 0x56D230` perpendicular-walk direction. Item #16 in this plan re-verifies fresh.
3. **Cost constant `0x7E37B8` (10.0f)** is reused by damage code (`Apply_area_damage`, `WarheadTypeClass__Detonate`). Naming it `AStar_Cost_10` in Ghidra would mislead. Item #7 notes this.

---

## 3. Function Inventory

**Phase definitions:**

- **Phase 1 (Core)** — Pathfinder + A* spine + dual closed-list. The skeleton everything hangs on. **Checkpoint required before Phase 2.**
- **Phase 2 (Cell-Layer)** — Can_Enter_Cell pipeline across class hierarchy, CheckBridgeTraversal, cell-state offsets.
- **Phase 3 (Locomotor)** — Per-locomotor bridge handling for live YR locomotors.
- **Phase 4 (Zones)** — UpdateBridgeZones / FloodFill / GetZoneID / Can_Reach_Zone / BridgeRecord lifecycle.
- **Phase 5 (Edge Cases)** — Path invalidation on collapse, formation pathing, repair-truck routing, AI attack-on-bridge, passenger unload, aircraft landing near bridges.
- **Phase 6 (TS-Legacy Justification)** — Justify non-coverage of TunnelLocomotion, ParachuteLocomotion, FloatLocomotion (not YR locomotor classes).
- **Phase 7 (Synthesis)** — Cross-reference, divergence list, OVERVIEW doc.

### Phase 1 — Pathfinder Core (Checkpoint before Phase 2)

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 1 | 1 | `0x429A90` | `AStar_main_loop` | A* per-step iteration core. Reads dual closed-list, calls UpdateBridgePassability, applies cost multipliers. | FULL | Low |
| 2 | 1 | `0x42C900` | `AStar_pathfind_search` | Outer orchestrator; sets up open/closed; reads PathfinderClass fields. | FULL | Low |
| 3 | 1 | `0x42ACF0` | `PathfinderClass__UpdateBridgePassability` | Toggles 0x40000 around tube/bridge records. Reads cell+0x124, walks cell+0xE4/+0xE8 lists. **Two-loop body must be decomp'd in full.** | FULL | Low — flag 0x40000 use confirmed |
| 4 | 1 | `0x42C290` | `Zone_precheck` | 3-tier hierarchical zone fast-path. Reads MapClass+0x40..+0x6C, uses g_PassabilityMatrix and slope cost. **Must extract every branch** (prior audit found an inlined heap-sift conflated with TS-branch). | FULL | Low |
| 5 | 1 | `0x429830` | `AStar_compute_edge_cost` | Reads the 3 cost-multiplier constants at 0x7E37B4/B8/BC. **Verify every read site and what condition selects each.** | FULL | Low |
| 6 | 1 | n/a (data) | `0x7E37B4` / `0x7E37B8` / `0x7E37BC` | `read_memory` to confirm float values 2.0/10.0/4.0; xref both directions. **Document that 0x7E37B8 (10.0f) is shared with damage code** — do NOT label as pathfinding-exclusive. | DATA | Cross-use trap |
| 7 | 1 | n/a (data) | PathfinderClass full struct | Inspect bytes at PathfinderClass singleton base for `+0x18`, `+0x1C`, `+0x20`, `+0x24` (dual closed-list region). Confirm which is ground vs bridge layer and what indexes them. | DATA | Low |
| 8 | 1 | `0x4D3920` | `FootClass__Find_Path` | Entry point that constructs the pathfinder call; passes locomotor context. | MEDIUM | Low |
| 9 | 1 | `0x4CBBA0` | `FootClass__Run_AStar` | Thin wrapper around the main pathfinder; confirm what it filters. | LIGHT | Low |

**Phase 1 checkpoint:** After items 1–9 complete, write a one-page summary covering (a) the verified A* edge-cost formula, (b) the verified PathfinderClass dual closed-list mechanism, (c) the verified gate that switches between ground/bridge cost arrays, (d) any address that drifted from the user's list. Pause for user review before Phase 2.

### Phase 2 — Per-Class Can_Enter_Cell + CheckBridgeTraversal + Cell-State Offsets

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 10 | 2 | `0x73F0A0` | `UnitClass__Can_Enter_Cell` | **Re-verify fresh.** All 12 phases, all 8 return codes. Confirm bridge pre-check (Phase 1) and two-pass Phase-6 override (`prevFacing == cell.height + 4` re-reads cell+0x128). | FULL | Low |
| 11 | 2 | tbd via vtable+0x1B0 of InfantryClass | `InfantryClass__Can_Enter_Cell` | Unlabeled. **Resolve via `read_memory` on InfantryClass vtable**. Decomp full body. Confirm whether infantry uses ground-only or both layers. | FULL | Low |
| 12 | 2 | tbd via vtable+0x1B0 of AircraftClass | `AircraftClass__Can_Enter_Cell` (the real one — `0x415B10` is suspected landing-pad finder, NOT this) | Resolve via vtable. Confirm clearance check semantics. | FULL | Low |
| 13 | 2 | tbd via vtable+0x1B0 of BuildingClass | `BuildingClass__Can_Enter_Cell` | Placement, not movement, but bridge-aware. Resolve via vtable. | MEDIUM | Low |
| 14 | 2 | `0x55ABF0` | `LocomotionClass__Can_Enter_Cell` | **Re-verify TS-dead claim.** Confirmed 4-byte `return 0` stub. Test: any subclass that doesn't override → call would no-op. Justify non-port. | DATA | Confirmed dead-base — flag justified |
| 15 | 2 | `0x4D9C60` | `CheckBridgeTraversal` | **Full re-decompilation.** Reads cell flags 0x100/0x200, cell+0x11B (Level), cell+0x11C (ramp passability). Returns 0/7. Called via embedded site inside Process_Drive_Track. | FULL | Low |
| 16 | 2 | CellClass | Field map: `+0x124` (ground occupancy) / `+0x128` (bridge occupancy) / `+0xE4` (FirstObject ground) / `+0xE8` (AltObject bridge) / `+0x140` (Flags u32) / `+0x11B` (Level i8) / `+0x11C` (ramp passable) / `+0x116` (tube index) | Inspect via `mcp__ghidra-mcp__get_struct_layout` and confirm via at-least-3 readers and 1 writer per offset. **+0x124 vs +0x128** is load-bearing for dual-layer occupancy. | DATA | Low |
| 17 | 2 | All readers/writers of cell-flag bits `0x80`, `0x100`, `0x200`, `0x400`, `0x800`, `0x40000` | **Re-verify each bit's semantic role in pathfinding.** `0x80` (?), `0x100` (on-bridge confirmed), `0x200` (bridgehead confirmed), `0x400` (?), `0x800` (NS bridge orientation), `0x40000` (PathfinderClass passability toggle confirmed). | MEDIUM | Low |

**Phase 2 checkpoint:** Phase 2 summary must confirm (a) Can_Enter_Cell hierarchy, (b) two-pass semantic resolved, (c) every cell flag bit's role table, (d) cell occupancy field layout. Pause for review.

### Phase 3 — Locomotor Bridge Handling (live YR locomotors only)

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 18 | 3 | `0x4B0F20` | `DriveLocomotionClass__Process_Drive_Track` | Full body (~5.6 KB). Bridge-detection at `0x4B1812` / `0x4B1830` / `0x4B184A`. Confirm all 3 instruction sites resolve into this function. | FULL | Low |
| 19 | 3 | `0x4AFD40` | `DriveLocomotionClass__Set_Destination` | Reads cell+0x140 & 0x100; adds `g_BridgeZOffset_Drive` to dest Z at `0x4AFDE2`. | FULL | Low |
| 20 | 3 | `0x4AF4A0` | `DriveLocomotionClass__ComputeBridgeZOffset` | Init `g_BridgeZOffset_Drive = ftol(g_DriveHeightStep * 4)`. Constructor-time. **Decompile and confirm `4 * height_step` formula.** | FULL | Low |
| 21 | 3 | `0x4B0500` | `DriveLocomotionClass__Process` | Caller of #18. Confirm dispatch. | MEDIUM | Low |
| 22 | 3 | `0x69EBB0` | `ShipLocomotionClass__Compute_BridgeZOffset` | Ship counterpart. **Confirm `g_BridgeZOffset_Ship` init value.** Open question from [NAVAL_SYSTEM_RESEARCH.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVAL_SYSTEM_RESEARCH.md). | FULL | Low — but confirm |
| 23 | 3 | Ship vtable | `ShipLocomotion::Set_Destination` / `Process_Drive_Track` | Per [SHIP_VS_DRIVE_LOCOMOTION_COMPARISON.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHIP_VS_DRIVE_LOCOMOTION_COMPARISON.md) 95% identical with 6 differences — confirm bridge behavior. | MEDIUM | Low |
| 24 | 3 | JumpJet vtable | `JumpJetLocomotion::Set_Destination` / `Process` / `Is_Moving` | Resolve via constructor `0x54AC40` vtable. **Search for on-bridge flag reads** — if none, document JumpJet ignores bridge layer. | FULL | Low |
| 25 | 3 | Hover Move @ `0x514310`, SpeedUpdate @ `0x515ED0` | `HoverLocomotion::Move` / `SpeedUpdate` | **Search for cell+0x140 & 0x100 reads.** Confirm whether Hover follows bridge layer or floats. | FULL | Low |
| 26 | 3 | Float locomotor | **JUSTIFY NON-COVERAGE.** No symbols. Phase 6 task — document that FloatLocomotion is not a YR locomotor class; ship/naval handling is in Ship locomotor. | N/A | n/a |
| 27 | 3 | Walk vtable @ `0x75AC00` (Head_To), `0x75AC80` (Process), `0x75AB30` (Is_Moving), `0x75ACB0` | `WalkLocomotionClass::*` | Full bridge interaction. **Confirm infantry on-bridge detection and any Z bump.** | FULL | Low |
| 28 | 3 | DropPod (Constructors `0x4B5AB0` / `0x4B5B00` / `0x4B66F0`) | `DropPodLocomotionClass` | **No labeled Process or Set_Destination.** Trace constructor xrefs to find dispatch. Confirm whether DropPod respects bridge layer at landing. **Verify active-in-YR** (Allied Paradrop uses parachute, not DropPod — DropPod may be cinematic/scripted-only). | MEDIUM | **TS-suspect** |
| 29 | 3 | Teleport @ `0x718B70` (Process), `0x718100` (HeadToCoord), `0x718080` (Is_Moving) | `TeleportLocomotionClass` | Chrono. Confirm teleport-destination cell layer selection. Bridge-on-arrival edge case. | MEDIUM | Low |
| 30 | 3 | Tunnel Constructor `0x728A00` | `TunnelLocomotionClass` | **TS-DEAD CONFIRMED — Phase 6 justifies non-coverage.** Zero static callers. Subterranean is TS-legacy and gated off in YR. | N/A | **TS-confirmed dead** |
| 31 | 3 | Parachute — no Locomotor class | **JUSTIFY NON-COVERAGE.** Parachute in YR is a FootClass state, not a Locomotor. `ObjectClass__DetachParachute @ 0x5F6DA0`, `SpawnUnitsWithParachute @ 0x4585C0`. Document that bridge interaction during parachute = landing-cell Can_Enter_Cell only. | N/A | n/a (not a locomotor) |
| 32 | 3 | `g_BridgeZOffset_*` family (Drive confirmed; Ship `g_BridgeZOffset_Ship`; others?) | **Search for all `g_BridgeZ*` globals via Ghidra string/data.** Confirm one per locomotor or shared. | DATA | Low |

**Phase 3 checkpoint:** Locomotor-by-locomotor table of bridge interaction depth. Confirms live vs TS-dead split.

### Phase 4 — Zone System

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 33 | 4 | `0x56C510` | `MapClass__UpdateBridgeZonesHelper` | **Full 8-phase decompilation (re-verify fresh).** Floods 3 zone arrays, then walks BridgeRecords baking edges. Wide caller list. | FULL | Low |
| 34 | 4 | `0x56CB90` | `MapClass__ZoneFloodFillScanLine` | **Re-verify asymmetric height thresholds.** Scan-line flood, assigns zone IDs via g_PassabilityMatrix. Recursive self-caller. | FULL | Low |
| 35 | 4 | `0x56D230` | `MapClass__GetZoneID` | **Re-verify perpendicular-walk direction** — prior audit found inverted ternary in [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md). Walk perpendicular to land endpoint when bridge destroyed. | FULL | Low — known-prior-error to re-confirm |
| 36 | 4 | `0x56D100` | `MapClass__Can_Reach_Zone` | High-level reachability. Wide caller list. Confirm same-zone test and out-of-playfield short-circuit. | FULL | Low |
| 37 | 4 | `0x56D6E0` | `MapClass__ComputeBridgeZones` | Initial scan: iterates every cell, detects bridge tile via IsoTileTypeIndex tables (`DAT_0082A734`/`DAT_0082A774`), pushes BridgeRecord at MapClass+0x54. Records never removed; `+0x08 is_intact` toggled. | FULL | Low |
| 38 | 4 | `0x56DA10` | `MapClass__FindBridgeRecord` | Linear scan; skips records with `+0x0C != 0` (low bridges — i.e., FindBridgeRecord is **high-only**). Confirm. | FULL | Low |
| 39 | 4 | `0x56DAE0` | `MapClass__InvalidateBridgeZones` | Sets is_intact=0, calls RemoveBridgeZoneEdges. Callers from damage state machine. | FULL | Low |
| 40 | 4 | `0x56DB70` | `MapClass__ValidateBridgeZones` | Sets is_intact=1, calls AddBridgeZoneEdges + Can_Reach_Zone validation. | FULL | Low |
| 41 | 4 | `0x5851B0` | `MapClass__AddBridgeZoneEdges` | **Full body (not just address).** Inserts up to 6 zone-graph edges into per-MovementZone table at MapClass+0x90. | FULL | Low |
| 42 | 4 | `0x584E50` | `MapClass__RemoveBridgeZoneEdges` | Inverse. | FULL | Low |
| 43 | 4 | `0x56D460` | `MapClass__AssignOrphanedCellZone` | Adjacent helper — confirm. | LIGHT | Low |
| 44 | 4 | `0x583180` | `MapClass__ResolvePathCoord_BridgeAware` | Snaps path waypoint to bridge endpoint. Uses Sqrt_Approx to pick nearer endpoint. | MEDIUM | Low |
| 45 | 4 | MapClass+0x90 region | PathfinderClass zone-adjacency graph layout (3 × 0x24 stride entries; `+0x00` vtable, `+0x04` edges_ptr, `+0x08` count, `+0x10` cap, `+0x18` endpoint pair) | Inspect via `read_memory` and confirm field types. | DATA | Low |
| 46 | 4 | Caller of `GetZoneID` not yet labeled | `Pathfinding_update_continued` | User-listed as called from GetZoneID. Find via xref and decomp. | MEDIUM | Low |

**Phase 4 checkpoint:** Zone system end-to-end: build → invalidate on bridge damage → revalidate on repair. Confirm GetZoneID perpendicular-walk direction matches the binary, refuting or confirming the prior inverted-ternary finding.

### Phase 5 — Edge Cases & Integrations

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 47 | 5 | Path invalidation on bridge collapse | Find xrefs from `InvalidateBridgeZones` callers; trace how in-flight FootClass `path_queue[24]` gets torn down. | FULL | Low |
| 48 | 5 | Group / formation pathfinding on bridges | `FootClass__group_destination` or similar. **Confirm chokepoint handling** — do groups serialize on bridges? | MEDIUM | Low |
| 49 | 5 | Two-layer occupancy divergence at bridgehead-exit boundary tick | The bounded-parity gap from prior audit. Trace the exact tick when a unit's cell-list pointer moves from cell+0x124 to cell+0x128 (or vice versa). | FULL | Low |
| 50 | 5 | Bridgehead vs body cell distinction | Cell flag `0x200` (bridgehead) vs `0x100` (body). Find every read site and confirm A* gate behavior at each. | MEDIUM | Low |
| 51 | 5 | Bump-crush on bridges | `BumpCrushClass` or similar; verify whether vehicles crushing infantry on bridge layer reads cell+0x128 or +0x124. | MEDIUM | Low |
| 52 | 5 | Sub-cell positioning on bridges | sub_x/sub_y per-cell math at bridge cells; verify same as ground or special. | LIGHT | Low |
| 53 | 5 | Repair-truck pathfinding to bridge huts | Hut registry + path target selection for engineer/MCV repair trucks. | MEDIUM | Low |
| 54 | 5 | AI pathfinding to attack a unit on a bridge | AI target-selection picks bridge-layer cell as approach point? | MEDIUM | Low |
| 55 | 5 | Passenger / unload pathfinding on bridges | Transport unloads on bridge — passengers need cell+0x128 reservation? | MEDIUM | Low |
| 56 | 5 | Aircraft landing-site selection near bridges | `AircraftClass__Can_Enter_Cell` (suspected `0x415B10` is landing-pad finder, NOT vtable Can_Enter_Cell). Re-verify what `0x415B10` actually does and how landing avoids bridges. | FULL | Confirm 0x415B10 semantic |
| 57 | 5 | `TooBigToFitUnderBridge` (UnitTypeClass+0xE16) | **Re-verify in pathfinding context, not just rendering.** Open semantic question: navigation block vs eviction-only. | FULL | Low |
| 58 | 5 | `LowBridge AllowBurrowing` from theater INI | Cross-check theater INI keys (`Tunnels=53`, `AllowBurrowing=false`) against any binary read site. Confirm burrowing is TS-dead in YR. | MEDIUM | TS-suspect — verify |
| 59 | 5 | `SpeedType` per locomotor × bridge-cell speed table | g_PassabilityMatrix lookup with bridge cells. Confirm each SpeedType's bridge-cell legality. | MEDIUM | Low |
| 60 | 5 | `MovementZone` per locomotor × bridge cell in PassabilityMatrix | Same as above, but for MovementZone enum. | MEDIUM | Low |
| 61 | 5 | `FlyBridgeHeight` (open from [AIRCRAFTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AIRCRAFTCLASS_GHIDRA_REPORT.md)) | Confirm INI key exists / doesn't, binary read site, semantic. | MEDIUM | Low |

### Phase 6 — TS-Legacy Justifications (no decompilation required, just documentation)

| # | Phase | Topic | Action |
|---|---|---|---|
| 62 | 6 | `TunnelLocomotionClass` non-coverage | Document: Constructor `0x728A00`, zero static callers. Subterranean is TS-legacy gated off in YR. Per MEMORY entry, do not implement. Cite confirmed-dead status. |
| 63 | 6 | `ParachuteLocomotionClass` non-existence | Document: Parachute is not a YR Locomotor class; it's a FootClass state. Bridge interaction = landing-cell Can_Enter_Cell only. |
| 64 | 6 | `FloatLocomotionClass` non-existence | Document: No symbols in gamemd. Ships use ShipLocomotionClass (Drive sibling). "Float" is a SpeedType/MovementZone, not a locomotor. |
| 65 | 6 | `LocomotionClass::Can_Enter_Cell @ 0x55ABF0` non-port | Document: 4-byte `return 0` stub. Pure-virtual base. Subclasses override. |
| 66 | 6 | Burrowing / `AllowBurrowing` non-implementation | Document: TS-legacy per CLAUDE.md. Confirm gating, no implementation. |

### Phase 7 — Synthesis

| # | Phase | Topic | Action |
|---|---|---|---|
| 67 | 7 | Write `BRIDGE_PATHFINDING_LOCOMOTION_OVERVIEW.md` | Cross-reference every per-system doc. List every parity divergence with severity = player-visibility × trigger-frequency (per CLAUDE.md). Quote address for every binary claim. |
| 68 | 7 | Update [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) | One entry per `/re-investigate` and `/verify-doc` run. |
| 69 | 7 | Update stale `BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md` | Correct the CliffBackImpassability claim per 2026-05-12 audit. |

---

## 4. Detail Checklist (per item, executor must extract)

For every Phase 1–5 item:

**Always extract:**
- Address (function start) + first 8 bytes of decompiled body
- Parameter list with types (note `int` vs `int *` for offset arithmetic — see CLAUDE.md decompilation pitfall)
- Return type and meaning of every return code
- Caller list (count + named callers from `get_function_callers`)
- Active-in-YR status (Yes / No / Conditional) with gating-flag citation
- Confidence: HIGH / MEDIUM / LOW
- Cross-references to other items in this plan

**Specific extraction targets:**

- **Magic numbers**: Every literal float, int, and hex constant in A* edge cost, Z-offset math, height thresholds, cost multipliers. Cite the address of each `.rdata` slot.
- **Bit flags and masks**: Every `& 0x*` and `| 0x*` on cell flags. Build a bit-by-bit table of cell+0x140.
- **State machine states**: Cell + bridge state enums. Match to the 18-state damage machine where relevant.
- **INI keys**: Every key from §5 that the function reads, via traced ReadINI call chain.
- **Struct offsets**: Cite `param_1` type explicitly before quoting offsets (CLAUDE.md trap).
- **Clamps, rounding, off-by-ones**: Especially `ftol`, `floor`, `>>`, height-level math.
- **Edge cases**: Null cell pointer, cell at map edge, bridge mid-collapse, unit on the seam tile.
- **Timing/ordering**: Where in `advance_tick` (Rust naming) the binary equivalent runs.
- **TS-legacy flags**: `SpecialFlags & 0x1000`, fog flags, anything found gated.
- **Vtable dispatches**: Resolved to concrete address per locomotor + class.

---

## 5. INI Keys in Scope

| Key | Section | Default | Suspected purpose | In Rust? |
|-----|---------|---------|-------------------|----------|
| `TooBigToFitUnderBridge` | per-unit | implicit false | Block / evict tall units under bridges (open semantic) | Partial — render only |
| `ZFudgeBridge` | per-unit | numeric | Z-offset clearance under bridge | Unknown |
| `BridgeRepairHut` | per-building | yes | Tags bridge repair station | Yes (state present, not wired) |
| `Locomotor` (GUID) | per-unit | varies | Selects locomotor class | Yes (LocomotorKind enum) |
| `SpeedType` | per-unit | varies | Speed/cost grid selector | Yes |
| `MovementZone` | per-unit | varies | Zone-system selector | Yes |
| `HoverHeight` | [General] | 120 | Hover float height | Likely yes |
| `HoverDampen` / `HoverBob` / `HoverBoost` / `HoverAcceleration` / `HoverBrake` | [General] | various | Hover physics | Partial |
| `BalloonHoverHeight` / `BalloonHoverDampen` / `BalloonHoverBob` | [General] | various | Balloon variants | Unknown |
| `TunnelSpeed` | [General] | 1 | **TS-suspect** — verify not used in YR | Skip |
| `TrackedUphill` / `TrackedDownhill` / `WheeledUphill` / `WheeledDownhill` | [General] | 1.0/1.2 | Slope speed modifiers | Yes |
| `JumpJet` + family (JumpjetSpeed/Climb/Crash/Accel/TurnRate/Height/Wobbles/Deviation/NoWobbles) | per-unit | various | JumpJet locomotor params | Partial |
| `BridgeSet` / `WoodBridgeSet` / `TrainBridgeSet` / `BridgeTopLeft1..2` / etc. | [Theater] | tile IDs | Theater bridge tile mapping | Yes (parsed) |
| `WaterBridge` | [Theater] | tile ID | Water bridge tile | Yes |
| `BridgeVoxelMax` | [General] | 3 | Debris count on destruction | Yes (in damage state) |
| `BridgeExplosions` | [General] | TWLT* | Explosion anims | Yes |
| `BridgeDestruction` / `DestroyableBridges` | [General] | yes | Toggle destruction | Yes |
| `BridgeStrength` | [General] | 1500 | RNG threshold for destruction | Yes |
| `RepairBridgeSound` | [General] | BridgeRepaired | Sound cue | Missing |
| `Tunnels` | [Theater] | tile ID | Tunnel entrance tile — **TS-suspect** | Skip |
| `AllowBurrowing` | [Terrain] | false | Burrow gate — **TS-suspect** | Skip |
| `FlyBridgeHeight` | per-unit (?) | unknown | Aircraft bridge clearance — **open question** | Missing |

---

## 6. Caller & Integration Map

### Inbound (callers of plan items)

| Caller | Calls into | When | Decompile? |
|--------|-----------|------|------------|
| `CCINIClass__Constructor` | `ComputeBridgeZones` (#37), `UpdateBridgeZonesHelper` (#33) | Map load | LIGHT — confirm load-time only |
| `MapClass__CollapseBridge_EW_High` | `UpdateBridgeZonesHelper` (#33) | Bridge collapse | LIGHT |
| `ProcessBridgeDamageStateMachine_High/Low` | `InvalidateBridgeZones` (#39) | Per-tick damage state | LIGHT (already covered in HIGH_BRIDGE_DAMAGE_STATE_MACHINE doc) |
| `ProcessBridgeDestruction_High/Low` | `ValidateBridgeZones` (#40) | Repair completion | LIGHT |
| `FootClass__CanReachDestination`, `FootClass__Is_Cell_Harvestable`, `FootClass__Is_Cell_Weedable` | `GetZoneID` (#35) | Movement command | LIGHT — broad caller list confirms live |
| `AStar_pathfind_search` (#2) | `Zone_precheck` (#4), `Can_Reach_Zone` (#36) | Pathfind start | (already in plan) |
| `DriveLocomotionClass__Process` (#21) | `Process_Drive_Track` (#18) | Per-tick | (already in plan) |

### Outbound (Rust integration surface)

- `src/sim/pathfinding/core.rs` — A* spine, plan items #1–#9 inform changes
- `src/sim/pathfinding/cell_entry.rs` — per-class Can_Enter_Cell, plan items #10–#14
- `src/sim/movement/movement_bridge.rs` — bridge layer transitions, plan items #18–#25
- `src/sim/movement/movement_path.rs` — `supports_layered_bridge_pathing()` gate, plan items #18–#27
- `src/sim/pathfinding/zone_map.rs` / `zone_build.rs` — plan items #33–#42
- `src/sim/world/bridge_orchestrator.rs` — damage / invalidate / validate, plan items #39–#40

### Will NOT be investigated (justified)

- **TunnelLocomotion (#30, #62)** — TS-dead, zero static callers
- **ParachuteLocomotion (#31, #63)** — does not exist as a Locomotor class in YR; covered as FootClass state
- **FloatLocomotion (#26, #64)** — does not exist as a Locomotor class in YR; ships use ShipLocomotion
- **`AllowBurrowing` / `Tunnels=`** — TS-legacy per CLAUDE.md MEMORY entry
- **Render-only bridge logic** — `bridge_atlas.rs`, `bridge_railing_atlas.rs`, `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` — out of pathfinding/locomotion scope
- **Damage state machine internals** — already covered HIGH-confidence in `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`; will be referenced not re-derived

---

## 7. TS-Legacy Risk Register

| Risk | Where | Mitigation |
|------|-------|------------|
| **`TunnelLocomotionClass` confirmed TS-dead** (zero callers from constructor `0x728A00`) | Item #30, #62 | Document, do not port. |
| **`LocomotionClass::Can_Enter_Cell @ 0x55ABF0`** — 4-byte `return 0` stub (pure-virtual base) | Item #14, #65 | Document. Every subclass must override; not TS-dead, just base. |
| **`AircraftClass__Can_Enter_Cell @ 0x415B10` name is misleading** — decomp shows landing-pad finder, NOT the A* per-cell predicate | Item #12, #56 | Confirm via vtable+0x1B0 of AircraftClass; relabel after deep pass. |
| **`AllowBurrowing` / `Tunnels=` in theater INI** — TS-legacy | Item #58, #66 | Confirm gating, do not implement. |
| **A* cost constant `0x7E37B8` (10.0f)** — reused by damage code | Item #5, #6 | Note in synthesis; do NOT rename as pathfinding-exclusive. |
| **`BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md`** — stale CliffBackImpassability claim | §2 contradictions | Item #69 corrects. |
| **`DropPodLocomotionClass`** — sparse labeling, 3 constructors, no labeled Process | Item #28 | Verify active-in-YR. If only used by cinematic/script triggers, justify partial coverage. |
| **`SpecialFlags`-gated branches in Can_Enter_Cell, zone code, locomotor code** | All phases | For every gated branch found, decompile gate condition and confirm YR default. |
| **Fog-of-war gated bridge code (`SpecialFlags & 0x1000`)** | Phase 1, 2 | Per MEMORY: fog-of-war defaults off in YR. Any bridge code reachable only via fog gate is dead in standard YR. |
| **Two-pass Can_Enter_Cell Phase-6 override** — open from prior audit | Item #10 | This is NOT TS-dead, but the gate condition (`prevFacing == cell.height + 4`) needs full re-derivation; prior audit deferred. |

---

## 8. Current Rust Implementation Surface

See Agent C output for full inventory. Headline files:

- **Bridge state / orchestrator**: `src/sim/bridge_state/mod.rs`, `src/sim/bridge_state/walker.rs`, `src/sim/world/bridge_orchestrator.rs`, `src/sim/bridge_specs.rs`, `src/bridge_re.rs`
- **Pathfinding core**: `src/sim/pathfinding/core.rs`, `src/sim/pathfinding/passability.rs`, `src/sim/pathfinding/cell_entry.rs`, `src/sim/pathfinding/terrain_cost.rs`, `src/sim/pathfinding/terrain_speed.rs`, `src/sim/pathfinding/zone_map.rs`, `src/sim/pathfinding/zone_build.rs`, `src/sim/pathfinding/zone_search.rs`, `src/sim/pathfinding/path_smooth.rs`
- **Locomotors**: `src/sim/movement/locomotor.rs`, `src/sim/movement/drive_track.rs`, `src/sim/movement/movement_bridge.rs` (active bridge predicate), `src/sim/movement/air_movement.rs`, `src/sim/movement/jumpjet_movement.rs`, `src/sim/movement/droppod_movement.rs`, `src/sim/movement/teleport_movement.rs`, `src/sim/movement/tunnel_movement.rs`, `src/sim/movement/parachute_descent.rs`, `src/sim/movement/rocket_movement.rs`
- **Movement coordination**: `src/sim/movement/movement_path.rs` (gates `supports_layered_bridge_pathing()`), `src/sim/movement/movement_commands.rs`, `src/sim/movement/movement_tick.rs`, `src/sim/movement/movement_step.rs`, `src/sim/movement/movement_occupancy.rs`, `src/sim/movement/movement_blocked.rs`, `src/sim/movement/group_destination.rs`, `src/sim/movement/bump_crush.rs`
- **Rules**: `src/rules/locomotor_type.rs`, `src/rules/jumpjet_params.rs`, `src/rules/bridge_warheads.rs`

This surface is NOT being modified by this plan — the plan produces research only. Rust changes belong to subsequent `/brainstorm` + `/write-plan` cycles.

---

## 9. Deferred Open Questions

(Questions the scoping pass surfaced but couldn't answer — the executor must close each.)

1. **Two-pass Can_Enter_Cell Phase-6**: under what exact condition does the binary re-read `cell+0x128` instead of `cell+0x124`? Prior audit suspected `prevFacing == cell.height + 4`. Resolve in item #10.
2. **`g_BridgeZOffset_Ship` initialization**: where and how is the ship Z-offset constant set? Open in [NAVAL_SYSTEM_RESEARCH.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVAL_SYSTEM_RESEARCH.md). Resolve in item #22.
3. **`FlyBridgeHeight` INI key**: does it exist? Where read? What gates aircraft bridge clearance? Resolve in item #61.
4. **`TooBigToFitUnderBridge` semantic**: navigation block vs eviction-only? Resolve in item #57.
5. **`AircraftClass__Can_Enter_Cell @ 0x415B10`**: what is this actually? Decomp opening lines suggest landing-pad finder, not vtable Can_Enter_Cell. Resolve in items #12 and #56.
6. **GetZoneID perpendicular-walk inverted-ternary**: prior audit flagged [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) had it backwards. Resolve in item #35.
7. **DropPodLocomotion live-in-YR**: who calls it? Cinematic? Triggered? Resolve in item #28.
8. **Bridgehead-exit boundary-tick two-layer divergence**: the bounded parity gap from prior audit. Resolve in item #49.
9. **InfantryClass / BuildingClass / FootClass Can_Enter_Cell**: addresses not yet resolved (require vtable+0x1B0 reads). Resolve in items #11, #13.
10. **PathfinderClass full struct layout**: `+0x18`/`+0x1C`/`+0x20`/`+0x24` dual closed-list region — which is ground vs bridge, what indexes them. Resolve in item #7.
11. **`Pathfinding_update_continued` caller of GetZoneID**: find via xref. Resolve in item #46.

---

## 10. Execution Strategy

**Recommended: BATCHED SUBAGENTS WITH PHASE CHECKPOINTS.**

This plan exceeds the normal single-`/re-investigate` ceiling. Single-session would produce shallow reports on the back half. Execute as follows:

1. **Phase 1 (items #1–#9)** → one focused session. Items #1, #2, #3, #4 can run as parallel subagents (each producing a short per-function report); #5–#9 consolidate. **Checkpoint before Phase 2.**
2. **Phase 2 (items #10–#17)** → one session. Items #10–#13 (per-class Can_Enter_Cell) can parallelize. **Checkpoint before Phase 3.**
3. **Phase 3 (items #18–#32)** → one session. Items #18–#21 (Drive) sequential, then #22–#25 (Ship/JumpJet/Hover) parallel, then #27–#29 (Walk/DropPod/Teleport) parallel, then #26/#30/#31 (justified non-coverage). **Checkpoint.**
4. **Phase 4 (items #33–#46)** → one session. Items split into two halves: build/destroy/validate trio (#33–#40) sequential; edge inserters/removers + helpers (#41–#46) parallel.
5. **Phase 5 (items #47–#61)** → one session of mostly parallelizable mini-investigations.
6. **Phase 6 (items #62–#66)** → one short documentation pass. No decompilation.
7. **Phase 7 (items #67–#69)** → synthesis. Single session.

**Each phase ends with a written checkpoint** the user reviews before Phase N+1 starts. If a phase reveals scope drift (e.g., Phase 1 finds the A* spine is more complex than expected), the plan is revised on the spot rather than burning through Phase 2–7 on a stale scope.

**Per-item naming convention** for output docs in `docs/research/`:

- [BRIDGE_ASTAR_DUAL_CLOSED_LIST_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_ASTAR_DUAL_CLOSED_LIST_GHIDRA_REPORT.md) (items #1, #2, #3, #7)
- [BRIDGE_ASTAR_COSTS_AND_ZONE_PRECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ASTAR_COSTS_AND_ZONE_PRECHECK_GHIDRA_REPORT.md) (items #4, #5, #6)
- [BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CAN_ENTER_CELL_HIERARCHY_GHIDRA_REPORT.md) (items #10–#14)
- [BRIDGE_CHECK_TRAVERSAL_AND_CELL_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CHECK_TRAVERSAL_AND_CELL_OFFSETS_GHIDRA_REPORT.md) (items #15–#17)
- [BRIDGE_LOCOMOTOR_DRIVE_SHIP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_LOCOMOTOR_DRIVE_SHIP_GHIDRA_REPORT.md) (items #18–#23)
- [BRIDGE_LOCOMOTOR_AIR_HOVER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_LOCOMOTOR_AIR_HOVER_GHIDRA_REPORT.md) (items #24–#25)
- [BRIDGE_LOCOMOTOR_WALK_DROPPOD_TELEPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_LOCOMOTOR_WALK_DROPPOD_TELEPORT_GHIDRA_REPORT.md) (items #27–#29)
- [BRIDGE_LOCOMOTOR_NONCOVERAGE_JUSTIFICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/BRIDGE_LOCOMOTOR_NONCOVERAGE_JUSTIFICATION.md) (items #26, #30, #31, #62–#66)
- [BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md) (items #33–#42)
- [BRIDGE_ZONE_HELPERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_HELPERS_GHIDRA_REPORT.md) (items #43–#46)
- `BRIDGE_EDGE_CASES_GHIDRA_REPORT.md` (items #47–#61)
- `BRIDGE_PATHFINDING_LOCOMOTION_OVERVIEW.md` (item #67 — synthesis)

---

## 11. Success Criteria

This investigation closes only when ALL of the following hold:

- [ ] Every item #1–#69 has a per-system research doc or a written justified non-coverage entry.
- [ ] Every binary claim cites a Ghidra address AND either a decompilation snippet or a `read_memory` byte dump.
- [ ] Every function in §3 has an "Active in YR: Yes / No / Conditional" entry with citation.
- [ ] Every TS-legacy concern in §7 has a documented resolution.
- [ ] Every deferred question in §9 is closed (or explicitly re-deferred with a reason).
- [ ] The synthesis doc `BRIDGE_PATHFINDING_LOCOMOTION_OVERVIEW.md` exists, cross-references every per-system doc, and lists every parity divergence with severity = player-visibility × trigger-frequency.
- [ ] [PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md) has a `/verify-doc` audit appended.
- [ ] [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) has one entry per `/re-investigate` and `/verify-doc` run.
- [ ] `BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md` stale CliffBackImpassability claim is corrected (item #69).
- [ ] Confidence (HIGH / MEDIUM / LOW) is tagged on every finding.
- [ ] Cross-doc contradictions surfaced in §2 are resolved in the synthesis doc.

**Prior audit findings have been treated as starting points only.** Every claim in §3 has been or will be re-verified against the binary. The deliverable cites binary evidence per CLAUDE.md, not prior-doc inheritance.

---

## Sources

**Ghidra addresses sampled (Step 1 scoping):**
PathfinderClass: 0x42ACF0, 0x42C290, 0x429830, 0x429A90, 0x42A5B0, 0x42C900, 0x42CCD0, 0x42CF80, 0x4CBBA0
Cost data: 0x7E37B4, 0x7E37B8, 0x7E37BC
Can_Enter_Cell: 0x73F0A0, 0x415B10 (mislabeled?), 0x55ABF0 (stub)
CheckBridgeTraversal: 0x4D9C60
Drive locomotor: 0x4B0F20, 0x4AFD40, 0x4AF4A0, 0x4AF470, 0x4B0500, 0x4B1812, 0x4B1830, 0x4B184A, 0x4AFDE2
Ship: 0x69EBB0
Walk: 0x75AB30, 0x75AC00, 0x75AC80, 0x75ACB0
Fly: 0x4CCA90, 0x4CD600, 0x4CF610, 0x4CFD90, 0x4CCFD0
JumpJet: 0x54AC40 (constructor only)
Hover: 0x514310, 0x515ED0
Teleport: 0x718080, 0x718100, 0x718B70
DropPod: 0x4B5AB0, 0x4B5B00, 0x4B66F0
Tunnel: 0x728A00 (dead)
Parachute: 0x5F6DA0, 0x4585C0 (not a locomotor)
Foot: 0x4D3920, 0x4DDC40, 0x5F5FA0, 0x5F6A70
Map: 0x56C510, 0x56CB90, 0x56D100, 0x56D230, 0x56D460, 0x56D6E0, 0x56DA10, 0x56DAE0, 0x56DB70, 0x583180, 0x5851B0, 0x584E50
Data tables: 0x0082A734, 0x0082A774, MapClass+0x40..+0xF8

**Docs searched (Step 1 scoping):**
`docs/research/` — full list of 30+ bridge/pathfinding/locomotion docs enumerated in §2
`docs/gap-scans/` — 2026-05-11, 2026-05-12, 2026-05-08 disparity scans

**INI files checked:**
`ini/rulesmd.ini`, `rules.ini`, `artmd.ini`, `art.ini`, theater INIs

**Related plans:**
`docs/plans/2026-05-07-bridges-tier2-*`, `docs/plans/2026-05-08-bridges-tier2-*`, `docs/plans/2026-05-06-bridges-tier1-ini-parser-*` — implementation plans for the work the research feeds
