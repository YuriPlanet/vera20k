# Bridge Display Table — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass for Bridges Tier 2
> **Phase D (renderer)**. Execute by running `/re-investigate bridge display
> table` with this plan loaded as context, OR dispatch the function inventory
> to subagents in batches grouped by phase. Do NOT write Rust.

**Topic:** Bridge tile selection at draw time — the `(overlay_byte, damage_state, axis, deck_level) → visible tile` mapping in gamemd.exe, scoped for Phase D renderer parity.
**Scope Size:** Large — ~40 functions across 4 phases, ~7 globals/tables, ~5 INI keys (small INI surface — most parameters are hardcoded).
**Est. Effort:** ~10–15 hours of `/re-investigate` work (anchored: ~9 functions FULL × 20 min, ~17 MEDIUM × 8 min, ~14 LIGHT × 4 min). Recommend **batched subagents per phase**.
**Prior Research:**
  - `ra2-rust-game-docs/BRIDGE_RENDERING_GHIDRA_REPORT.md` — primary, **predates Phase F**, drift expected.
  - `ra2-rust-game-docs/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` — covers UpdateRamp_*_High / state machine.
  - `ra2-rust-game-docs/BRIDGE_SYSTEM.md` — TubeClass + zone integration.
  - `ra2-rust-game-docs/CELLCLASS_ZONES_SPEED_BRIDGES.md` — CellClass field map.
  - `docs/plans/2026-05-07-bridges-tier2-phase-f-orchestrator-design.md` — sim-side cascade design (rim refresh stub lives here).
**Expected Output:** research document at
`docs/research/BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md`
**Next Pipeline Step:** `/brainstorm bridge renderer (Phase D)` once this report lands. The renderer design is a non-trivial shape decision (per-cell pull vs. event-driven push, integration with `bridge_atlas`/`overlay_atlas`, rim-refresh model) — skip straight to `/write-plan` only if the brainstorm closes trivially.

---

## 1. Goal

The resulting research doc must answer, with citations from the binary:

1. **What does gamemd.exe do at draw time, per bridge cell, to select the visible tile?**
   The full pure-read pipeline: which fields it reads (`bridge_damage_state`, overlay byte, height level, Latin-square coords, neighbor mask), which functions execute, and what it writes back (render caches only, no game state).
2. **Is there a single classifier table — a `BridgeDisplayTable` — or is it a constellation?**
   Light scoping says no single symbol exists; confirm and enumerate every constituent table/array if the constellation hypothesis holds.
3. **Do high bridges have a runtime tile selector, or only damage_state mutations?**
   Light scoping found no `SelectBridgeTileVariant_High`. If high bridges only re-skin via `bridge_damage_state` + Latin-square frame jitter (with map-init-stamped `tile_index` unchanged), Phase D's high-bridge renderer is dramatically simpler than the low-bridge case. Confirm with high confidence.
4. **What is the post-Phase F overlay-range invariant the renderer must honor?**
   Verify HIGH `0xCD..=0xE6` raw + `0xE7/0xE8` final, LOW `0x4A..=0x63` raw + `0x64/0x65` final, and the state-machine "out-of-range" cells, against the binary tables (`ApplyBridgeDestruction_*` inline tables, `SelectDestroyedBridgeTile_Low`).
5. **What does the rim-refresh function (`UpdateAdjacentBridges_High` / `_Low`) actually do, and when does it fire?**
   Our orchestrator stubs it (HIGH §11.9). The doc must give us the exact 8-direction walk, the dirtying criteria, and whether it mutates per-cell visible state or only marks redraw rects.
6. **Are there any draw-time mutations to per-cell game state?**
   Scoping says draw is pure-read. Confirm by tracing every callee of `DrawOverlay_Body`, `DrawOverlay_Shadow`, `Get_Draw_Offset`, `CellOverlay_TileDraw`, `FUN_006d7c00`, `FUN_004d1890`, `Cell_ContentRendering` — flag any field write outside the `cell+0x64/+0x68../+0x118` render-cache set.
7. **How do `UpdateRamp_*_High` / `_Low` cross over into display?**
   These are state-machine-side, but several have an "overlay-write branch" (Tasks 13.5/15.5) that mutates display-only state. The doc must specify which display fields they write so the renderer pulls from the right source after a state transition.

A success criterion failed if the doc cannot answer Q3 with HIGH confidence — Phase D's complexity hinges on it.

---

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md) | Per-frame draw chain, Y-offset formula, Latin-square jitter, railing emit, blitter selection | HIGH (pre-Phase F) | Predates orchestrator; rim-refresh and post-state-machine repaint not covered. Railing entry table layout (`DAT_00ABC210`/`0xABC2D0`) only partially mapped — element count + indexing formula deferred. |
| `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` | `UpdateRamp_*_High` mutators, axis encoding, destruction overlay tables, deferred rebuild flag | HIGH | Display-side consumers of state-machine output not enumerated end-to-end. `UpdateBridgeEdgeTiles_High` / `UpdateAdjacentBridges_High` mentioned but not deep-traced. |
| `BRIDGE_SYSTEM.md` | TubeClass, zone integration, A/B/C orientation tables, corner tile pointers | HIGH | Renderer-side consumption of corner tiles + middle tile arrays not traced through to draw. |
| [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) | CellClass bridge fields, IsBridge/IsWoodBridge/IsLowBridgeCell predicates | HIGH | Render-only fields (Z-adjust set, draw cache) explicitly OUT OF SCOPE in that doc. |
| Phase F orchestrator design (`...phase-f-orchestrator-design.md`) | Sim-side cascade, rim-refresh **stub** policy | HIGH (sim) | Renderer integration: `display_tile` callback unspecified. Rim refresh marked "stub-or-active pending renderer-query check" — this investigation resolves that question. |

**Conflicts between reports:** None found. All four primary docs cite consistent addresses for shared functions (e.g., `DrawOverlay_Body @ 0x47F6A0`, `bridge_damage_state` at `cell+0x11E`, `g_BridgeSet @ 0xAA0E28`). The drift is by **omission** (Phase F post-dates [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md)), not by contradiction.

**Prior plan check:** No standalone renderer/display-table investigation plan exists in `docs/plans/` (verified; see directory listing this date). Phase D renderer is consistently marked "deferred" across Tier 2 plans.

---

## 3. Function Inventory

40 functions, grouped by execution phase. **Phase 1 checkpoint** mandatory before Phase 2.

### Phase 1 — Core (per-frame draw entry chain + immediate body/shadow draw)

The executor must produce a usable answer to **Q1, Q2, Q3, Q6** after Phase 1 alone.

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|-------|----------------|
| 1 | 1 | `0x006D3D10` | `TacticalClass::Draw` | Top-level draw entry; reads + clears `g_Tactical+0xD7C` deferred-rebuild flag (HBR §12.7). Confirms what triggers a redraw cascade. | MEDIUM | Low |
| 2 | 1 | `0x006D3470` | `Tactical_layer_base_terrain` | Per-frame iter that calls `FUN_004d1890` per visible cell. Confirm bridge content-render path entry. | MEDIUM | Low |
| 3 | 1 | `0x006D3040` | `Tactical_layer_overlays` | Per-frame iter that calls `FUN_006d7c00` per visible cell. Sibling overlay path. | MEDIUM | Low |
| 4 | 1 | `0x004D1890` | `FUN_004d1890` | Per-cell terrain content; case `0x14` → `Get_Draw_Offset` + `DrawOverlay_Body`. **786 instructions** — full decompile to enumerate every case branch. Disambiguate vs `Cell_ContentRendering`. | FULL | Low — but check for SpecialFlags-gated branches |
| 5 | 1 | `0x006D6D10` | `Cell_ContentRendering` | Sibling per-cell content dispatch — also has case `0x14`. Determine which path actually fires for bridges per-frame (BRR §2.1 says this; FUN_004d1890 also has it — must disambiguate). | FULL | Low |
| 6 | 1 | `0x006D7C00` | `FUN_006d7c00` | Per-cell overlay dispatcher (called from `Tactical_layer_overlays`, 17 callees). Identify which callee handles bridge overlays specifically. | FULL | Low |
| 7 | 1 | `0x0047F6A0` | `CellClass::DrawOverlay_Body` | Bridge body SHP draw. **Reads** `bridge_damage_state` (cell+0x11E), Latin-square @ `0x81CC30`, frame_count, cell flag `0x80`. **Writes** render cache only (cell+0x64/+0x68../+0x118). The single most important function in Phase 1. | FULL | Low |
| 8 | 1 | `0x0047F510` | `CellClass::DrawOverlay_Shadow` | Shadow render; `frame_count/2 + body_frame`. Confirm shadow uses identical frame-selection as body. | MEDIUM | Low |
| 9 | 1 | `0x00480110` | `CellClass::Get_Draw_Offset` | Bridge Y offset (-16 for state 0–8, -31 for state 9–17). The axis-encoded damage_state interpretation lives here. | FULL | Low |
| 10 | 1 | `0x005FDCC0` | `FUN_005fdcc0` | Overlay-type Y offset (0 / -12 / -1) — additive into Get_Draw_Offset. Reads `OverlayTypeClass+0x2A8/0x2A9/0x2AA`. | MEDIUM | Low |
| 11 | 1 | `0x00480350` | `CellOverlay_TileDraw` | TMP deck blit + `FUN_004802A0` (railing trampoline). | MEDIUM | Low |
| 12 | 1 | `0x004802A0` | (railing trampoline — unnamed) | Wraps `FUN_005471F0` + `FUN_00547230`. | LIGHT | Low |
| 13 | 1 | `0x00547230` | `FUN_00547230` (railing emit) | Looks up railing SHP from `DAT_00ABC210` / `DAT_00ABC2D0` 16-byte-stride tables. **Open Q from prior doc:** map element count + index formula by `(overlay_byte, sub_tile_index)`. | FULL | Low |
| 14 | 1 | `0x005471F0` | `FUN_005471f0` (railing pre-check) | Sibling of `0x547230` — checks pavement/bridge bit before emit. Confirm bit semantics. | MEDIUM | Low |
| 15 | 1 | `0x00483E30` | `FUN_00483e30` | Recurring shared callee from `DrawOverlay_Body`/`Shadow`/`FUN_004d1890`/`Cell_ContentRendering`. Suspected OverlayTypeClass→SHP source resolver. Resolve purpose. | MEDIUM | Low |

**Phase 1 checkpoint deliverable:** a draft answer to Q1 (full draw chain), Q2 (constellation enumerated, even if partial), Q3 (high-bridge runtime selector — exists or not), Q6 (no game-state writes confirmed). If Phase 1 reveals the scope is wrong (e.g., a major branch we missed), revise plan before Phase 2.

### Phase 2 — Depth (tile classification, table-driven selection, low-bridge variant pickers)

Fills in **Q2 (constellation), Q4 (overlay-range verification), Q5 (rim-refresh body)**.

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|-------|----------------|
| 16 | 2 | `0x0057B440` | `MapClass::ApplyBridgeTile` | Stamps `tileset_index + heightLevel` onto a bridge cell. Calls `FUN_004863D0`. The "writer" side of tile selection — fires from map-init + state transitions, NOT per-frame. | FULL | Low |
| 17 | 2 | `0x004863D0` | `FUN_004863d0` | **Suspect: the central `(overlay_byte → tile-class index)` classifier.** 176 instr / 39 cyclomatic / no callees / immediates 16/20/40. If this is the closest thing to a "BridgeDisplayTable", a clean decompile resolves Q2. | FULL | Medium — verify all branches are YR-live (not TS) |
| 18 | 2 | `0x0057ACF0` | `MapClass::SelectBridgeTileVariant_Low` | Picks healthy low-bridge tileset variant from neighbor mask. Inline tables, immediates 22/23/24/25/28/29/30/31/32/33/35/37/39/40. | FULL | Low |
| 19 | 2 | `0x00579620` | `MapClass::SelectDestroyedBridgeTile_Low` | Picks destroyed/damaged low-bridge tile. Verify against post-Phase F invariant `0x4A..=0x63` raw + `0x64/0x65` final. | FULL | Low |
| 20 | 2 | `0x0057A430` | `MapClass::UpdateBridgeTile_Low` | Re-stamps low-bridge tile after state change; recursive on neighbors. Confirm whether this is the rim-refresh body for low bridges. | FULL | Low |
| 21 | 2 | `0x0057B210` | `MapClass::ComputeBridgeSurfaceMask` | 8-direction surface mask. Fed into `SelectBridgeTileVariant_Low` + `UpdateBridgeTile_Low`. | MEDIUM | Low |
| 22 | 2 | `0x00579B70` | `MapClass::ComputeBridgeAdjacencyMask_Low` | 8-direction adjacency mask (bit-packed). Fed into `SelectDestroyedBridgeTile_Low`. Calls `FUN_004863D0`. | MEDIUM | Low |
| 23 | 2 | `0x0057A430` | (UpdateBridgeTile_Low — already #20) | — | — | — |
| 24 | 2 | `0x00576770` | `MapClass::UpdateAdjacentBridges_High` | **The rim-refresh function our orchestrator stubs.** Walks 8-dir, dirties redraw rect when ramp tiles change. Resolve: does it mutate cell state or only mark redraw? | FULL | Low |
| 25 | 2 | `0x00571050` | `MapClass::UpdateAdjacentBridges` (low) | Low-bridge sibling of #24. | FULL | Low |
| 26 | 2 | `0x00576200` | `MapClass::UpdateBridgeEdgeTiles_High` | Ramp-edge re-evaluation (height 5/7/8/12 → new tile). | MEDIUM | Low |
| 27 | 2 | `0x0047E040` | `CellClass::SetBridgeDirection_NESW` | Sets cell flags + writes `bridge_damage_state` (0=EW, 9=NS) on 4–5-cell group. **Map-init high-bridge writer.** | MEDIUM | Low |
| 28 | 2 | `0x0047E470` | `CellClass::SetBridgeDirection_NWSE` | Alternate-direction variant. | LIGHT | Low |
| 29 | 2 | `0x0059E740` | `FUN_0059e740` (map-init bridge fixup) | Map-init pass that subtracts/adds 4 from heightLevel and stamps bridge tiles. **Likely the place where high-bridge `tile_index` is set, never updated post-init.** Confirms or refutes Q3. | FULL | Low |
| 30 | 2 | `0x004865D0` | `CellClass::HasBridgeOverlay` | Tests overlay-type membership in bridge ranges. Verify ranges match HIGH `0xCD..=0xE6` + `0xE7/0xE8`, LOW `0x4A..=0x63` + `0x64/0x65`. **Direct Q4 verification.** | MEDIUM | Low |
| 31 | 2 | `0x00484680` | `Cell_ComputeZAdjust` | Pre-computes `cellZAdjust_top/+0x10C/_bottom`, the latter pre-baked at `heightLevel+4` for any bridge. Z-buffer integration. | MEDIUM | Low |

### Phase 3 — Context & Edges (predicates, blitters, locomotor offsets, sim-side ramp mutators for cross-reference, TS-legacy clearance)

Confirms invocation context, fills **Q7 (UpdateRamp_*_High / _Low display crossover)**, and rules out TS-legacy traps.

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|-------|----------------|
| 32 | 3 | `0x00486750` | `CellClass::IsBridge` | Membership check vs `g_BridgeSet`. | LIGHT | Low |
| 33 | 3 | `0x00486770` | `CellClass::IsWoodBridge` | Membership check vs `g_WoodBridgeSet`. | LIGHT | Low |
| 34 | 3 | `0x00484AB0` | `CellClass::IsLowBridgeCell` | TubeIndex≥0 + LandType==10 (low bridge underpass). | LIGHT | Low |
| 35 | 3 | `0x00485060` | `CellClass::IsOnBridgeSurface` | Used by `SelectBridgeTileVariant_Low`. | LIGHT | Low |
| 36 | 3 | `0x00574600` | `MapClass::IsLowBridgeEndpointTile` | Endpoint classifier. | LIGHT | Low |
| 37 | 3 | `0x005746C0` | `MapClass::IsBridgeRampTile` | Ramp tile classifier. | LIGHT | Low |
| 38 | 3 | `0x00578D80` | `IsOnBridgeRamp` | Classifies cell into 6 ramp regions. Used by ApplyBridgeTile + state machine. | MEDIUM | Low |
| 39 | 3 | `0x004AED70` | `CC_Draw_Shape` | Core SHP blitter. param_7 → Z-buffer enable; flag `0x4E00`=body, `0x4601`=shadow/railing. Confirm Z-write semantics for bridge sprites. | LIGHT | Low |
| 40 | 3 | `0x00547CF0` | `TMP_TileBlitter` | Per-pixel terrain blit with Z R+W; bridge deck tiles always `z_enable=1`. | LIGHT | Low |
| 41 | 3 | `0x00490B90` | `Blitter_selector` | vtable `0xC0` selector for bridge body (Z R+W remap). | LIGHT | Low |
| 42 | 3 | `0x00576BA0` | `ProcessBridgeDamageStateMachine_High` | **Already shipped in Phase F** — re-touch only to confirm it never invokes a draw-side callee. Verify display-side mutations live exclusively in `UpdateRamp_*_High` and `ApplyBridgeDestruction_*_High`. | LIGHT | Low |
| 43 | 3 | `0x00571490` | `ProcessBridgeDamageStateMachine_Low` | Same scope as #42, low-bridge sibling. | LIGHT | Low |
| 44 | 3 | `0x00572230..0x00573170` | `UpdateRamp_{NS,EW}_{DamageA,DamageB,CollapseA,CollapseB}_High` (8 funcs) | **Q7 focus.** Each has an overlay-write branch (Tasks 13.5 deferred — runtime globals zero in static binary). Trace which display fields they write (`tile_index`, overlay_byte, flags) and whether they call `UpdateBridgeTile_*` / `UpdateAdjacentBridges_*`. | MEDIUM each | **Medium** — overlay-write branches gated on `DAT_00abad30` / `DAT_00aa1028` / `DAT_00aa0e28` which are zero in static binary. Document that the *static* binary view is incomplete; do NOT implement; cross-reference live debugger plan. |
| 45 | 3 | `0x0056ED40..0x0056FC80` | `UpdateRamp_*_Low` (8 funcs) | Low-bridge sibling set of #44. | MEDIUM each | Same as #44 |
| 46 | 3 | `0x00578100` | `MapClass::RecalcBridgeShroudFlags` | Per-120-frame shroud sweep; writes `cell+0x140` shroud bits. **Not a render-path write — confirm by tracing caller (`LogicClass::PerTickUpdate`).** Document for Q6 ruling-out. | LIGHT | Low |
| 47 | 3 | `0x004AF470` | `DriveLocomotionClass::ComputeBridgeRenderOffset` | Render-side Z offset for **units on bridges** (sprite layer, not tile selection). Out of scope for the table itself but orient the executor — clarifies that unit Z-offset is computed locomotor-side, not cell-side. | LIGHT | Low |
| 48 | 3 | `0x004DAFF0` | `ComputeZFudge` | Reads `TechnoType+0xDCC` (`ZFudgeBridge` INI key); applies at unit draw time. Out-of-scope for tile selection but document for completeness. | LIGHT | Low |

**Sizing note:** counting `UpdateRamp_*_High` and `_Low` as 16 individual entries (8 each) yields 48+ functions. They will be batch-decompiled in pairs (NS+EW, DamageA+DamageB, etc.) since they share structure. Effective unique work is closer to 32–35 distinct decompiles.

---

## 4. Detail Checklist

The executor uses this as a flat to-do list. Every item must be resolved or explicitly deferred with reason.

### Magic numbers / constants
- Latin-square table @ `0x81CC30` — verify 16 entries `{0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}` (already extracted in gap-scan, re-confirm).
- Y offsets: `-16` (state 0–8 EW), `-31` (state 9–17 NS) in `Get_Draw_Offset`. Effective-height formula `effective_height * -15 - 2` @ `0x47F7EB`.
- Overlay-type Y offsets: `0` / `-12` / `-1` in `FUN_005FDCC0`.
- Inline immediates in `SelectBridgeTileVariant_Low`: 22/23/24/25/28/29/30/31/32/33/35/37/39/40 — decode each.
- `FUN_004863D0` immediates: 16/20/40 — decode.
- Blitter flags: `0x4E00` (body), `0x4601` (shadow/railing) at `CC_Draw_Shape`.
- Blitter vtable slot: `0xC0`.
- Heightlevel adjustments: `+4` / `-4` in `FUN_0059E740` map-init.
- Edge-tile heights: 5 / 7 / 8 / 12 in `UpdateBridgeEdgeTiles_High`.

### Bit flags and masks
- Cell flag `0x80` (bridge present, used in `DrawOverlay_Body`).
- Cell flag `0x40000` (XOR-toggled by `PathfinderClass::UpdateBridgePassability`) — out of render scope but note for completeness.
- Shroud flags at `cell+0x140` (set by `RecalcBridgeShroudFlags`, not render).
- 8-direction adjacency masks in `ComputeBridgeSurfaceMask` / `ComputeBridgeAdjacencyMask_Low` — bit positions per direction.
- OverlayTypeClass flag bits at `+0x2A8/0x2A9/0x2AA` (consumed by `FUN_005FDCC0`).

### State machine states
- Bridge damage_state at `cell+0x11E`: 0–8 (EW progression) + 9–17 (NS progression). Confirm exact state→tile-frame mapping in `DrawOverlay_Body`.
- 6 ramp regions classified by `IsOnBridgeRamp` @ `0x578D80`.

### INI keys to verify (small surface — see Section 5)
All bridge-rendering INI is small. Verify each key is read into the expected struct field, with default values matching observed binary defaults.

### Struct offsets to extract
- **CellClass** — `param_1` is `int *` (verify) → check pointer arithmetic carefully:
  - `+0x11E` — `bridge_damage_state` (HBR §3)
  - `+0x80` — bridge-present flag (used in DrawOverlay_Body)
  - `+0x140` — shroud flags
  - `+0x44` — overlay byte (cell-side overlay slot)
  - `+0x64`, `+0x68..+0x74`, `+0x118` — render cache (last_draw_frame, last_clip_rect, draw token)
  - `+0x10C` — cellZAdjust_top
  - `+? ` — heightLevel (extract exact offset)
  - `+? ` — tileset_index (extract; written by `ApplyBridgeTile`)
  - TubeIndex offset (from `IsLowBridgeCell`)
- **OverlayTypeClass** — `param_1` typing varies (TS legacy uses `int *`, indexing × 4 — verify each function before extracting offsets):
  - `+0x2A8 / 0x2A9 / 0x2AA` — Y-offset selector flags (FUN_005FDCC0)
- **MapClass** — likely `int *`:
  - g_Tactical+0xD7C — deferred-rebuild flag
  - Other globals consumed (BridgeMiddle1/2 base addresses, etc.)
- **TechnoTypeClass+0xDCC** — `ZFudgeBridge` (already known).

> **Reminder (CLAUDE.md):** Always check `param_1` type before extracting offsets.
> `int *` indexing must be multiplied by 4 to get byte offset. Bridge-rendering
> functions span CellClass, OverlayTypeClass, MapClass, TacticalClass — each
> may type its `param_1` differently.

### Clamps, rounding, off-by-ones
- Latin-square index: `(cell.ry & 3) * 4 + (cell.rx & 3)` — verify mask is `& 3` not `& 4`.
- Frame index in `DrawOverlay_Shadow`: `frame_count/2 + body_frame` — verify integer division semantics.
- Heightlevel `+4` adjustment in `Cell_ComputeZAdjust` — applied unconditionally for any bridge cell? Verify.

### Edge cases to test
- Cell with `bridge_damage_state` outside `0..=17` range — what does `DrawOverlay_Body` do? (Likely no-op or fallback frame.)
- High-bridge cell after destruction (overlay `0xE7` / `0xE8`) — what's the visible output?
- Low-bridge cell with neighbor mask `0x00` (isolated) — does `SelectBridgeTileVariant_Low` return a sentinel?
- Bridge cell at map edge — boundary behavior in 8-direction mask compute.
- Cell with shroud flag set — does `RecalcBridgeShroudFlags` interact with the draw-time path?

### Timing / ordering
- Per-frame: `TacticalClass::Draw` → layer dispatch order: base_terrain → overlays → smudges → terrain_shadows → shroud_edges → animations → building_overlays. **Bridges draw in base_terrain (deck tile) AND overlays (body+shadow+railing)** — confirm exact ordering.
- Per-tick (sim, not draw): state machine + UpdateRamp_*_* → `UpdateBridgeTile_*` → `UpdateAdjacentBridges_*` → set `g_Tactical+0xD7C` deferred-rebuild flag → next `TacticalClass::Draw` clears the flag and full-redraws.
- Confirm: are there any sim functions that bypass the deferred flag and write `cell+0x118` (DAT_00880940 token) directly? If so, that's a per-frame shortcut.

### TS-legacy flags
- `SpecialFlags & 0x1000` (FogOfWar) — bridge tile selection should NOT depend on this. Verify by grepping for `SpecialFlags` reads in the function inventory.
- TS had only wood low bridges; YR added concrete high bridges. `SelectBridgeTileVariant_Low` may have branches that only fire when `g_BridgeSet == 0` (wood-only theaters). Flag any such branch.
- `FUN_004863D0` immediates 16/20/40 may include TS-era tile-class indices that are dead in YR. Verify each branch via xref tracing.
- `RecalcBridgeShroudFlags` runs every 120 frames; verify this is YR-active (not gated on a TS flag).

### Vtable dispatches
- **None observed at draw time** in light scoping. Confirm during deep decompile that `DrawOverlay_Body`/`Shadow`/`Get_Draw_Offset`/`CellOverlay_TileDraw` are direct calls everywhere they're invoked.
- OverlayTypeClass vtable — not exercised in the bridge tile-selection path under light scoping. Re-verify if `FUN_00483E30` turns out to be a vtable dispatcher.

### Globals / static tables to confirm
- `DAT_0081CC30` — Latin-square (16 entries).
- `g_BridgeSet @ 0xAA0E28`, `g_WoodBridgeSet @ 0xABAD1C` — theater-loaded base tile indices.
- `0x0082A734` (start-height A), `0x0082A774` (walk-direction B), `0x0082A7B4` (end-height C) — orientation tables (16 each).
- `0x0082A7F4` (height-class), `0x0082A89C` (direction-class) — 42 entries each.
- `0x0082A944` — direction table (16) for `SetBridgeDirection`.
- `BridgeMiddle1 @ 0xABAD30`, `BridgeMiddle2 @ 0xAA1028` — 4-variant mid-span tile arrays.
- Corner tile pointers `0xABC2B4..0xAA1540` — 8 pointers (TL/TR/BL/BR × 1/2).
- Pavement-propagation classes `0xABC1E8 / 0xAA0E38` — used by `UpdateRamp_NS_DamageA_High`. **Zero in static binary** (Tasks 13.5/15.5 blocker).
- Tier-4 damage classes `0xAA1548 / 0xAA0740` — used by `ProcessBridgeDestruction_High §12.1`.
- Railing entry tables `0xABC210 / 0xABC2D0` — 16-byte stride. **Element count + indexing formula: open question from prior doc.**
- `ApplyBridgeDestruction_*` inline next-overlay tables (`0x57E7A0`/`0x57ED00`/`0x57DD50`/`0x57E2A0`).
- `DAT_00880940` — per-frame draw-cache invalidator.
- `g_Tactical+0xD7C` — deferred-terrain-rebuild flag.

---

## 5. INI Keys in Scope

INI surface for bridge rendering is **small** — most parameters are hardcoded into the binary. Only 5 keys touch rendering directly.

| Key | Section | Default | Purpose | Currently Parsed in Rust? |
|-----|---------|---------|---------|----------------------------|
| `BridgeVoxelMax` | `[General]` | `3` | Max debris chunks on destruction (visual). | TBD — verify |
| `RepairBridgeSound` | `[General]` | `BridgeRepaired` | Audio cue (not visual). | TBD |
| `BridgeExplosions` | `[General]` | `TWLT026,TWLT036,TWLT050,TWLT070` | Animation list at destruction. | TBD |
| `BridgeStrength` | `[CombatDamage]` | `1500` | HP for damage gating (already in `bridge_warheads.rs`). | **Yes** |
| `DestroyableBridges` | `[CombatDamage]` | `yes` | Master toggle. | **Yes** (via combat path) |

**Per-unit keys (out of tile-selection scope but flagged for completeness):**
- `TooBigToFitUnderBridge` — boolean per-unit (~35 instances). Locomotor / pathfinding.
- `ZFudgeBridge` — int per-unit (2 instances at value `7`). Sprite Z offset; `ComputeZFudge @ 0x4DAFF0`.

**Per-tile sections (`[BRIDGE1]`, `[LOBRDG01..28]`, etc.) are tileset registrations, not selectors.** Each maps section name → `Image=` filename. The renderer-side selection logic does NOT read these at draw time — selection uses tileset_index (an integer) baked into `g_BridgeSet`/`g_WoodBridgeSet` at theater load via the `BridgeSet=` / `WoodBridgeSet=` keys.

**Theater-load keys to verify:**
- `BridgeSet` (string @ `0x00829504`) — base tile index source.
- `WoodBridgeSet` (string @ `0x00829514`) — low-bridge base tile index.
- `WaterBridge` (string @ `0x00829364`) — TileSet config marker.

**Conclusion:** The Phase D renderer needs **no new INI parsing** for tile selection. The visible-tile decision is computed from runtime state (`bridge_damage_state`, overlay byte, neighbor mask, axis, deck level) against hardcoded constants and theater-loaded `g_BridgeSet`/`g_WoodBridgeSet` indices. INI surface is purely metadata (section→filename) which `bridge_atlas.rs` and `overlay_atlas.rs` already consume.

---

## 6. Caller & Integration Map

### Per-frame callers (the path Phase D must hook into)

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|------------|--------------|----------------------------|
| `0x006D3D10` | `Tactical_layer_*` (7 layers) | Once per frame from main loop | YES (#1 — MEDIUM) — confirm layer order |
| `0x006D3470` | `FUN_004D1890` per visible cell | Per frame, once per cell (base_terrain layer) | YES (#2 — MEDIUM) |
| `0x006D3040` | `FUN_006D7C00` per visible cell | Per frame, once per cell (overlays layer) | YES (#3 — MEDIUM) |
| `0x004D1890` | `Get_Draw_Offset`, `DrawOverlay_Body`, `FUN_00483E30`, others | Per cell from base_terrain | YES (#4 — FULL) |
| `0x006D6D10` | `DrawOverlay_Body` + `Shadow` (case 0x14) | Per cell — **but disambiguate vs FUN_004D1890** | YES (#5 — FULL) |
| `0x006D7C00` | `FUN_004802A0` (railing) + 16 other callees | Per cell from overlays | YES (#6 — FULL) |

### Sim-tick callers (the path that mutates state the renderer reads)

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|------------|--------------|----------------------------|
| `Apply_area_damage @ 0x4894B0` | `ProcessBridgeDamageStateMachine_*` | On weapon hit (sim tick) | LIGHT (#42, #43) — confirm display fields written |
| `BlowUpBridge @ 0x47DD70` | `DestroyBridge_High @ 0x57CCF0` + walkers | On final-state cell (sim tick) | LIGHT — already shipped Phase F |
| `LogicClass::PerTickUpdate` | `RecalcBridgeShroudFlags @ 0x578100` | Every 120 frames (sim tick, not render) | LIGHT (#46) — confirm not draw-time |

### Rust integration today

- **Sim side (already shipped, Phase F+G):** `bridge_orchestrator.rs` runs the full damage cascade. Per-cell `BridgeRuntimeState` tracks `damage_state`, `overlay_byte`, `role`, `axis`, `deck_level`. Mutations are committed within a tick.
- **Render side (Phase D, what this investigation feeds):** `bridge_atlas.rs` (high-bridge bodies BRIDGE1/2/B1/B2, 18 frames each) and `overlay_atlas.rs` (generic overlay frames including LOBRDG##) are loaded **statically** at map init. **No code reads `BridgeRuntimeState` at render time — verified by grep `BridgeRuntime\|overlay_byte` in `src/render/` returns 0 hits.** Map-load reads `OverlayEntry.frame` from `[OverlayDataPack]` once and never updates.
- **The gap Phase D must close:** a per-frame (or event-driven) hot path that, given the post-tick `BridgeRuntimeState`, picks the visible tile + frame for each bridge cell and updates the renderer's per-cell display state.

### Callers explicitly NOT investigated

- `BlowUpBridge @ 0x47DD70` and the walker paths (`destroy_bridge_walker_*`) — already shipped in Phase F+G. Out of scope per task description. (Cross-reference only — phase 3 LIGHT touch on `ProcessBridgeDamageStateMachine_*` to confirm no draw-time invocations.)
- `Apply_area_damage @ 0x4894B0` and `IonCannon` retry logic — already shipped, sim-side.
- `PathfinderClass::UpdateBridgePassability @ 0x42ACF0`, `CheckBridgeTraversal @ 0x4D9C60` — pathfinding, not rendering.
- `ComputeBridgeZones @ 0x56D6E0`, `FindBridgeRecord @ 0x56DA10`, `InvalidateBridgeZones @ 0x56DAE0`, `UpdateBridgeZonesHelper @ 0x56C510` — zone graph, already covered by `BRIDGE_ZONE_CONNECTIVITY_GHIDRA_REPORT.md`.

---

## 7. TS-Legacy Risk Register

Consolidated risks. The executor MUST flag every finding's YR-active status in the report.

1. **`UpdateRamp_*_High` and `_Low` overlay-write branches** — gated on globals `DAT_00abad30 / DAT_00aa1028 / DAT_00aa0e28 / DAT_00aa0e38 / DAT_00abc1e8` which are **zero in the static binary**. Live-game capture is needed to populate these (separate task — Tasks 13.5/15.5, not this investigation). The executor must:
   - Document the branch structure (the *if-block* exists, the data is missing).
   - NOT attempt to extract the runtime tile classes from the static binary.
   - Cross-reference the live-debugger task in `docs/plans/` (out-of-scope flag).

2. **`SelectBridgeTileVariant_Low` wood-vs-concrete branches** — TS had only wood; YR added concrete. Branches gated on `g_BridgeSet == 0` (or similar) may be TS-only. Verify each immediate (22/23/24/25/...) is reachable in YR theater configurations.

3. **`FUN_004863D0` classifier immediates 16/20/40** — likely tile-class indices. Verify each branch maps to a YR-live tile class (cross-reference `g_BridgeSet`/`g_WoodBridgeSet` ranges).

4. **`SpecialFlags & 0x1000` (FogOfWar)** — defaults OFF in YR. Verify NO bridge-rendering function reads this flag. (Light scoping found no obvious reads, but exhaustive check during FULL decompile.)

5. **`Cell_ContentRendering` vs `FUN_004D1890`** — both have case 0x14. One may be a TS-era leftover. Disambiguate which fires per-frame for bridges in YR. If `Cell_ContentRendering` is dormant, document and skip it from the renderer model.

6. **`RecalcBridgeShroudFlags @ 0x578100`** runs every 120 frames. Verify it's not gated on FogOfWar or another TS flag.

7. **Bridge railing path (`FUN_00547230` + tables `0xABC210/0xABC2D0`)** — TS had simpler bridges; the railing system may have YR-only entries. Validate table extents are fully populated (no zero-padding region implying TS-vestigial).

8. **High-bridge map-init pass `FUN_0059E740`** — heightlevel ±4 logic; verify both branches fire in YR (TS may have used only one).

---

## 8. Current Rust Implementation Surface

What exists today (from Agent C's source-side scan):

| Path | Purpose | Phase D-relevant gaps |
|------|---------|------------------------|
| `src/sim/bridge_state/mod.rs` | `BridgeRuntimeState`: 512×512 grid, per-cell `damage_state` + `overlay_byte` + `role` + `axis` + `deck_level`. **Already shipped.** | The renderer never reads from this — Phase D must wire it in. |
| `src/sim/bridge_specs.rs` | Ramp state-machine drivers, destruction overlay tables (4 tables × 16 entries, byte-verified). **Shipped.** | Pavement/bridgehead overlay-write branch deferred (Tasks 13.5/15.5) — out of Phase D scope. |
| `src/sim/bridge_state/walker.rs` | HIGH walker entries (`destroy_bridge_high`, classifies overlay byte to NS/EW). **Shipped.** | LOW walkers stubbed for Task 8 — sim-side, not Phase D. |
| `src/sim/world/bridge_orchestrator.rs` | 4-path dispatcher + cascade consumers. **Shipped Phase F+G.** | `update_adjacent_bridges()` is empty (line 208–210). **This is the rim-refresh stub Phase D must resolve** — depends on findings for `UpdateAdjacentBridges_High` (#24) and `_Low` (#25). |
| `src/render/bridge_atlas.rs` | High-bridge body atlas (BRIDGE1/2, BRIDGEB1/2; 18 frames each). | No per-cell selection logic. Phase D must consume this. |
| `src/render/overlay_atlas.rs` | Generic overlay atlas (includes LOBRDG##, BRIDG*, etc.). | No bridge-state-aware lookup. Phase D must add a bridge-specific resolver. |
| `src/map/overlay.rs` | Static `OverlayEntry` parse from `[OverlayPack]`/`[OverlayDataPack]`. | Stores map-load overlays only. Phase D needs a path to bridge runtime state. |
| `src/map/overlay_types.rs` | Overlay metadata, `is_bridge_overlay_index` (hardcoded ranges 24/25/237/238/74–101/122–125/205–232/233–236). | Verify these ranges match the binary's HIGH `0xCD..=0xE6` + final `0xE7/0xE8` and LOW `0x4A..=0x63` + final `0x64/0x65` invariants. **Direct cross-reference target during execution.** |
| `src/bridge_re.rs` | Pure RE-backed helpers (overlay damage step). Mirrors gamemd closed specs. **Not wired to live runtime.** | Reference only — Phase D will likely consume some of this. |

**Key code-side fact (verified by Agent C grep):** `grep -r "BridgeRuntime\|overlay_byte" src/render/` returns **0 matches**. The renderer is fully decoupled from bridge runtime state today.

---

## 9. Deferred Open Questions

These are explicit "figure out during execution" items the scoping pass could not resolve:

1. **Disambiguate `Cell_ContentRendering @ 0x6D6D10` vs `FUN_004D1890 @ 0x4D1890`.** Both have case 0x14 → overlay. Which actually fires for bridges per-frame? (BRR §2.1 attributes case 0x14 to `Cell_ContentRendering` at offset +0x2F1 within the function, but `FUN_004D1890` also has it.)
2. **Q3 — Does `SelectBridgeTileVariant_High` exist?** Light scoping found no symbol. Confirm by either:
   - Exhaustive xref to `g_BridgeSet`, looking for any selector-shaped function, OR
   - Confirming `FUN_0059E740` (map-init) is the only writer of high-bridge `tileset_index` and that runtime mutations only touch `bridge_damage_state` (`cell+0x11E`).
3. **Railing entry table layout (`DAT_00ABC210` / `DAT_00ABC2D0`).** 16-byte stride is known. Need: element count, indexing formula by `(overlay_byte, sub_tile_index)`. Decompile `FUN_00547230` + `FUN_005471F0` end-to-end.
4. **`FUN_004863D0` purpose.** Strong suspect: the central `(overlay_byte → tile-class index)` classifier. Confirm and document the full input→output table.
5. **`FUN_00483E30` purpose.** Recurring shared callee from 4+ render-path functions. Suspected OverlayTypeClass→SHP source resolver — confirm.
6. **`UpdateAdjacentBridges_High/_Low` rim-refresh body.** Does it mutate per-cell visible state, or only mark redraw rects? Resolution determines whether our orchestrator's `update_adjacent_bridges()` stub needs to write `overlay_byte`/`tile_index` or just queue render-side dirty events.
7. **`UpdateRamp_*` overlay-write branches.** Document the *structure* of the `if`-block even though the data globals are zero in the static binary — the structure is needed for Phase D to know which fields can change post-state-transition.
8. **Latin-square invocation context.** The 16-entry table at `0x81CC30` is the frame-jitter source. Confirm it applies to **all healthy high-bridge cells** unconditionally (no per-axis or per-state gate) and exactly once per cell-frame.
9. **OverlayClass / OverlayTypeClass vtable slots used in render.** Light scoping found no virtual dispatch in the bridge tile-selection path. Re-verify during FULL decompile of `FUN_00483E30` and the `FUN_006D7C00` callee chain.
10. **Per-frame draw-cache invalidator `DAT_00880940` semantics.** Is it a monotonic counter? An incrementing token per frame? Used to detect "this cell already drawn this frame"? The renderer model depends on it.

---

## 10. Execution Strategy

**Recommended: Batched subagents per phase.** The 40+-function scope is too large for a single-session `/re-investigate` and the phases have clean boundaries.

### Suggested batching

- **Batch 1A (Phase 1 entry):** #1, #2, #3, #6 — 4 functions, MEDIUM each.
- **Batch 1B (Phase 1 dispatchers):** #4, #5 — disambiguate `FUN_004D1890` vs `Cell_ContentRendering`. **Run sequentially, not parallel** — one informs the other.
- **Batch 1C (Phase 1 body draw):** #7, #8, #9 — the core `DrawOverlay_*` family, FULL/MEDIUM/FULL. Agent must cite the exact mapping `bridge_damage_state` → frame index.
- **Batch 1D (Phase 1 tile + railing):** #10, #11, #12, #13, #14, #15 — overlay Y offset, TMP blit, railing chain.
- **Phase 1 checkpoint:** synthesize Q1/Q2/Q3/Q6 answers. Revise plan if Q3 surprises.
- **Batch 2A (Phase 2 classification core):** #16, #17 — `ApplyBridgeTile` + suspect `FUN_004863D0`.
- **Batch 2B (Phase 2 low-bridge selectors):** #18, #19, #20 — `Select*_Low` family + `UpdateBridgeTile_Low`.
- **Batch 2C (Phase 2 masks + rim refresh):** #21, #22, #24, #25, #26 — adjacency masks, `UpdateAdjacentBridges_*`, `UpdateBridgeEdgeTiles_High`.
- **Batch 2D (Phase 2 map-init + flags):** #27, #28, #29, #30, #31 — `SetBridgeDirection_*`, `FUN_0059E740`, `HasBridgeOverlay`, `Cell_ComputeZAdjust`.
- **Batch 3A (Phase 3 predicates):** #32–#38 — all LIGHT classification predicates.
- **Batch 3B (Phase 3 blitters):** #39, #40, #41 — LIGHT touches.
- **Batch 3C (Phase 3 sim-side cross-ref):** #42, #43, #44 (×8), #45 (×8) — confirm UpdateRamp_*_* display crossover.
- **Batch 3D (Phase 3 contextual edges):** #46, #47, #48.

### Single-session fallback
If batching overhead is too high: run the full plan in 3 sequential `/re-investigate` sessions, one per phase, with the Phase 1 checkpoint enforced.

### Hard constraints during execution
- **No Rust code.** Per `/re-investigate` rules.
- **CellClass param_1 typing must be checked per-function** before extracting offsets (CLAUDE.md decompilation pitfall).
- **Out-of-scope hard fence:** do NOT investigate `BlowUpBridge`, walker paths, `Apply_area_damage`, or any bridge-damage logic — already shipped Phase F+G.
- **Tasks 13.5/15.5 hard fence:** document the *structure* of UpdateRamp_*_* overlay-write branches, but do NOT attempt to extract runtime tile-class globals — they are zero in the static binary and require live-debugger capture (separate task).

---

## 11. Success Criteria

The executed research document at `ra2-rust-game-docs/BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` must:

- **Answer all 7 questions in Section 1** with HIGH confidence and binary citations. Q3 (high-bridge runtime selector existence) is the highest-value answer — Phase D's complexity hinges on it.
- **Include every function in Section 3** (40 entries) or explicitly justify omission with a one-line reason per skip.
- **Resolve every Section 9 deferred question** or re-document as unresolved with the next-step needed (e.g., "needs live-debugger capture").
- **State "Active in YR: Yes / No / Conditional"** for every finding, especially the `UpdateRamp_*` overlay-write branches and any `FUN_004863D0` immediate that decodes to a wood-only or concrete-only tile class.
- **Cite Ghidra addresses for every HIGH-confidence claim.** No "according to YRpp"; no inferred-from-naming claims without binary backing.
- **Include a "Renderer Model" section** that synthesizes the findings into a clean, language-agnostic data flow:
  - Inputs: post-tick `BridgeRuntimeState` per cell + map-init `tile_index` (high) or computed `tile_index` (low) + neighbor mask.
  - Decision: which selector function applies (degenerate for high, `Select*_Low` for low).
  - Output: visible tile name + frame index + Y offset + Z-adjust + railing entries.
  - Caveats: rim-refresh propagation rules, deferred-rebuild flag interaction, Latin-square jitter scope.
- **Provide an explicit Phase D renderer integration sketch** (NOT Rust code — pseudocode or data-flow diagram) showing where in `advance_tick` (or post-tick) the bridge tile re-evaluation should fire, what `BridgeRuntimeState` fields it reads, and what render-side fields it writes.

---

## Sources

- **Ghidra addresses sampled (live, read-only this session):** `0x6D3D10`, `0x6D3470`, `0x6D3040`, `0x4D1890`, `0x6D6D10`, `0x6D7C00`, `0x47F6A0`, `0x47F510`, `0x480110`, `0x5FDCC0`, `0x480350`, `0x4802A0`, `0x547230`, `0x5471F0`, `0x483E30`, `0x57B440`, `0x4863D0`, `0x57ACF0`, `0x579620`, `0x57A430`, `0x57B210`, `0x579B70`, `0x576770`, `0x571050`, `0x576200`, `0x47E040`, `0x47E470`, `0x59E740`, `0x4865D0`, `0x484680`, `0x486750`, `0x486770`, `0x484AB0`, `0x485060`, `0x574600`, `0x5746C0`, `0x578D80`, `0x4AED70`, `0x547CF0`, `0x490B90`, `0x576BA0`, `0x571490`, `0x572230..0x573170`, `0x56ED40..0x56FC80`, `0x578100`, `0x4AF470`, `0x4DAFF0`. Plus globals at `0x81CC30`, `0xAA0E28`, `0xABAD1C`, `0x82A734..0x82A944`, `0xABAD30`, `0xAA1028`, `0xABC1E8`, `0xAA0E38`, `0xABC2B4..0xAA1540`, `0xABC210`, `0xABC2D0`, `0x880940`, `0x887324+0xD7C`.
- **Docs searched:**
  - `ra2-rust-game-docs/BRIDGE_RENDERING_GHIDRA_REPORT.md`
  - `ra2-rust-game-docs/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`
  - `ra2-rust-game-docs/BRIDGE_SYSTEM.md`
  - `ra2-rust-game-docs/CELLCLASS_ZONES_SPEED_BRIDGES.md`
  - `docs/plans/2026-05-07-bridges-tier2-phase-f-orchestrator-design.md`
  - `docs/plans/2026-05-07-bridges-tier2-damage-state-machine-design.md` and `-plan.md`
  - `docs/plans/2026-05-07-bridges-tier2-task-13-redesign-design.md` and `-plan.md`
  - `docs/plans/2026-05-07-bridges-tier2-task-15-redesign-design.md` and `-plan.md`
  - `docs/gap-scans/2026-05-06-gap-scan-bridges-deep.md`
  - `docs/gap-scans/2026-05-06-gap-scan-cellclass.md`
  - `ra2-rust-game-docs/BRIDGE_ZONE_CONNECTIVITY_GHIDRA_REPORT.md` (cross-reference only)
- **INI files checked:** `ini/rulesmd.ini`, `ini/rules.ini`, `ini/artmd.ini`, `ini/art.ini`.
- **Rust source surface mapped:** `src/sim/bridge_state/`, `src/sim/world/bridge_orchestrator.rs`, `src/sim/bridge_specs.rs`, `src/render/bridge_atlas.rs`, `src/render/overlay_atlas.rs`, `src/map/overlay.rs`, `src/map/overlay_types.rs`, `src/bridge_re.rs`, `src/rules/bridge_warheads.rs`.
- **Related plans:** none other — this is the first display-table investigation plan; Tier 2 sim-side plans (Phases B/C/E/F/G) all defer renderer to Phase D.
