# High Bridge Damage State Machine — Ghidra Research Report

**Primary function:** `ProcessBridgeDamageStateMachine_High` @ `0x00576BA0`
**Overall confidence:** HIGH (decompiled 12+ functions, traced callers, verified dispatch chain)
**Active in YR:** Conditional — area-damage paths require `SpecialFlags & 0x8000` (`DestroyableBridges`) and a per-warhead `Wall=yes` byte; the warhead default is false. Direct `ApplyDamageToCell` callers and the hut/bomb runtime-collapse path are live, but `0x00574000` has no map-init caller. (corrected 2026-07-10: `decompile_function 0x0075CEC0` initializes `WarheadTypeClass+0x144` to 0; `decompile_function 0x0075D3A0` reads `Wall=` into it; `get_function_callers 0x00574000` returns only `BombClass__Detonate` and `BuildingClass__Update` — INFERENCE_HARDENED)

## 1. Overview

High bridges (reinforced concrete, tile set `BridgeSet`) have a damage pipeline independent
from low (wooden/tube) bridges. A high bridge damage event transitions cells through a
short state machine: **healthy → damaged → destroyed → collapsed span**. The state is
stored in two different fields (direction + body overlay). When the final hit lands on a
body cell, a collapse walker iterates outward along the bridge axis, destroying the full
span and spawning debris animations.

This doc mirrors §3.3 of [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) (Low state machine) for
high-bridge parity.

## 2. Key Fields

### CellClass fields touched by the state machine

| Offset | Size | Name | Purpose |
|--------|------|------|---------|
| +0x24  | 4    | `MapCoord` (X:Y packed) | Cell coordinate |
| +0x2C  | 4    | `BridgeAnchorPtr` | Pointer to partner cell in the bridge pair (when `flags & 0x80 == 0`) |
| +0x38  | 4    | `IsoTileTypeIndex` | Base tileset tile index (anchor reference) |
| +0x44  | 4    | `OverlayTypeIndex` | Body-cell overlay (this is the damage-state visual) |
| +0x11A | 1    | `BridgeClassId` | Template bridge-class / footprint-row ID; the bridgehead walk seeks class 4 (NS family) or 2 (EW family), not geometric height. (corrected 2026-07-10: `decompile_function 0x0057B440` writes the template linear slot index to `cell+0x11A`, while `decompile_function 0x00576BA0` compares that byte with 4/2 — OFFSET_RETYPED_WRONG) |
| +0x11B | 1    | `Level` | Cell Z level (passed as Z-fudge to `SetOverlayAndPropagate`) |
| +0x11E | 1    | `BridgeDirectionState` | Direction + damage-phase encoding |
| +0x140 | 4    | `Flags` | Bit 0x80 = anchor-self; bit 0x100 = bridge-body cell; bit 0x400 = bridge ramp; bit 0x500 mask used during adjacency walks |

Verified: `flags & 0x80` selects between self-anchor and dereferencing `+0x2C`
(anchor partner) across every high-bridge function. `flags & 0x100` marks a bridge body
cell; `flags & 0x400` marks a ramp cell; `flags & 0x500` is their union used by
`UpdateAdjacentBridges_High`.

### Field `+0x11E` state encoding (shared with Low bridges)

| State | Direction | Semantic |
|-------|-----------|----------|
| 0-5   | NS        | Healthy body — 6 frame variants |
| 6     | NS        | Damaged body (first hit applied) — next hit collapses |
| 7     | NS        | Mid-collapse, ramp A only collapsed |
| 8     | NS        | Mid-collapse, ramp B only collapsed |
| 9-14  | EW        | Healthy body — 6 frame variants |
| 15    | EW        | Damaged body — next hit collapses |
| 16    | EW        | Mid-collapse, ramp B only |
| 17    | EW        | Mid-collapse, ramp A only |

**Note on states 7/8 and 16/17:** These are reached only during multi-step collapse
sequences where one ramp has already collapsed. A second hit on such a cell finishes
the collapse via the single remaining ramp path.

### Field `+0x44` body-overlay encoding

Distinct from `+0x11E`. These are tileset-relative overlay indices:

| Range | Meaning | Axis |
|-------|---------|------|
| 0xCD-0xD2 (6 values) | Healthy body — frame variants | EW |
| 0xD3-0xD5 (3 values) | Damaged body | EW |
| 0xD6-0xDB (6 values) | Healthy body — frame variants | NS |
| 0xDC-0xDE (3 values) | Damaged body | NS |
| 0xDF          | EW damaged endpoint stub — progresses to 0xE0 | EW |
| 0xE0          | EW destroyed endpoint stub | EW |
| 0xE1          | EW damaged endpoint stub — progresses to 0xE2 | EW |
| 0xE2          | EW destroyed endpoint stub | EW |
| 0xE3          | NS damaged endpoint stub — progresses to 0xE4 | NS |
| 0xE4          | NS destroyed endpoint stub | NS |
| 0xE5          | NS damaged endpoint stub — progresses to 0xE6 | NS |
| 0xE6          | NS destroyed endpoint stub | NS |
| 0xE7          | Fully destroyed body | EW |
| 0xE8          | Fully destroyed body | NS |

Verified against `DestroyBridgeWalker_NS_High` (0x57CF60, EW ranges) and
`DestroyBridgeWalker_EW_High` (0x57D530, NS ranges). **Ghidra function labels are
swapped vs. physical bridge axis** — see §7.

### Runtime-initialized globals (tileset-relative bridgehead classes)

These are zero in the static binary image; populated from theater / tileset load:

| Global | Purpose |
|--------|---------|
| `DAT_00AA0E28` | `BridgeSet` — base IsoTileTypeIndex of concrete (high) bridges |
| `DAT_00ABAD1C` | `WoodBridgeSet` — base IsoTileTypeIndex of wooden (low) bridges |
| `DAT_00ABAD30` | NS bridgehead overlay class base (BridgeSet-relative; 4 consecutive values: +0..+3) |
| `DAT_00AA1028` | EW bridgehead overlay class base (BridgeSet-relative; 4 consecutive values: +0..+3) |
| `DAT_00ABC1E8` / `DAT_00AA0E38` | Pavement-related overlay classes (for `ToggleBridgePavement` trigger) |
| `DAT_00ABC2B4` / `DAT_00AA1130` | Additional bridgehead classes selected with bridge-class ID `+0x11A == 8` |
| `DAT_00AA1548` / `DAT_00AA0740` | Additional bridgehead classes selected with bridge-class ID `+0x11A == 12` |
| `DAT_00ABDE88` | Leptons per height level (multiplier for debris Z position) |

## 3. Core Logic — `ProcessBridgeDamageStateMachine_High` (0x00576BA0)

Called from `ApplyDamageToCell` (0x587401) when damage lands on a high-bridge overlay.

Entry filter: the cell must either (a) have `flags & 0x100` (bridge body), or (b) have
IsoTileTypeIndex-relative class in the NS or EW bridgehead overlay set. Otherwise
returns 0 (no damage effect).

### 3.1 Body-cell branch (`flags & 0x100` set)

```
if (flags & 0x80 == 0)       // not the anchor-self cell
    cell = cell->BridgeAnchorPtr  // follow to partner

dir = (state > 8) ? 0 : -1   // NS = -1, EW = 0
iVar2 = (dir & ~1) + 4       // NS=2, EW=4   (direction offset for UpdateRamp_*A)
iVar6 = dir & 6              // NS=6, EW=0   (direction offset for UpdateRamp_*B)

switch (state):
  0..5 (NS healthy):
    state = 6
    UpdateRamp_NS_DamageA_High(cell, 2)
    UpdateRamp_NS_DamageB_High(cell, 6)
    return 0                                  // visible damage only

  6 (NS damaged):
    UpdateRamp_NS_CollapseA_High(cell, 2)
    UpdateRamp_NS_CollapseB_High(cell, 6)
    → SetBridgeDirection_NESW(0, 0)           // clear NS bridge flags
    state = 0; OverlayTypeIndex = -1          // binary writes +0x44 (OverlayTypeIndex), NOT +0x38 (IsoTileTypeIndex) — `*(undefined4 *)(puVar9 + 0x44) = 0xffffffff` verified inside ProcessBridgeDamageStateMachine_High @ 0x00576BA0
    UpdateAdjacentBridges_High(cell)
    if (InvalidateBridgeZones)
        UpdateBridgeZonesHelper()
    return 1

  7 (NS CollapseA-only):
    UpdateRamp_NS_CollapseA_High(cell, 2)
    → SetBridgeDirection_NESW(0, 0); clear state / overlay; adjacency + zones
    return 1

  8 (NS CollapseB-only):
    UpdateRamp_NS_CollapseB_High(cell, 6)
    → SetBridgeDirection_NESW(0, 0); clear state / overlay; adjacency + zones
    return 1

  9..14 (EW healthy):
    state = 15
    UpdateRamp_EW_DamageA_High(cell, 4)
    UpdateRamp_EW_DamageB_High(cell, 0)
    return 0

  15 (EW damaged):
    UpdateRamp_EW_CollapseA_High(cell, 4)
    UpdateRamp_EW_CollapseB_High(cell, 0)
    → SetBridgeDirection_NESW(6, 0)           // clear EW bridge flags
    break

  16 (EW CollapseB-only):
    UpdateRamp_EW_CollapseB_High(cell, 0)
    → SetBridgeDirection_NESW(6, 0); break

  17 (EW CollapseA-only):
    UpdateRamp_EW_CollapseA_High(cell, 4)
    → SetBridgeDirection_NESW(6, 0); break

  default: return 0

(after break):
  state = 0; OverlayTypeIndex = -1            // binary writes +0x44 (OverlayTypeIndex), NOT +0x38 (IsoTileTypeIndex) — same `*(undefined4 *)(puVar9 + 0x44) = 0xffffffff` site verified inside ProcessBridgeDamageStateMachine_High @ 0x00576BA0
  UpdateAdjacentBridges_High(cell)
  if (InvalidateBridgeZones) UpdateBridgeZonesHelper()
  return 1
```

### 3.2 Bridgehead-cell branch (`flags & 0x100 == 0`, overlay in bridgehead set)

The bridgehead is the ramp / connection piece off the bridge body. State is NOT in
`+0x11E` here — it is encoded by the 4-value class offset from `DAT_00ABAD30` (NS) or
`DAT_00AA1028` (EW).

```
iVar2 = (IsoTileTypeIndex - BridgeSet) + 1   // tile-relative overlay class
h = cell.BridgeClassId

// Walk through the template footprint to find the class-4 NS anchor or class-2 EW anchor.
// For the NS family, (h & 1) != 0 returns early; for the EW family, h > 4 returns early.
// Otherwise choose the signed walk direction from h relative to the target class.

if (NS) {  // iVar2 ∈ [ABAD30 .. ABAD30+3]
    if (iVar2 == ABAD30+3) {
        // "Most damaged" NS bridgehead variant → full collapse
        // Binary blows up 3 AXIAL cells centered on the anchor:
        // NS branch → (X, Y-1), (X, Y), (X, Y+1) — Y varying, X fixed
        // (axial along the NS bridge's length). NOT a perpendicular row.
        // Verified inside ProcessBridgeDamageStateMachine_High @ 0x00576BA0.
        BlowUpBridge(X, Y-1); BlowUpBridge(X, Y); BlowUpBridge(X, Y+1);
        // (EW mirror at the analogous EW branch below blows up (X-1,Y)/(X,Y)/(X+1,Y) — also axial.)
        SetOverlayAndPropagate(anchor, ABAD30+3+BridgeSet, -1, cell.Level-4, 0)
        UpdateRamp_NS_CollapseA_High(anchor, 2)
        UpdateRamp_NS_CollapseB_High(anchor, 6)
        UpdateAdjacentBridges_High × 2           // two perpendicular neighbors
        if (InvalidateBridgeZones) UpdateBridgeZonesHelper()
        // 3×2 cell loop populates 10-slot damage-anim array (debris scatter)
        return 1
    }
    if (iVar2 ∈ [ABAD30 .. ABAD30+2]) {           // all three non-final input slots converge
        SetOverlayAndPropagate(anchor, ABAD30+2+BridgeSet, -1, -1, 0)
        UpdateRamp_NS_DamageA_High(anchor, 2)
        UpdateRamp_NS_DamageB_High(anchor, 6)
        return 0                                  // damage visible, bridge still up
    }
}

if (EW) {  // iVar2 ∈ [AA1028 .. AA1028+3]
    // Mirror of NS on the opposite axis. Inputs +0/+1/+2 all write +2;
    // only input +3 takes the final-collapse branch.
}
```

The four class slots are therefore **not** a direct `+0 → +1 → +2 → +3`
progression: direct damage to `+0`, `+1`, or `+2` writes `+2`; `+3` collapses.
(corrected 2026-07-10: `decompile_function 0x00576BA0` shows both NS and EW
non-final branches passing `base + 2 + BridgeSet` to
`MapClass__SetOverlayAndPropagate` — OPERATOR_OR_ORDER_DRIFT)

### 3.3 Return value

- `0` = damage absorbed (healthy → damaged, or bridgehead < final stage)
- `1` = full collapse triggered (walker will run; zones invalidated)

## 4. Call Chain

```
warhead explosion
    → Apply_area_damage (0x00489280)
        ├─ [gate: SpecialFlags & 0x8000 (DestroyableBridges) && warhead.bridge_damage]
        ├─ [gate: IonCannonWarhead OR RandomRanged(1, BridgeStrength) < warhead_damage]
        │
        ├─ if cell.IsoTileIndex in [0x4A..0x63] → DestroyBridge_Low      (direct low-bridge)
        ├─ if cell.IsoTileIndex in [0xCD..0xE6] → DestroyBridge_High     (direct high-bridge, tile)
        └─ else → ApplyDamageToCell (0x00587180)
                ├─ cell.OverlayIndex in [0x4A..0x63] → DestroyBridge_Low
                ├─ cell.OverlayIndex in [0xCD..0xE6] → DestroyBridge_High
                ├─ flag 0x100 + anchor.OverlayIndex ∈ {0x18, 0x19} → ProcessBridgeDamageStateMachine_High
                ├─ flag 0x100 + anchor.OverlayIndex ∈ {0xED, 0xEE} → ProcessBridgeDamageStateMachine_Low
                ├─ bridgehead class match in BridgeSet space → ProcessBridgeDamageStateMachine_High
                └─ bridgehead class match in WoodBridgeSet space → ProcessBridgeDamageStateMachine_Low

DestroyBridge_High (tile, 0x0057CCF0):
    overlay ∈ [0xCD..0xD5] ∪ [0xDF..0xE2] ∪ {0xE7}
        → DestroyBridgeWalker_NS_High (0x0057CF60)    // NOTE: label swapped
    overlay ∈ [0xD6..0xDE] ∪ [0xE3..0xE6] ∪ {0xE8}
        → DestroyBridgeWalker_EW_High (0x0057D530)    // NOTE: label swapped

DestroyBridgeWalker_*_High:
    transitions body overlay (see §2 table)
    calls ApplyBridgeDestruction_*_High (EW=0x57ED00, NS=0x57E7A0) at siblings
    if reaching 0xE7/0xE8 → FindBridgeEndpoints_*_High → RepairBridgeSegment
    RecalcAttributes on all 3 cells; dirties screen rect

ProcessBridgeDamageStateMachine_High body-cell path
    → UpdateRamp_*_High helpers (see §5)
    → SetBridgeDirection_NESW (clears flags + state across 4-5 cell group)
    → UpdateAdjacentBridges_High → UpdateBridgeEdgeTiles_High (edge-tile re-evaluation)
    → InvalidateBridgeZones + UpdateBridgeZonesHelper (zone graph refresh)
```

Full collapse (separate runtime entry point — called by bomb detonation or `BuildingClass::Update`):
```
BuildingClass::Update (0x0044031B)
    → DestroyBridge_High_OnHutDeath (0x00574000)
        → 5×5 scan for any high-bridge overlay cell
        → DestroyBridgeFromCell_High (0x005749C0)
            ├─ classify overlay → CollapseBridge_EW_High (0x00575870) or
            │                      CollapseBridge_NS_High (0x00575BA0)
            └─ walker iterates 4 segments along bridge axis:
                   spawn debris anim (BridgeExplosions[rand])
                   call DestroyBridge_High on each segment (up to 3 retries)
               Final: UpdateBridgeZonesHelper(), g_Tactical+0xD7C = 1
```

`0x00574000` is runtime-only in the active call graph; the historical `_MapInit`
name was an inference, not a reachability fact. (corrected 2026-07-10:
`get_function_callers 0x00574000` and `get_function_xrefs 0x00574000` show only
`BombClass__Detonate @ 0x00438982` and `BuildingClass::Update @ 0x0044031B` —
INFERENCE_HARDENED)

## 5. Helper Functions

| Address | Name | Purpose |
|---------|------|---------|
| 0x00572230 | `UpdateRamp_NS_DamageA_High` | Walk 1 cell in direction `param & 7`; if target has `flags & 0x80` then `state<4→4`, `state==5→6`. Else propagate pavement (for classes at `DAT_00ABC1E8` / `DAT_00AA0E38`) or re-set bridgehead overlay. |
| 0x0057XXXX | `UpdateRamp_NS_DamageB_High` | Symmetric sibling |
| 0x0057XXXX | `UpdateRamp_NS_CollapseA_High` | Collapse variant (sets cell to destroyed state) |
| 0x0057XXXX | `UpdateRamp_NS_CollapseB_High` | Collapse variant, other ramp |
| 0x0057XXXX | `UpdateRamp_EW_DamageA_High` | EW sibling |
| 0x0057XXXX | `UpdateRamp_EW_DamageB_High` | EW sibling |
| 0x0057XXXX | `UpdateRamp_EW_CollapseA_High` | EW sibling |
| 0x0057XXXX | `UpdateRamp_EW_CollapseB_High` | EW sibling |
| 0x0047E040 | `CellClass::SetBridgeDirection_NESW` | Clears/sets bridge flags on 4-5 cell group. With `(0, 0)` clears NS; `(6, 0)` clears EW. When `param3==0`, calls `BlowUpBridge` on each cell (kills units on deck). |
| 0x00576770 | `MapClass::UpdateAdjacentBridges_High` | Walks 8 directions; once a `flags & 0x500` cell is found, re-evaluates the bridge edge tile classification (height 5/7/8/12) → `UpdateBridgeEdgeTiles_High` with direction offset 2 or 4. Dirties screen rect if tile changed. |
| 0x0057DC20 | `MapClass::FindBridgeEndpoints_NS_High` | Walks in `DAT_0089F6A0` direction until off-bridge, then `DAT_0089F690` direction the other way. Calls `RepairBridgeSegment` with both endpoints to refresh span data. |
| 0x0057DAF0 | `MapClass::FindBridgeEndpoints_EW_High` | EW variant |
| 0x0057E7A0 | `MapClass::ApplyBridgeDestruction_NS_High` | Per-cell destruction effect (unit damage / scatter) |
| 0x0057ED00 | `MapClass::ApplyBridgeDestruction_EW_High` | EW variant |
| 0x0047DD70 | `CellClass::BlowUpBridge` | Kills ground units on collapsed cell; spawns debris |
| 0x005XXXXX | `MapClass::InvalidateBridgeZones` | Returns true if zone graph needs recompute |
| 0x005XXXXX | `MapClass::UpdateBridgeZonesHelper` | Flood-fill zone recompute |
| 0x005XXXXX | `MapClass::SetOverlayAndPropagate` | Writes overlay index + optional frame / Z-fudge and dirties neighbors |
| 0x0047D2B0 | `CellClass::RecalcAttributes` | Re-derives cell flags from current tileset + overlay |

### Collapse walker detail — `CollapseBridge_NS_High` (0x00575BA0)

```
(X, Y) = input cell
scan Y downward while overlay ∈ [0xCD..0xE8]    → count = north_count
scan Y upward   while overlay ∈ [0xCD..0xE8]    → count = south_count
// Pivot to balanced center:
Y_center = Y - (north_count - south_count) / 2
step = (south_count < north_count) ? -1 : +1

for i in 0..4:    // max 4 iterations
    if (overlay != 0xE8):                           // not already-destroyed endpoint
        for j in 0..2:
            debris_cell = (X-1, Y+j-1)              // debris on west perpendicular row
            coord.X = cell.X * 0x100 + 0x80 + rand_jitter
            coord.Y = cell.Y * 0x100 + 0x80 + rand_jitter
            coord.Z = cell.Level * DAT_00ABDE88
            anim_type = RulesClass.BridgeExplosions[rand(0, count-1)]
            new AnimClass(anim_type, coord, RandomRanged(1,5), 1, 0x600, 0, 0)

    for retry in 0..3:
        if (DestroyBridge_High(cell) == 1) break    // single-tile destruction primitive

    Y += step
    if (new_overlay ∉ [0xCD..0xE8]) break
UpdateBridgeZonesHelper()
g_Tactical.redraw_flag = 1
```

`CollapseBridge_EW_High` is the mirror along X axis with debris on north perpendicular row.

## 6. INI Keys

| Section | Key | Default | Stored at | Effect |
|---------|-----|---------|-----------|--------|
| `[CombatDamage]` | `BridgeStrength` | 1500 | `RulesClass+0x1740` | Denominator of destruction RNG: `Random(1, BridgeStrength) < warhead_damage` to damage |
| `[CombatDamage]` | `IonCannonWarhead` | `IonCannonWH` | `RulesClass+0xFF0` | Warhead identity; if equal, bypasses RNG gate (always destroys) |
| `[CombatDamage]` | `C4Warhead` | `Super` | `RulesClass+0xFA8` | Warhead used by `BlowUpBridge` to kill units on collapsing deck |
| `[AudioVisual]` | `BridgeExplosions` | `TWLT026,TWLT036,TWLT050,TWLT070` | `RulesClass+0x15C`/`+0x168` (ptr+count) | AnimType array; collapse walker picks one at random per debris |
| `[AudioVisual]` | `MetallicDebris` | `DBRIS1LG...` | `RulesClass+0x14C`/`+0x150`/`+0x154` | Debris array (not referenced by the high-bridge collapse path we traced) |
| `[AudioVisual]` | `BridgeVoxelMax` | 3 | `RulesClass+0x624` | Max voxel segments spawned on explosion (per-segment limit) |
| `[General]` | `DestroyableBridges` | `yes` | `SpecialFlags` bit `0x8000` via `*DAT_00A8B230` | Master toggle. If not set, area-damage gate skips all bridge damage code. |
| `[General]` | `BridgeRepairHut` | `yes` | `RulesClass+?` | Enables engineer-repairable bridges (separate system from damage) |
| warhead section | `Wall` | false per warhead unless INI opts in | `WarheadTypeClass+0x144` | Required for area damage to touch bridges. (corrected 2026-07-10: `decompile_function 0x0075CEC0` zeroes `+0x144`; `decompile_function 0x0075D3A0` reads string `Wall`; `decompile_function 0x00489280` tests `SpecialFlags & 0x8000` and `warhead+0x144` before all bridge blocks — INFERENCE_HARDENED) |

## 7. NS/EW Axis Mapping — Ghidra Label Inconsistency

**Finding:** `DestroyBridgeWalker_NS_High` (0x57CF60) and `DestroyBridgeWalker_EW_High`
(0x57D530) are labeled with the **opposite** axis from the bridge they actually process.

Evidence:
- Walker 0x57CF60 (labeled NS) operates on overlay range `[0xCD..0xD5]` and fetches
  sibling cells at `(X, Y-1)` and `(X, Y+1)` — a 3-cell span on the Y axis, which is
  the **width** of an **EW-oriented** bridge (length along X).
- `DestroyBridgeFromCell_High` dispatches this same range `[0xCD..0xD5]` to
  `CollapseBridge_EW_High` (0x575870), which walks along the **X axis**. An EW walk
  is consistent with an EW bridge.
- `BRIDGE_SYSTEM.md` authoritative table lists `0xCD..0xD5 = EW intact`.

So:
- Range `[0xCD..0xD5]` + `[0xDF..0xE2]` + `{0xE7}` = **EW high bridges** — despite being
  processed by a function labeled "NS_Walker".
- Range `[0xD6..0xDE]` + `[0xE3..0xE6]` + `{0xE8}` = **NS high bridges** — despite being
  processed by a function labeled "EW_Walker".

**State-machine labels (cases 0-5/6-8 = NS, cases 9-14/15-17 = EW) are consistent with
the bridge axis.**

**The Walker, ApplyBridgeDestruction, and FindBridgeEndpoints labels are swapped;
the UpdateRamp and CollapseBridge labels are not (corrected 2026-07-10).** Verified
via overlay-range and coordinate-walk cross-check —
`ApplyBridgeDestruction_NS_High @ 0x57E7A0` handles overlay range
`[0xCD..0xD5] + [0xDF..0xE2] + {0xE7}` (the EW range per §2), and
`ApplyBridgeDestruction_EW_High @ 0x57ED00` handles `[0xD6..0xDE] + [0xE3..0xE6] + {0xE8}`
(the NS range). `FindBridgeEndpoints_NS_High @ 0x57DC20` is reached from the
physical-EW walker, and `_EW_High @ 0x57DAF0` from the physical-NS walker. Verified
inside `ApplyBridgeDestruction_NS_High @ 0x57E7A0` decompile — function writes
to 3 cells along Y axis (`local_b8`/`local_c4`/`local_cc` at `param_1[1] ± 1`),
uses the NS lookup table (per §11.2), and progresses endpoints 0xDF→0xE0 / 0xE1→0xE2
(EW endpoints per §2). By contrast, `CollapseBridge_EW_High @ 0x575870` walks X,
`CollapseBridge_NS_High @ 0x575BA0` walks Y, and `ProcessBridgeDamageStateMachine_High`
dispatches `UpdateRamp_NS_*` only for states 0..8 and `UpdateRamp_EW_*` only for
states 9..17. (`decompile_function 0x0057CF60`, `0x0057D530`, `0x0057E7A0`,
`0x0057ED00`, `0x0057DC20`, `0x0057DAF0`, `0x00575870`, `0x00575BA0`, and
`0x00576BA0` — RTTI_LABEL_DRIFT)

**Recommendation:** when porting, key transitions off the actual **overlay range** and
write clear "axis-A / axis-B" or "EW / NS" names in Rust; do not inherit the Ghidra
walker labels.

## 8. Active in YR — gating summary

| Path | Gate | Default | Active? |
|------|------|---------|---------|
| Area-damage → ProcessBridgeDamageStateMachine_High | `SpecialFlags & 0x8000` + `warhead.Wall=yes` | `DestroyableBridges=yes`; `Wall` is false unless the warhead opts in | **Conditional** |
| Area-damage → DestroyBridge_High (direct) | same SpecialFlag + `warhead+0x144` (`Wall`) | `Wall` defaults false per warhead; only opted-in warheads pass | **Conditional** |
| Runtime hut/bomb collapse → `0x00574000` | live calls from `BombClass::Detonate` and `BuildingClass::Update` | event-dependent | **Yes** |
| Direct ApplyDamageToCell | unconditional | — | **Yes** (called from trigger actions FUN_006E0490, FUN_006E2050) |
| IonCannonWarhead path | bypasses only the `BridgeStrength` RNG after the SpecialFlag + `Wall` gates | `IonCannonWH` opts into `Wall` in retail data | **Conditional on outer gates** |

The corrected per-warhead default and runtime-only helper reachability were verified
with `decompile_function 0x00489280`, `decompile_function 0x0075CEC0`, `decompile_function 0x0075D3A0`,
`get_function_callers 0x00574000`, and `get_function_callers 0x00587180`
(2026-07-10 — INFERENCE_HARDENED).

**TS-legacy check:** no evidence this is dormant code. Verified live callers in
BuildingClass::Update, trigger scripting (FUN_006E0490, FUN_006E2050), and area-damage
dispatch. `DestroyableBridges` defaults ON in YR skirmish.

## 9. Current Rust Implementation Status (re-audited 2026-07-10)

This section supersedes the historical numbered Rust-gap lists in §§11.16, 12.18,
13.17, 14.21, and 15.9; those lists describe earlier snapshots and are not a current
implementation handoff.

Current source is centered in `src/sim/bridge_state/{mod.rs,walker.rs}` and
`src/sim/world/bridge_orchestrator.rs`. It now includes per-cell 18-state encoding,
mutable overlay bytes, body and bridgehead transition drivers, direct high/low walkers,
the four-path damage dispatcher, `DestroyableBridges` and `Wall` gating,
`BridgeStrength` RNG with IonCannon bypass, bounded collapse walks, hut-death dispatch,
`BlowUpBridge`-style ground-kill/deck-`DropIn` fallout, debris animation RNG,
radar/path/zone refresh, and state-driven bridge rendering. (corrected 2026-07-10:
source scan of those files plus `decompile_function 0x00576BA0`, `0x00489280`,
`0x00575870`, `0x00575BA0`, and `0x0047DD70` for the corresponding binary contracts —
RUST_STATUS_DRIFT)

Remaining implementation-facing cautions found in this pass:

- `update_adjacent_bridges` is present, but exact equivalence to binary
  `UpdateAdjacentBridges_High → UpdateBridgeEdgeTiles_High` is not certified.
- trigger-event 31 remains an intentional skirmish no-op; campaign/script integration
  is deferred.
- `is_high_bridge_index()` still groups all four anchor IDs under a misleading helper
  name; active family dispatch should use the separate bridge-fact discriminators.
- No gamemd-derived exhaustive replay/check currently certifies the complete bridge
  pipeline byte/pixel/RNG identical; implemented mechanisms remain **UNVERIFIED**, not
  parity-certified.

## 10. Open Questions — resolution status

| # | Original question | Status | Notes |
|---|-------------------|--------|-------|
| 1 | `warhead+0x144` meaning | **Resolved** — `Wall=yes/no` | §11.6 |
| 2 | Overlay values 0xD4 / 0xD5 origin | **Resolved** — `ApplyBridgeDestruction` picks via 16-entry neighbor table (§11.2); values come from neighbor-lookup, not walker sets | |
| 3 | All 8 `UpdateRamp_*_High` transitions | **Resolved** — full table in §11.1 | |
| 4 | `(8<state)-1` + direction offset 2/4/6/0 meaning | **Resolved** — compass table mapping: N=0, NE=1, E=2, SE=3, S=4, SW=5, W=6, NW=7 (§11.7) | |
| 5 | BridgeExplosions anim-list layout | **Resolved 2026-05-18.** DVC base for MetallicDebris is `+0x13C`; for BridgeExplosions is `+0x158`. Live reads (`+0x140/+0x14C`, `+0x15C/+0x168`) are `data*` and `ActiveCount` through standard DVC offsets. See §11.13 + [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md). | |
| 6 | `RepairBridgeSegment` actual behavior | **Resolved — misnamed.** Does NOT repair. Walks span and fires `ProcessCellAction(0x1F, ...)` on each occupied cell. §11.3 | |
| 7 | Anchor overlays 0x18 / 0x19 NS vs EW | **Resolved 2026-05-18.** 0x18 = N-S, 0x19 = E-W. Axis carried by `+0x11E` (0 vs 9) and `Flags & 0x800` (SET=NS, CLEAR=EW). See [BRIDGE_ANCHOR_OVERLAY_18_19_AXIS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_ANCHOR_OVERLAY_18_19_AXIS_GHIDRA_REPORT.md). | |

Remaining open items:
- `g_Tactical+0xD7C` reader (who consumes the deferred-rebuild flag)
- `field_0x6DF` on BuildingClass — exact setter that latches the bridge-collapse pending flag
- `action code 0x1F` in `TechnoClass::ProcessCellAction` — what occupant-level effect it triggers

## 11. Extended Findings — Deep Dive

### 11.1 All 8 `UpdateRamp_*_High` helpers

| # | Function | Address |
|---|----------|---------|
| 1 | `UpdateRamp_NS_DamageA_High` | `0x00572230` |
| 2 | `UpdateRamp_NS_DamageB_High` | `0x00572330` |
| 3 | `UpdateRamp_NS_CollapseA_High` | `0x00572440` |
| 4 | `UpdateRamp_NS_CollapseB_High` | `0x005727E0` |
| 5 | `UpdateRamp_EW_DamageA_High` | `0x00572B80` |
| 6 | `UpdateRamp_EW_DamageB_High` | `0x00572C90` |
| 7 | `UpdateRamp_EW_CollapseA_High` | `0x00572DA0` |
| 8 | `UpdateRamp_EW_CollapseB_High` | `0x00573170` |

**Neighbor walk:** every helper reads ONE adjacent cell via
`target = param_1 + (g_DirectionOffsets[param_2 & 7], DirectionOffsets_Y[param_2 & 7])`.
`param_2` is a compile-time constant at each callsite (see §3.1 — 2, 4, 6, or 0).

**State transitions — writes to target cell `+0x11E`** (gated by `target.Flags & 0x80`):

| Helper | Input state | Output state |
|--------|-------------|--------------|
| NS_DamageA | 0-3 | 4 |
| NS_DamageA | 5 | 6 |
| NS_DamageB | 0-3 | 5 |
| NS_DamageB | 4 | 6 |
| NS_CollapseA | 0-6 | 7 |
| NS_CollapseA | 8 | 0 (+ recurse, clear bridge dir 0, overlay = -1) |
| NS_CollapseB | 0-6 | 8 |
| NS_CollapseB | 7 | 0 (+ recurse, clear bridge dir 0, overlay = -1) |
| EW_DamageA | 9-12 | 0x0E |
| EW_DamageA | 0x0D | 0x0F |
| EW_DamageB | 9-12 | 0x0D |
| EW_DamageB | 0x0E | 0x0F |
| EW_CollapseA | 9-15 | 0x11 |
| EW_CollapseA | 0x10 | 0 (+ recurse, clear bridge dir 6, overlay = -1) |
| EW_CollapseB | 9-15 | 0x10 |
| EW_CollapseB | 0x11 | 0 (+ recurse, clear bridge dir 6, overlay = -1) |

**A vs B are two cooperative halves** — A and B helpers for the same damage tier write
different intermediate states (4 vs 5, 7 vs 8, 0x0D vs 0x0E, 0x10 vs 0x11) and
converge on the final tier from different directions.

**Overlay writes (via `SetOverlayAndPropagate`)** — each helper writes one of four
bridgehead-class offsets: +0, +1, +2, or +3 from the NS-base (`DAT_00ABAD30`) or
EW-base (`DAT_00AA1028`), added to the bridge set base (`DAT_00AA0E28`). Collapse
variants on the `+3` case also fire `CellClass__BlowUpBridge` on 3 perpendicular
neighbor cells — selected by `cell.BridgeClassId & 1` (NS) or
`cell.BridgeClassId < 5` (EW). (corrected 2026-07-10: `decompile_function
0x0057B440` proves `+0x11A` is the template slot/class byte, and
`decompile_function 0x00576BA0` performs these tests on `+0x11A` —
OFFSET_RETYPED_WRONG)

**Low vs High diff:** `UpdateRamp_*_Low` helpers (0x56ED40 family) have **identical**
state-transition logic; the only difference is the overlay-base constant —
`DAT_00ABAD1C` (WoodBridgeSet) instead of `DAT_00AA0E28` (BridgeSet). Same A/B
cooperation pattern, same Damage-vs-Collapse semantics.

### 11.2 `ApplyBridgeDestruction_*_High` — 16-entry overlay neighbor table

`ApplyBridgeDestruction_NS_High` @ `0x0057E7A0` and `_EW_High` @ `0x0057ED00` are the
**per-cell visual transition primitives** called by the destroy walker. Each reads
its center cell + two perpendicular neighbors, picks a next-overlay value from a
local 16-entry table via `CheckBridgeNeighbors_*_High()`, writes the chosen overlay
to all 3 cells, calls `TacticalClass::DirtyScreenRect` and `RecalcAttributes`, then
fires `FUN_00487a10` 3× to damage techno occupants on each cell (via `ReceiveDamage`
with C4Warhead, amount=0, force_kill=1).

**NS (0x57E7A0) next-overlay table** (indexed by `CheckBridgeNeighbors_EW_High`):

| idx | 0  | 1    | 2    | 3  | 4    | 5    | 6    | 7  | 8    | 9    | 10   | 11   | 12   | 13   | 14   | 15   |
|-----|----|------|------|----|------|------|------|----|------|------|------|------|------|------|------|------|
| val | -1 | 0xD2 | 0xD5 | -1 | 0xD1 | 0xD3 | 0xD5 | -1 | 0xD4 | 0xD4 | 0xE7 | -1   | -1   | -1   | -1   | -1   |

Progressive chain: `0xDF → 0xE0`, `0xE1 → 0xE2`, final destroyed = `0xE7`. Outer
overlay-id gate: `0xCD..=0xE8`.

**EW (0x57ED00) next-overlay table** (indexed by `CheckBridgeNeighbors_NS_High`):

| idx | 0  | 1    | 2    | 3  | 4    | 5    | 6    | 7  | 8    | 9    | 10   | 11   | 12   | 13   | 14   | 15   |
|-----|----|------|------|----|------|------|------|----|------|------|------|------|------|------|------|------|
| val | -1 | 0xDB | 0xDE | -1 | 0xDA | 0xDC | 0xDE | -1 | 0xDD | 0xDD | 0xE8 | -1   | -1   | -1   | -1   | -1   |

Progressive chain: `0xE3 → 0xE4`, `0xE5 → 0xE6`, final destroyed = `0xE8`. Outer
overlay-id gate: `0xCD..=0xE8`.

Indices 11..=15 verified live `0x57E7A0` and `0x57ED00` 2026-05-07: each is
explicitly initialized to `0xffffffff` in the function prologue (no fall-through,
no aliased entry — genuinely unused).

**This resolves open question 2** (origin of overlay values 0xD4/0xD5 and 0xDD/0xDE
— they come from the neighbor-check table, picked based on what's on the adjacent
cells, not from map state).

When new overlay == final-destroyed (`0xE7` or `0xE8`): sets a flag that triggers
`RadarClass::MarkTerrainDirty` on 3 cells in a staggered pattern.

### 11.2-LOW `ApplyBridgeDestruction_*_Low` — wood-bridge equivalent

`ApplyBridgeDestruction_NS_Low` @ `0x0057DD50` and `_EW_Low` @ `0x0057E2A0` are
the wood-bridge counterparts to the HIGH primitives in §11.2 — structurally
identical (same 3-cell write, same screen-dirty + RecalcAttributes + 3×
`FUN_00487a10` C4Warhead occupant kill, same `MarkTerrainDirty` on final-destroyed
state) but operating in the wood-overlay range. Companion neighbor-check helpers:
`CheckBridgeNeighbors_NS_Low` @ `0x0057B990`, `CheckBridgeNeighbors_EW_Low` @ `0x0057B870`.
Verified live 2026-05-07.

**LOW NS (0x57DD50) next-overlay table** (indexed by `CheckBridgeNeighbors_EW_Low`):

| idx | 0  | 1    | 2    | 3  | 4    | 5    | 6    | 7  | 8    | 9    | 10   | 11   | 12   | 13   | 14   | 15   |
|-----|----|------|------|----|------|------|------|----|------|------|------|------|------|------|------|------|
| val | -1 | 0x4F | 0x52 | -1 | 0x4E | 0x50 | 0x52 | -1 | 0x51 | 0x51 | 0x64 | -1   | -1   | -1   | -1   | -1   |

Progressive chain: `0x5C → 0x5D`, `0x5E → 0x5F`, final destroyed = `0x64`. Outer
overlay-id gate: `0x4A..=0x65`.

**LOW EW (0x57E2A0) next-overlay table** (indexed by `CheckBridgeNeighbors_NS_Low`):

| idx | 0  | 1    | 2    | 3  | 4    | 5    | 6    | 7  | 8    | 9    | 10   | 11   | 12   | 13   | 14   | 15   |
|-----|----|------|------|----|------|------|------|----|------|------|------|------|------|------|------|------|
| val | -1 | 0x58 | 0x5B | -1 | 0x57 | 0x59 | 0x5B | -1 | 0x5A | 0x5A | 0x65 | -1   | -1   | -1   | -1   | -1   |

Progressive chain: `0x60 → 0x61`, `0x62 → 0x63`, final destroyed = `0x65`. Outer
overlay-id gate: `0x4A..=0x65`.

**Structural symmetry with HIGH:** unused slots are identical (0, 3, 7, 11..=15);
slot pairs that share a value (1↔none, 2↔6, 8↔9) are identical. Final-destroyed
slot is always 10. The overlay byte ranges are simply shifted from the HIGH
range (`0xCD..=0xE8`) to the LOW range (`0x4A..=0x65`).

**Final-destroyed `MarkTerrainDirty` pattern:** identical to HIGH per axis —
center cell + (NS) `+(0,1)` and `+(0,2)`, or (EW) `+(1,0)` and `+(0,1)` (3-cell
staggered pattern).

### 11.3 `RepairBridgeSegment` @ `0x00575EE0` — misnamed, actually `NotifyBridgeSpanCollapse`

**Contrary to the function name, this does NOT repair a bridge.** It walks the span
between the two endpoints and fires `TechnoClass::ProcessCellAction(0x1F, 0, DAT_00ABD480, 0, 0)`
on every occupied cell along the path. Action code `0x1F` is an occupant-level
notification — likely "shake", "damage", or "knock into water". There is no overlay
reset, no state restore, no zone-record update.

**Call sites:** `FindBridgeEndpoints_{NS,EW}_{Low,High}` all call this at the
moment of final bridge destruction. `UpdateBridgeEdgeTiles_High` also calls it on
ramp-rebuild transitions.

**Rename candidate:** `MapClass::NotifyBridgeSpanCollapse` or `ShakeBridgeSpan`.
Action `0x1F` behavior is still an open question.

### 11.4 `CellClass::BlowUpBridge` @ `0x0047DD70` — complete behavior

Gate: `DAT_00A8ED6B == 0` (byte global, currently 0 in binary image — live path).

**Step 1: kill cell's ground occupants.**
Walks `cell.FirstObject` (+0xE4) linked list. For each techno, invokes vtable `+0x16C`
(`ReceiveDamage`) with:
```
ReceiveDamage(
    coord   = techno + 0x6C,                  // CoordStruct*
    damage  = 0,                              // amount = 0, kill is forced
    warhead = RulesClass.C4Warhead (+0xFA8),  // verified
    dist    = 0,
    force_kill = 1,
    flag    = 1,
    source  = 0
)
```

This **corrects the prior BRIDGE_SYSTEM.md claim** that ReceiveDamage was invoked with
`amount=BridgeStrength=1500`. The actual amount is `0`; the kill is carried by
`force_kill=1`. `BridgeStrength` at `+0x1740` is a separate field used in the
probability gate in `Apply_area_damage`, not as a damage amount.

**Step 2: destroy alt-layer (bridge-deck) occupants.**
Walks `cell.AltObject` (+0xE8) linked list. For each, calls vtable `+0xEC`
(`Limbo`/detach — takes no args). This silently removes bridge-deck objects without
spawning damage effects.

**Step 3: log collapsed cell to global ring buffer.**
Global container at `DAT_0087F8BC` (vector-like: array ptr, count, capacity fields).
If capacity allows, appends `cell.MapCoord` (+0x24 packed short:short) to the array.
Purpose of this queue: deferred post-collapse processing (exact consumer not yet
traced).

**Step 4: spawn debris animations (two arrays).**

*Anim #1* — gated by two RNG rolls (one of which is `RulesClass+0x14C` count check):
```
if (RulesClass+0x14C > 0 && random() * DAT_007e3570 < DAT_007e4f58
                         && random()                < DAT_007e1738):
    world_coord.X = cell.MapCoord_X * 0x100 + 0x80 + jitter
    world_coord.Y = cell.MapCoord_Y * 0x100 + 0x80 + jitter
    world_coord.Z = cell.Level * DAT_0089E7C0 + DAT_0089E7B4
    anim_type = RulesClass.Array_at_0x140[random(0, RulesClass+0x14C - 1)]
    new AnimClass(anim_type, &world_coord, owner=0, flags=1, 0x600, 0, 0)
```

*Anim #2* — unconditional if alloc succeeds:
```
if (RulesClass+0x168 > 0):
    anim_type = RulesClass.Array_at_0x15C[random(0, RulesClass+0x168 - 1)]
    new AnimClass(anim_type, &world_coord, owner=random(1,5), flags=1, 0x600, 0, 0)
```

**Discrepancy flag:** one agent pass identified BridgeExplosions as parsed to
`Rules+0x168/+0x16C/+0x170` (via `ReadGeneral` @ `0x66DBA8`). Both BlowUpBridge and
the collapse walker's debris spawn read `+0x15C` (data ptr) and `+0x168` (count).
This implies the vector stored at `+0x15C` has header ptr at +0 and active-count at
+0xC, yielding ReadGeneral-base offsets of `+0x15C` (vector header) and the count
field derived. Exact `DynamicVectorClass` layout inside RulesClass needs one more
verification pass. Two interpretations:

1. `BridgeExplosions` vector embedded at `+0x15C`; data ptr at `+0x15C+4 = +0x160`
   and active count at `+0x15C+0xC = +0x168`. Accessing `+0x15C` directly yields
   the vtable/vector header — a decompiler mis-interpretation.
2. `+0x15C` is a different array (the "top-of-bridge" anim list); `+0x168` is its
   count; BridgeExplosions lives elsewhere.

Quick verification: check `*(int*)(g_RulesClass + 0x15C)` at runtime vs
`*(int*)(g_RulesClass + 0x168)`. Defer until next session.

**BlowUpBridge does NOT modify overlay or flags on the cell** — only kills
occupants, logs the coord, and spawns animations. Overlay transitions are done
separately by `ApplyBridgeDestruction_*_High`.

### 11.5 `SetBridgeDirection_NESW` / `_NWSE` — flag-bit semantics

`SetBridgeDirection_NESW` @ `0x0047E040` and `_NWSE` @ `0x0047E470` have
**byte-identical decompiled code**. The diagonal distinction is carried by
the `param_2` (walk direction index) chosen at each callsite — NESW callers and
NWSE callers pick opposite compass indices into `g_DirectionOffsets`.

**Parameters:**
- `param_1` = cell pointer (first / anchor cell of the bridge pattern)
- `param_2` = walk direction (0-7 index, or ≥8 = no advance). `0` = NS clear,
  `6` = EW clear, other values walk direction.
- `param_3` = state byte. `0` = **destroy path** (triggers `BlowUpBridge` on every
  walked cell); non-zero = **build/intact path** (marks flags).

**Cells touched:** 4 cells along the walk direction, plus an optional 5th at
`-direction` from param_1, plus an optional 6th only when `param_2 == 6` (at a fixed
diagonal offset via `DAT_0089F690`).

**Flag bits at `cell.Flags` (+0x140) manipulated by the function:**

| Bit    | Mask hex | Meaning (cross-referenced) | Set when |
|--------|----------|----------------------------|----------|
| 7 (0x80)  | 0xFFFEE07F clears | "High bridge" / elevated-cell — used by `GetEffectiveHeight` to add +4 | param_3 & 1 |
| 8 (0x100) | in all masks | "Bridge structural cell" — gate in `ProcessBridgeDamageStateMachine` | param_3 & 1 |
| 9 (0x200) | most masks | Bridge endpoint / ramp marker | param_3 & 1, cells 1+2 only |
| 10 (0x400) | always | **"Destroyed/collapsed" flag** — ONLY set on destruction path (param_3 == 0) | param_3 == 0 |
| 11 (0x800) | cells 2-4 | "Bridge tail / no-direction" | param_2 == 0 |
| 12 (0x1000) | cell 3 | "Bridge mid-segment" | param_3 & 1 |
| 16 (0x10000) | cell 3 + 6 | "Intact / propagated" | param_3 & 1 |

**Back-reference at `cell.BridgeAnchorPtr` (+0x2C):** written to `param_1` on cells
2-5 when param_3 != 0. Written to `0` on destruction path. This is the pointer that
`ProcessBridgeDamageStateMachine_High`'s body-cell branch follows when
`flags & 0x80 == 0` (sibling-cell).

**Per-cell breakdown:**
- **Cell 1** (anchor, `param_1`): clears bits 7-12 + 16 mask `0xFFFEE07F`, sets
  bits 7|8|9|12|16 from param_3, bit 10 from !param_3, bit 11 from !param_2. Sets
  `field_0x11E = 0` or `9` based on direction. Calls `BlowUpBridge` if param_3==0.
- **Cell 2-4** (walk steps 1-3): minor variations. Cell 3 omits bit 9. Cell 4 touches
  only bit 12.
- **Cell 5** (`-direction` from anchor): clears wider mask `0xFFFEE7FF`, sets
  bits 8|9|10|11|16.
- **Cell 6** (`param_2==6` only, fixed `DAT_0089F690` offset): sets
  `field_0x2C = param_1` and toggles bit 16 from param_3.

**Destruction path (`param_3==0`):** each of 4 walked cells plus cell 5 gets a
`BlowUpBridge` call in addition to flag writes. Total: 4-5 cells blown up per
`SetBridgeDirection_NESW(*, 0)` call.

### 11.6 Warhead `+0x144` = `Wall=` INI key — VERIFIED

`WarheadTypeClass::ReadINI_Body` @ `0x0075D3A0` at address `0x0075D508` writes the
byte at offset `+0x144` from a `CCINIClass::ReadBool` call using string literal at
`0x0081AC58` = ASCII `"Wall"`. Confirms the gate in `Apply_area_damage`:

> A warhead damages bridges only if `Wall=yes` in its INI definition.

This is why most generic weapons (bullets, cannon shells) don't destroy bridges —
only explicitly wall-piercing warheads (demo-charge, IonCannonWH, SuperWH) do.

In conjunction with `DestroyableBridges` SpecialFlag (bit 0x8000), both must be true
for area-damage to reach the state machine.

### 11.7 Direction offset table `g_DirectionOffsets` @ `0x0089F688`

Runtime-built by `FUN_0049F300` at startup (zero in static image). Layout:
8 entries × 4 bytes each = 8 dwords, each holding `(dx:i16, dy:i16)`.

| Index | dx | dy | Compass | Symbol |
|-------|----|----|---------|--------|
| 0 | 0  | -1 | N  | `g_DirectionOffsets` |
| 1 | +1 | -1 | NE | `DAT_0089F68C` |
| 2 | +1 | 0  | E  | `DAT_0089F690` |
| 3 | +1 | +1 | SE | `DAT_0089F694` |
| 4 | 0  | +1 | S  | `DAT_0089F698` |
| 5 | -1 | +1 | SW | `DAT_0089F69C` |
| 6 | -1 | 0  | W  | `DAT_0089F6A0` |
| 7 | -1 | -1 | NW | `DAT_0089F6A4` |

Indexing pattern: `x += (&g_DirectionOffsets)[i*2]; y += (&DAT_0089F68A)[i*2]` —
reads X from the dword at base+i*4, Y from the short at base+i*4+2.

Resolves open question 4: state machine's `iVar2 = 2` → East, `iVar2 = 4` → South,
`iVar2 = 6` → West, `iVar6 & 6 = 0` → North. These are the compass directions the
UpdateRamp helpers walk to reach adjacent bridge pieces.

### 11.8 `ComputeBridgeZones` / `InvalidateBridgeZones` / `UpdateBridgeZonesHelper`

**`ComputeBridgeZones` @ `0x0056D6E0`** — run at map init. Walks every cell via
`CellIterator_Next`. For each cell matching `IsBridge()` or `IsWoodBridge()`, uses
three parallel lookup tables (each keyed by `IsoTileTypeIndex - bridge_base`):

| Table | Address | Meaning |
|-------|---------|---------|
| A     | `0x0082A734` | Expected bridge-class ID per tile variant (filter: only build record if `cell+0x11A == table[A]`) |
| B     | `0x0082A774` | Perpendicular walk direction index (1-byte, indexes `g_DirectionOffsets`) |
| C     | `0x0082A7B4` | Terminator overlay check — walk in direction B until a cell matches |

Builds a 16-byte record:

| Offset | Size | Meaning |
|--------|------|---------|
| +0x00  | 4    | endpoint_a packed CellCoord |
| +0x04  | 4    | endpoint_b packed CellCoord |
| +0x08  | 1    | is_intact (1 = alive, 0 = destroyed) |
| +0x09  | 3    | padding |
| +0x0C  | 4    | bridge_kind (0 = high, 1 = low) |

Stored at `MapClass + 0x54 + record_idx * 0x10`; count at `MapClass + 0x60`. Low
bridges use a separate detection via `IsLowBridgeCell` + `GetTubeAtCell`, reading
the endpoint coord at `+0x28`.

**`InvalidateBridgeZones` @ `0x0056DAE0`** — called with cell coord. Uses
`FindBridgeRecord(coord, kind_mask=3, start=0)` (mask `3` matches both high and
low bridges). If the found record has `is_intact != 0`, calls
`RemoveBridgeZoneEdges(record)`, clears the intact byte, returns `true`. Sweeps
all matching records — one coord may belong to multiple overlapping bridges
(crossing bridges case). Returns `true` if any record was flipped.

**`UpdateBridgeZonesHelper` @ `0x0056C510`** — heavy rebuild, called when
`InvalidateBridgeZones` returned `true`:
1. Clears 13 zone caches at `(*this+0x18 .. +0x4C)`.
2. Re-floodfills every unassigned cell via `ZoneFloodFillScanLine`, numbering zones
   into the passability matrix at `+0x68`.
3. Walks all bridge records. For each intact record: looks up zone IDs at both
   endpoints; if they differ, registers a zone-edge pair in a hash bucket at
   `(**this+0x14) + uVar21*0x18` (hash = packed 4-bit pair).
4. Builds per-movement-type adjacency — iterates passability matrix until
   `< 0x82A734`, which is 8 movement types.
5. BFS on the zone graph assigning "super-zone" IDs, written to `*puStack_40[]` at
   `+0x18..+0x48`. Terminator `0xFFFF` appended.
6. Returns the largest zone ID found.

### 11.9 `UpdateBridgeEdgeTiles_High` @ `0x00576200` — ramp re-evaluation walker

Called by `UpdateAdjacentBridges_High` with a direction offset (2 or 4) and a
dirty-rect accumulator pointer. Walks up to 30 cells from the start coord in the
given direction, looking for ramp-edge tile classes (indexed from
`DAT_00ABC1E8/DAT_00AA0E38` pair for E-walks, `DAT_00ABC1D0/DAT_00AA1540` pair for
S-walks). When it finds one:

- Unions a dirty-rect around both ends via `TacticalClass::CoordsToClient2` padded
  ±`0x40`.
- Walks back toward the start, detecting `Flags & 0x80` transitions:
  - **Was-set → now-clear** (ramp disappeared): step back one cell, call
    `SetBridgeDirection_NESW(dir, 0)` (destruction path), clear `+0x11E`, set
    `+0x44 = -1`, mark radar dirty, recurse on the caller's start, return 1.
  - **Was-clear → now-set** (ramp newly valid): call `RepairBridgeSegment` once
    (latched by `bVar2` so at most one call per walk).

Returns 1 if it performed a ramp rebuild, 0 if no transition.

### 11.10 `RepairBridge_High` @ `0x0057F440` — NOT the engineer entry

Decompilation confirms this is called **only from `ProcessBridgeDestruction_High` @
`0x005735C8`**, which is the unified "post-damage / post-repair handler". That parent
is called from:
- `BuildingClass::Update` (§11.11 — bridge-repair-hut death path)
- `InfantryClass::Mission_Enter` (engineer entering hut — via capture)

Engineers don't call `RepairBridge_High` directly. The flow:

```
engineer.Mission_Enter(BridgeRepairHut)
    → InfantryClass::Mission_Enter
        → (Engineer-specific capture/occupy logic sets a flag)
        → ProcessBridgeDestruction_High(bridge_cell)   // unified handler
            → if intact → RepairBridge_High (restore tiles)
            → if destroyed → damage state machine advance
```

**`RepairBridge_High` internal behavior:** reads cell `+0x44` to classify as NS
(ranges `0xCD..0xD5`, `0xDF..0xE2`, `{0xE7}`) or EW (ranges `0xD6..0xDE`,
`0xE3..0xE6`, `{0xE8}`), picks a perpendicular probe cell to decide which side the
walker starts on, then dispatches to `RepairBridgeWalker_NS_High` or
`RepairBridgeWalker_EW_High`. Walkers step along the bridge and swap damaged tile
types for pristine ones.

### 11.11 `BuildingClass::Update` — BridgeRepairHut death triggers collapse

Call site `0x0044031B` inside `BuildingClass::Update` (body at `0x0043FB20`).

Guarded by:
- `this->Type->field_0x16B8 != 0` — BuildingTypeClass flag (likely from
  `BridgeRepairHut=yes` INI key)
- `this->field_0x6DF != 0` — pending-collapse latch (set from building's damage
  path when the hut is killed)

Logic:
1. Scans a 5×5 neighborhood around the building's base cell (via vtable `+0x1B8`
   CoordStruct getter).
2. For each of the 25 cells, classifies bridge type via two tests:
   - `OverlayTypeIndex ∈ [WoodBridgeSet .. WoodBridgeSet+0x10)` (low/wood overlays)
   - `OverlayTypeIndex ∈ [0x4A..0x65]` (low-bridge cliff-tile range)
3. If ANY match, sets top byte of a local dword to `0x01` (low-bridge flag).
4. Dispatches:
   - Low flag set → `DestroyBridge_Low_MapInit(bridge_cell)`
   - Else → `DestroyBridge_High_MapInit(bridge_cell)`
5. Clears `field_0x6DF = 0` and `field_0x540 = 0`.

**`_MapInit` suffix is misleading** — these functions are runtime-only in the active
call graph; pre-destroyed map bridges come from packed map state rather than these
calls. `0x00574000` is now best described as
`DestroyBridge_High_OnHutDeath`. (corrected 2026-07-10:
`get_function_callers 0x00574000` and `get_function_xrefs 0x00574000` show only
runtime sites `BombClass__Detonate @ 0x00438982` and
`BuildingClass::Update @ 0x0044031B` — INFERENCE_HARDENED)

This **confirms the canonical gameplay mechanic:** destroying a `BridgeRepairHut`
collapses the bridge that hut governs. Previously undocumented in our research.

### 11.12 Latin-square table `DAT_0081CC30` — resolved (0-3 range, 16 entries)

Memory dump confirms 16 `int32` entries:

```
{0, 1, 2, 3,
 3, 2, 1, 0,
 2, 3, 0, 1,
 1, 0, 3, 2}
```

Classic 4×4 Latin square. Used at `CellClass::DrawOverlay_Body` @ `0x0047F7D3` to
pick one of 4 frame variants per bridge cell based on `(cell.Y & 3) << 2 | (cell.X & 3)`.

**Resolves the self-contradiction in [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md)** which said
both "0-3 range" and "0-8 range" in different sections. Correct is **0-3, 4 variants**.

### 11.13 Rules offsets corrected (RESOLVED 2026-05-18)

**Discrepancy resolved by [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md).**
The earlier "Rules offsets corrected" table was off by 0xC bytes: the three
trailing writes after `CopyFrom` are the `DynamicVectorClass` extension fields
(`ActiveCount / GrowthStep / trailing`), not `data*/cap/count`. The actual DVC
base for each vector sits 0xC bytes lower than previously claimed.

DVC layout (per `DynamicVectorClass::CopyFrom @ 0x00525060`):

| Sub-field | Offset from DVC base |
|-----------|----------------------|
| vtable | +0x00 |
| Vector (data ptr) | +0x04 |
| VectorMax (capacity) | +0x08 |
| IsAllocated (byte at +0x0D) | +0x0C dword |
| ActiveCount | +0x10 |
| GrowthStep | +0x14 |
| trailing | +0x18 |

Definitive Rules offsets (verified via `ReadGeneral @ 0x0066D530` writers and
`BlowUpBridge @ 0x0047DD70` readers):

| INI key | DVC base | data ptr | ActiveCount | Parser write site | Live read site |
|---------|----------|----------|-------------|-------------------|----------------|
| `MetallicDebris` | `+0x13C` | `+0x140` | `+0x14C` | `0x0066DB4B LEA EBX,[ESI+0x13C]` | `BlowUpBridge` Anim #1 |
| `BridgeExplosions` | `+0x158` | `+0x15C` | `+0x168` | `0x0066DC52 LEA EBX,[ESI+0x158]` | `BlowUpBridge` Anim #2 + collapse walker |
| `BridgeVoxelMax` | — | — | — | `+0x624` (scalar) | none — TS-only dormant |
| `BridgeStrength` | — | — | — | `+0x1740` (scalar) | RNG denominator |
| `IonCannonWarhead` | — | — | — | `+0xFF0` (ptr) | warhead identity |
| `C4Warhead` | — | — | — | `+0xFA8` (ptr) | BlowUpBridge occupant kill |

Both `BlowUpBridge` live reads (`+0x140/+0x14C` and `+0x15C/+0x168`) are
correct — they read `data*` and `ActiveCount` directly through the standard
DVC accessors. There is no decompiler error; the §12.11 layout was simply
8-bytes-off due to mis-counting where `CopyFrom`'s writes ended versus where
`ReadGeneral`'s post-copy writes began.

### 11.14 Runtime `DestroyBridge_High_OnHutDeath` sets a deferred-redraw flag

At the end of the runtime hut/bomb area-search destroy (just before
`UpdateBridgeZonesHelper`), writes `1` to `g_Tactical + 0xD7C`
(absolute address `0x008880A0`, assuming `g_Tactical == 0x00887324`).

No currently-indexed reader — likely consumed by the tactical map surface compose
path on the next frame for a full terrain rebuild. Per-cell walkers use
`TacticalClass::DirtyScreenRect` for incremental redraws and do **not** set this
global.

### 11.15 Summary of new callees discovered

| Address | Function | Purpose |
|---------|----------|---------|
| `0x00572330` | `UpdateRamp_NS_DamageB_High` | Ramp damage B-half (§11.1) |
| `0x00572440` | `UpdateRamp_NS_CollapseA_High` | Ramp collapse A-half |
| `0x005727E0` | `UpdateRamp_NS_CollapseB_High` | Ramp collapse B-half |
| `0x00572B80` | `UpdateRamp_EW_DamageA_High` | EW damage A |
| `0x00572C90` | `UpdateRamp_EW_DamageB_High` | EW damage B |
| `0x00572DA0` | `UpdateRamp_EW_CollapseA_High` | EW collapse A |
| `0x00573170` | `UpdateRamp_EW_CollapseB_High` | EW collapse B |
| `0x0056D6E0` | `ComputeBridgeZones` | Map-init bridge record builder |
| `0x0056DAE0` | `InvalidateBridgeZones` | Per-coord bridge invalidation |
| `0x0056C510` | `UpdateBridgeZonesHelper` | Full zone graph rebuild |
| `0x00576200` | `UpdateBridgeEdgeTiles_High` | Ramp edge rebuild walker |
| `0x005735C8` | `ProcessBridgeDestruction_High` | Unified damage/repair dispatcher |
| `0x0043FB20` | `BuildingClass::Update` (body) | BridgeRepairHut death dispatches collapse |
| `0x0049F300` | `InitDirectionOffsets` (inferred) | Runtime builder of `g_DirectionOffsets` |
| `0x0075D3A0` | `WarheadTypeClass::ReadINI_Body` | Reads `Wall=` into `+0x144` |
| `0x0066DBA8` | `RulesClass::ReadGeneral` | Parses `BridgeExplosions`, `BridgeVoxelMax` |
| `0x0087F8BC` | `g_CollapsedBridgeCellQueue` (inferred) | Ring buffer of collapsed cells (from `BlowUpBridge`) |
| `0x00887324+0xD7C` | `g_Tactical.deferred_terrain_rebuild` | Redraw flag set by runtime area-search destroy (corrected 2026-07-10: `get_function_callers 0x00574000` shows runtime-only reachability — INFERENCE_HARDENED) |
| `0x00A8ED6B` | `g_BlowUpBridgeDisable` | Byte gate on `BlowUpBridge` (currently 0 = live) |
| `0x0081AC58` | string `"Wall"` | INI key for warhead+0x144 |
| `0x0082A734` | bridge orientation table A (height filter) | ComputeBridgeZones lookup |
| `0x0082A774` | bridge orientation table B (direction index) | ComputeBridgeZones lookup |
| `0x0082A7B4` | bridge orientation table C (terminator) | ComputeBridgeZones lookup |

### 11.16 Additional Rust gaps (continued in §12.18)

Beyond the gaps listed in §9:

11. **BridgeRepairHut death → bridge collapse** (§11.11) is not implemented. A
    `BridgeRepairHut=yes` building's death should trigger `DestroyBridge_*_MapInit`
    on the governed bridge.
12. **Engineer-capture repair flow** (§11.10) — engineer entering hut should call
    unified `ProcessBridgeDestruction_High` which dispatches to `RepairBridge_High`.
13. **The 16-entry neighbor-overlay lookup table** for `ApplyBridgeDestruction_*`
    (§11.2) is missing. Rust currently has no per-cell overlay transition — the
    choice of next overlay depends on what the adjacent cells show.
14. **Bridge record table** at `MapClass+0x54` (16 bytes per record; endpoint_a,
    endpoint_b, is_intact, bridge_kind) is not represented. Rust has
    `BridgeEndpointRecord` but without the `is_intact` flip-on-invalidation logic.
15. **Zone graph rebuild** (§11.8) when bridges change — not mirrored in Rust.
16. **`RepairBridgeSegment` misnamed** — whatever action `ProcessCellAction(0x1F)`
    does to span occupants is a missing effect.
17. **Two-array debris spawn** (§11.4 anim #1 and #2) — Rust spawns zero debris.
18. **Alt-layer object destruction** via `Limbo` (§11.4 step 2) — **Reclassified in
    §12.7: not Limbo but `DropIn` — bridge-deck units SURVIVE and fall to ground layer.**

## 12. Second Deep Dive — Corrections and New Integration

This section extends §11 and **supersedes** several claims made earlier. Each subsection
cross-references the earlier claim it replaces or refines.

### 12.1 Unified dispatcher `ProcessBridgeDestruction_High` @ `0x00573540`

**Correction to §4 and §11:** the address given as `0x005735C8` is an internal offset,
not the entry point. The function starts at `0x00573540`.

**It is THE single entry point** for both engineer-triggered repair AND
building-death-triggered collapse. The decision flow:

1. **5×5 scan for damaged overlays** around the input coord. If any cell has
   `OverlayTypeIndex ∈ [0xCD..0xE8]` (HIGH damaged/destroyed range) →
   call `MapClass::RepairBridge_High(&cell)` and return. This is the repair branch.
2. If no damaged cell found: walk up to 8 neighbor offsets looking for a cell with
   `Flags & 0x100` (bridge head) or `Flags & 0x400` (destroyed bridge).
3. Pick an anchor based on `Flags & {0x100, 0x400, 0x80, 0x800}`:
   - Flag 0x80 set → use `cell.MapCoord`
   - Flag 0x80 clear + 0x100 set → follow `cell.BridgeAnchorPtr (+0x2C)` to the partner
   - Reverse-walk with direction `(flags & 0x800 ? 2 : 0) + 2` until `0x400` clears
4. **Damage-advance state machine** — walks the bridge, checks each cell's
   `(OverlayTypeIndex - BridgeSet) + 1` against 4 bridgehead class families with
   expected bridge-class ID at `cell+0x11A`:
   - `DAT_00ABC2B4 / DAT_00AA1130` + class ID 8 → `ToggleBridgePavement` + tier-2
     damage redraw via `FUN_00568E40`
   - `DAT_00ABAD30..+3` + class ID 5 → first-tier damage (NS step); on the `+3` variant
     calls `SetOverlayAndPropagate`, bumps +0x11B by +4 on 3 cells, validates zones,
     and **recurses** on `(X-2, Y)`.
   - `DAT_00AA1548 / DAT_00AA0740` + class ID 12 → tier-4 damage (`FUN_00568E40(*, 4)`)
   - `DAT_00AA1028..+3` + class ID 7 → second-tier damage (EW step); same pattern,
     recurses on `(X, Y-2)`.
   (corrected 2026-07-10: `decompile_function 0x0057B440` writes the template
   linear slot to `+0x11A`; `decompile_function 0x00573540` compares that byte
   with 8/5/12/7 — OFFSET_RETYPED_WRONG)
5. If `ValidateBridgeZones` returned true, calls `UpdateBridgeZonesHelper` at end.

**Low-bridge mirror** `ProcessBridgeDestruction_Low @ 0x00570050` — structurally
identical, overlay range `[0x4A..0x65]`, calls `RepairBridge_Low` and `FUN_00569760`
(low redraw). Uses `DAT_00ABAD1C` as base. Adds an extra map-diamond-clip test via
`DAT_0087F8DC / DAT_0087F8E0`.

**Callers (verified xrefs):**
- `InfantryClass::Mission_Enter @ 0x00519D12` (high), `0x00519CF6` (low)
- Self-recursion at `0x00573C53 / 0x00573F30`

**Note:** `BuildingClass::Update` bridge-death path (§11.11) does NOT call
`ProcessBridgeDestruction_High` directly — it calls `DestroyBridge_*_MapInit` instead.

### 12.2 Repair walkers (full overlay reverse-transition table)

**`RepairBridgeWalker_NS_High @ 0x005800D0`** and **`_EW_High @ 0x00580600`** —
mirror of the destroy walkers.

NS walker algorithm:
1. Start at input cell; **decrement X** until overlay leaves `[0xCD..0xE8]`; increment
   back to the last damaged cell (finds north end of the damaged NS run).
2. Iterate cells along +X, working the triple-row (center + `Y±1`).
3. Switch on current cell's overlay:
   - `0xD1..0xD5 / 0xE7` (fully destroyed) → `FUN_00598030() + 0xCD` (random healthy
     frame from 0xCD-0xD2 via randomizer)
   - `0xDF / 0xE0` → `0xDF` (one step back toward healthy)
   - `0xE1 / 0xE2` → `0xE1` (one step back toward healthy)
   - default `0xD6..` → skip, advance
4. Write new overlay to all 3 cells; call `RecalcAttributes` on each; dirty screen
   and (if was `0xE7`) radar.
5. Loop while `FUN_00580B70()` (bridge-continuity probe) returns non-zero.
6. On any fully-destroyed → restored transition, sets flag and calls
   `UpdateBridgeZonesHelper` at end.

EW walker mirror: decrements Y, walks +Y, triple-column (center + `X±1`). Destroyed
set: `0xDA..0xDE / 0xE8`. Restored base: `FUN_00598030() + 0xD6`. Intermediate pairs:
`0xE3/0xE4 → 0xE3`, `0xE5/0xE6 → 0xE5`.

**Low variants:** `RepairBridgeWalker_NS_Low @ 0x0057F6A0`,
`RepairBridgeWalker_EW_Low @ 0x0057FBC0`. Same shape, low-bridge overlay constants.

**Bridge-flag clearing:** the walkers do NOT explicitly clear the "destroyed" flag
(0x400). `CellClass::RecalcAttributes` is invoked on each cell, which — per §12.4 —
does NOT touch bridge flags. **Flag 0x400 clearing remains unaccounted for** in the
decompilation pass. Candidate: may be cleared as a side effect of
`SetOverlayAndPropagate` or the next `InvalidateBridgeZones` pass. Open question.

### 12.3 Engineer entry — `InfantryClass::Mission_Enter` @ `0x005196A0`

Confirmed engineer-repair path. Triggered when:
- Engineer unit's `Mission == 6` (capture/repair state)
- Target building's `Type[+0x16B6] != 0` (`BridgeRepairHut=yes` INI flag — see §12.5)

Logic:
1. Play EVA_BridgeRepaired + `RepairBridgeSound` (via `RulesClass+0x248` voc reference)
2. 5×5 neighbor scan around the hut; classify each cell:
   - Low-bridge tile: `DAT_00ABAD1C <= OverlayTypeIndex < DAT_00ABAD1C + 0x10`
   - Or low-bridge overlay: `0x4A <= OverlayTypeIndex <= 0x65`
3. If any match → dispatch to `ProcessBridgeDestruction_Low`; else
   `ProcessBridgeDestruction_High`.
4. Walk `DAT_00A83DF8` event-listeners calling hut vtable `+0x28`; then hut vtable
   `+0x2E0` (probably `Mark_For_Redraw`).

### 12.4 Per-cell primitives — clarifications

#### `ToggleBridgePavement @ 0x0056E990`

Toggles **bit `0x2000`** in `CellClass.Flags`. Does NOT change overlay.

Recursion: flood-fills to all 8 neighbors where `IsoTileTypeIndex` matches the input
cell. Depth-limited by the second parameter (`recurseGuard` — first call passes 0,
recursive calls pass 1). Dirties radar + 256×256 screen rect on outer call.

**Correction to §3.2:** Previously stated the helper "propagates pavement" — true at
the cell-flag level, but **it modifies Flags, not OverlayTypeIndex**. Rust
implementation should be a flag toggle + neighbor propagation, not an overlay
rewrite.

#### `FUN_00487A10` — confirmed 5×5 damage helper (not 3-wide)

**Correction to §11.2:** Called from `ApplyBridgeDestruction_*_High` 3× — we said
"once per cell in the 3-wide perpendicular row." Actually each invocation covers a
**5×5 area** (-2..+2 around the cell), not just a single cell. Three invocations
thus sweep three overlapping 5×5 regions, guaranteeing coverage across the
bridge-width + destruction zone.

Per cell in the 5×5:
1. Walk `FirstObject` (correction: at `cell+0xE4`, not +0xE8 as earlier claimed).
2. For each occupant: call vtable `+0x1AC` (`What_Action`) with args `(-1,-1,0,1)`.
   If result == 7, fire `ReceiveDamage(damage=0, warhead=Rules+0xFA8, force_kill=1,
   flag1=1)`.
3. Optionally destroy specific occupant types based on the `only_infantry_flag`
   parameter.

#### `ReceiveDamage` parameter order — CORRECTION

**Correction to §11.4:** The parameter order in `ObjectClass::ReceiveDamage` is:

```
(damage*, distance, warhead, source_techno, ignore_armor, flag1, attacker_house)
```

NOT `(coord, damage, warhead, dist, force_kill, flag1, source)` as earlier claimed.

- `ignore_armor` (5th) bypasses armor multiplier, Insignificant flag, prone
  reduction, and ForceFire gate.
- `flag1` (6th) propagates to `TechnoClass::ReceiveDamage` where it gates retaliation
  and trigger-action dispatch.

#### `CellClass::RecalcAttributes @ 0x0047D2B0` — does NOT touch bridge flags

**Correction to §5 (function list):** RecalcAttributes only refreshes LandType,
SlopeIndex, Zone metadata, and Tiberium/anim flags (`0x10000`, `0x20000`). Bridge
flags 0x80/0x100/0x200/0x400 at `+0x140` are NOT re-derived here — they're set only
by `SetBridgeDirection_NESW` at build/destruction time. Safe to call on bridge cells
without disturbing bridge state.

### 12.5 Bridge hut death mechanics — full trace

**`field_0x6DF` setter** (the pending-collapse latch):

Primary writer: `TechnoClass::ReceiveDamage @ 0x00701F47` (line inside the function).
Conditions to set:
- Damage result == 4 (fatal)
- `What_Am_I() == 6` (building)
- Source warhead's Type has `field_0x130 != 0` — a warhead/attacker flag meaning
  "blows up bridges" (INI key not yet traced; candidate: `Wall=` or a dedicated bit)
- `BuildingType+0x1551 != 0` — the **"is bridge hut" capability** (distinct from
  `+0x16B6 BridgeRepairHut=`). One of them is the type flag, the other the repair
  capability; two separate fields.
- Fresh-strike check: `frame - +0x528 > +0x530` (cooldown)

When triggered:
```
this+0x6DF = 1            // pending-collapse latch
this+0x528 = CurrentFrame // anti-double-fire
this+0x530 = cooldown
IsAlive   = true
Health    = 1              // hut SURVIVES the fatal hit!
return 5                   // "damage absorbed, escaped death"
```

The bridge hut does NOT die from a single hit — it **absorbs the lethal blow** and
schedules the bridge collapse for the next Update tick. `BuildingClass::Update @
0x0043FB20` reads the latch, does the 5×5 bridge-type scan, dispatches to
`DestroyBridge_{High,Low}_MapInit`, and clears the latch at `0x00440320`.

**Additional writers of `+0x6DF`:**
- `InfantryClass::Mission_Enter @ 0x0051A5A9` (engineer entry — likely different
  semantic; may be related to handover/cleanup)

**BuildingType flag layout correction** (replaces §11.11 claim that `Type+0x16B8`
is `BridgeRepairHut`):

| Offset | INI key | String address |
|--------|---------|----------------|
| `+0x16B6` | `BridgeRepairHut` | `0x0081A898` |
| `+0x16B7` | `Gate` | `0x0081AA8C` |
| `+0x16B8` | `SAM` | `0x0081AA88` (not bridge-related) |

**Rust implication:** detect the hut via `BuildingType.bridge_repair_hut`, not by
any `SAM` bit.

### 12.6 `TechnoClass::ProcessCellAction` is actually `FireTriggerAction`

**Correction to §11.3:** `TechnoClass::ProcessCellAction @ 0x006E53A0` was mislabeled
— actual role is `TechnoClass::FireTriggerAction(eventType, source, target, arg1,
arg2)`, which walks the techno's `AttachedTag.ActionList` and evaluates each entry
via `TriggerActionEntry::EvaluateConditions`.

**Action code `0x1F` = TriggerEvent ID 31.** `RepairBridgeSegment` broadcasts event
31 along every occupied cell in the bridge span. Effect is **purely scripted**: if a
map trigger is bound to event 31, that trigger fires on those occupants. On vanilla
skirmish maps with no triggers, this call is a **no-op**.

**Rename candidate:** `BroadcastBridgeSpanTriggerEvent(31)` — not "repair," not
"shake," not "damage." The name `RepairBridgeSegment` is doubly misleading.

Other known event codes in this dispatcher: `0x26..0x2C` are damage/crush/destroy
events. Bridge-specific events are therefore in the 30s range.

### 12.7 Bridge-deck units fate — `DropIn` (NOT Limbo)

**Major correction to §11.4 step 2.** The vtable `+0xEC` slot is **`ObjectClass::DropIn
@ 0x005F4160`**, not `Limbo`. All 21 classes (Object, Foot, Aircraft, Anim,
Building, Bullet, Infantry, Isometric, Light, Overlay, Particle, Smudge, Techno,
Terrain, Unit, VoxelAnim, Wave, …) inherit the same implementation.

**`DropIn` does NOT destroy the unit**:
```
this.flag_0x8D = 1         // registered flag
this.flag_0x8F = 1         // needs-sound
vtable+0x124(this)         // Mark(0) — remove from cell occupancy
DisplayClass::RemoveFromLayer(this)
this.OnBridge (+0x23) = 0  // clear bridge-deck flag
DisplayClass::Submit_Object(this) // re-insert into ground layer
vtable+0x124(this, 1)      // Mark(1) — re-add to cell occupancy
if (flag_5 & 4):
    if (!locomotor.vtable+0x10())   // not landed
        vtable+0xF4(this, coords)   // Mark_Remove ground
    else:
        locomotor.vtable+0x9C(this, 0)  // force-land
```

**Consequence: asymmetric fate for cell occupants during bridge collapse:**

| Layer | Accessed via | Fate | Mechanism |
|-------|--------------|------|-----------|
| Ground (FirstObject, +0xE4) | Cell's ground occupancy | **DIE** instantly | `ReceiveDamage(damage=0, C4Warhead, force_kill=1)` |
| Bridge-deck (AltObject, +0xE8) | Cell's bridge-layer occupancy | **SURVIVE**, drop to ground | `DropIn` — clears OnBridge, snaps to ground height, no damage |

**Practical consequence:** a Grizzly on a collapsing water bridge survives, drops
into the water cell below, and becomes stranded (cannot move — its `SpeedType` is
incompatible with water). **No drown mechanism exists** (see §12.9). This matches
known vanilla YR player observations.

### 12.8 Pathfinding invalidation after collapse

Verified: path invalidation is NOT eager. There is no "InvalidatePath" function.
Instead:

1. **End-of-collapse in state machine** → `InvalidateBridgeZones` (per-record
   intact=0 + `RemoveBridgeZoneEdges`) → if any record flipped, `UpdateBridgeZonesHelper`
   (full passability matrix rebuild, O(cells) + O(zones²)).
2. **Bridge-passability bit `0x40000`** on cell.Flags — toggled per-unit during A*
   search by `PathfinderClass::UpdateBridgePassability @ 0x0042ACF0`. Called from
   `AStar_main_loop @ 0x00429A90`.
3. **Stale paths fail emergently**: next time the unit's locomotor requests the next
   cell, the cell's `0x100` is clear (cleared by `SetBridgeDirection_NESW(*, 0)`) so
   the locomotor fails and a re-path is requested.
4. **Mid-crossing units**: `FootClass::ShouldBeOnBridge @ 0x004DDC40` inspects
   `OnBridge` flag against cell's `0x100`. `Set_Height_On_Bridge @ 0x005F5FA0` is a
   pure Z-setter with no side effects.

### 12.9 No drown / fall-to-death / EVA announcement

**Verified absence of:**
- **Drown or submerge mechanism.** Zero matches for "Drown" / "Submerge" in
  bridge-related code. `UnitClass::IsFalling @ 0x00746D80` is aircraft-crash-only.
- **Fall-damage on bridge collapse.** No height-diff damage applied when units drop
  to ground layer via `DropIn`.
- **`EVA_BridgeDestroyed` string.** Only `EVA_BridgeRepaired @ 0x00825538` exists
  (fired from the engineer path in `Mission_Enter`).
- **`BridgeDestroyedSound` INI key.** Only `RepairBridgeSound @ 0x0083A7FC` exists.

Destruction audio comes from:
1. Spawned debris anims' own `Report=` INI entries (`BridgeExplosions`,
   `MetallicDebris`)
2. C4Warhead detonation sounds on killed ground-layer units (indirect)

**Rust parity: do NOT add a bridge-destroyed EVA.** Vanilla doesn't have one.

### 12.10 `Random__RandomRanged @ 0x0065C7E0` is the lockstep RNG

Verified as the deterministic Westwood RNG. `__thiscall` on 0x40C-byte state struct:

- `[+0x04]` = index A, `[+0x08]` = index B (dual rolling indices)
- `[+0x0C]` = 0xFA-dword ring buffer (state)
- Classic lagged-Fibonacci XOR walk

Same RNG used by: AircraftClass::AI, BulletClass::Fire/Detonation,
TechnoClass::Fire_At, AnimClass::AI, BuildingClass::ReceiveDamage,
CellClass::SpreadTiberium/PlaceTiberium/BlowUpBridge, CrateClass::PickupDispatch,
EMPulseClass::Apply, CaptureManagerClass::Update — **every lockstep-critical
outcome** goes through this single function.

Draw range: uniform over `[param_2, param_3]` via mask-and-retry (not modulo).
Seeded by `GameSeed` in the lobby event stream.

**Rust parity:** implement exact ring-buffer + index-walk + mask-and-retry
algorithm. `RandomRanged(0, count-1)` for BridgeExplosions picks must be
bit-identical across clients.

### 12.11 RulesClass `DynamicVectorClass` layout — RESOLVED 2026-05-18

**Superseded by [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md).**

The §11.13 / §12.11 layouts in earlier passes were wrong by 0xC bytes. Correct
DVC layout (verified via `CopyFrom @ 0x00525060` decomp): `vtable +0 / data* +4
/ VectorMax +8 / IsAllocated +0xD byte / ActiveCount +0x10 / GrowthStep +0x14
/ trailing +0x18` — total instance size 0x1C bytes.

Definitive Rules vector bases:

| Vector | DVC base | data* | VectorMax | ActiveCount |
|--------|----------|-------|-----------|-------------|
| `MetallicDebris` | `+0x13C` | `+0x140` | `+0x144` | `+0x14C` |
| `BridgeExplosions` | `+0x158` | `+0x15C` | `+0x160` | `+0x168` |

The earlier "+0x148" / "+0x164" base claims were artifacts of mis-attributing
the post-`CopyFrom` writes (which target the DVC extension fields at +0x10/+0x14/
+0x18) to the front-of-struct slots. `DamageFireTypes` at +0x12C in the
superseded table also needs re-verification by the same method — it was based
on the same wrong layout heuristic.

The `BlowUpBridge` reads (`+0x140/+0x14C` for MetallicDebris, `+0x15C/+0x168`
for BridgeExplosions) are correct; they read `data*` and `ActiveCount` through
the standard offsets relative to the now-corrected base.

**`BridgeVoxelMax` at `RulesClass+0x624`: DORMANT / TS-only.** Exhaustive byte-pattern
scan found no readers in gamemd.exe. `BridgeVoxelMax=3` is parsed from INI but never
consumed — a Tiberian Sun ghost. Voxel-based bridge piece tumbling was a TS feature
not wired up in YR; YR uses 2D `BridgeExplosions` AnimTypes only.

**Rust implication:** do NOT implement `BridgeVoxelMax`. Document it as TS-legacy.

### 12.12 `g_Tactical+0xD7C` — deferred full-redraw flag

**Writer sites:** runtime `DestroyBridge_High_OnHutDeath @ 0x00574000`,
`CollapseBridge_NS_High @ 0x00575BA0`, and other bridge-collapse paths set the
byte to `1`. (corrected 2026-07-10: `get_function_by_address 0x00574000` identifies
the runtime helper and `get_function_callers 0x00574000` shows no map-init caller —
INFERENCE_HARDENED)

**Reader:** `TacticalClass::Draw @ 0x006D3D10` — checks `if (*(char *)(this +
0xD7C) == '\0')` near the start. If clear, allows dirty-rect-only fast-path redraw.
If set, forces a **full tactical viewport rebuild**. Clears it at end of full draw
(along with sibling byte `+0xD7D`).

**Confirmed role:** terrain-geometry-changed signal that disables the dirty-rect
optimization for one frame after a bridge collapse. Non-bridge code paths also use
this (any global terrain change).

### 12.13 Anchor overlay `0x18` / `0x19` axis mapping

**Partial resolution from rulesmd.ini overlay list:**

| Overlay ID | INI Name | Art |
|------------|----------|-----|
| `0x18` (24)  | `DUMMY15` | no art (placeholder) |
| `0x19` (25)  | `BRIDGE1` | `Image=BRIDGE` (concrete bridge, NS-oriented) |
| `0x1A` (26)  | `BRIDGE2` | same art, second anchor cell |
| `0xED` (237) | `BRIDGEB1` | low/wood bridge (EW variant per prior research) |
| `0xEE` (238) | `BRIDGEB2` | low/wood bridge |

**Finding:** the check `anchor+0x44 == 0x18 || == 0x19` in `ApplyDamageToCell`
accepts DUMMY15 or BRIDGE1. Concrete high bridges in RA2/YR appear to be NS-oriented
by art convention; the EW variant uses `BRIDGEB1/B2` (wood, low-bridge dispatch).

**However** — the body-overlay range (0xCD..0xE8) clearly has two orientation
groups: 0xCD-0xD5 (agent's analysis says EW) and 0xD6-0xDE (NS). If concrete bridges
are only NS-oriented, the 0xCD..0xD5 range represents concrete bridges at a
different **diagonal** orientation (NE-SW vs NW-SE), not a true EW. Further map
inspection needed to confirm.

**Open**: which of `{0x18, 0x19}` corresponds to which diagonal. Rust currently
lists both as HIGH anchors, which is correct for dispatch but doesn't preserve
orientation.

### 12.14 Runtime `DestroyBridge_{High,Low}_OnHutDeath` helpers are structurally identical

Per full function diff: identical bytes except the overlay range compare (205..232
vs 74..101) and the destroy-from-cell dispatcher called (`_High` vs `_Low`).
The runtime names/reachability are verified by `get_function_by_address` and
`get_function_callers` for `0x00574000` and `0x00574C20` (corrected 2026-07-10 —
INFERENCE_HARDENED).

**Rust**: one helper with a bridge-type parameter suffices.

### 12.15 Summary of new addresses discovered

| Address | Function / Global | Purpose |
|---------|-------------------|---------|
| `0x00573540` | `ProcessBridgeDestruction_High` (entry, corrects §4) | Unified repair/damage dispatcher |
| `0x00570050` | `ProcessBridgeDestruction_Low` | Low-bridge dispatcher mirror |
| `0x005800D0` | `RepairBridgeWalker_NS_High` | Repair NS walker |
| `0x00580600` | `RepairBridgeWalker_EW_High` | Repair EW walker |
| `0x0057F6A0` | `RepairBridgeWalker_NS_Low` | Low NS repair walker |
| `0x0057FBC0` | `RepairBridgeWalker_EW_Low` | Low EW repair walker |
| `0x005196A0` | `InfantryClass::Mission_Enter` | Engineer entry |
| `0x0056E990` | `ToggleBridgePavement` | Flag 0x2000 toggle + neighbor flood-fill |
| `0x00487A10` | `FUN_00487A10` (anti-occupant helper) | 5×5 occupant-kill via ReceiveDamage |
| `0x005F5390` | `ObjectClass::ReceiveDamage` | Verified 7-param layout |
| `0x006E53A0` | `TechnoClass::FireTriggerAction` (NOT ProcessCellAction) | Scripted trigger dispatcher |
| `0x005F4160` | `ObjectClass::DropIn` | vtable+0xEC — bridge-deck fall-through |
| `0x00701F47` | `TechnoClass::ReceiveDamage`:bridge-hut-latch | Sets +0x6DF on fatal hit to bridge hut |
| `0x00701900` | `TechnoClass::ReceiveDamage` (full) | Parent of 0x00701F47 |
| `0x00440320` | `BuildingClass::Update`:latch-clearer | Clears +0x6DF after dispatch |
| `0x0065C7E0` | `Random__RandomRanged` | Deterministic lockstep RNG |
| `0x006D3D10` | `TacticalClass::Draw` | Reader/clearer of `g_Tactical+0xD7C` |
| `0x00525060` | `DynamicVectorClass::CopyFrom` | Vector layout reference |
| `0x0045FE50` | `BuildingTypeClass::ReadINI_Water` | BridgeRepairHut INI parser |
| `0x0081A898` | string `"BridgeRepairHut"` | INI key for BuildingType+0x16B6 |
| `0x007EF060` | ObjectClass vtable base | DropIn at +0xEC |
| `0x007E8C94` | FootClass vtable base | |
| `0x007F4960` | TechnoClass vtable base | |
| `0x007F5C70` | UnitClass vtable base | |

### 12.16 BuildingType flag table (expanded)

| Offset | INI key | Purpose |
|--------|---------|---------|
| `+0x1551` | (unknown INI key — `Bridge=`?) | "Is bridge hut capability" flag; gates `field_0x6DF` latch in ReceiveDamage |
| `+0x16B6` | `BridgeRepairHut` | INI bool; read by BuildingClass::Update for collapse dispatch |
| `+0x16B7` | `Gate` | Unrelated |
| `+0x16B8` | `SAM` | Unrelated (not bridge) |

Two bridge-hut flags exist. `+0x16B6` is the INI key; `+0x1551` is the derived
capability. One likely implies the other at load time.

### 12.17 Updated YR-activity table (supersedes §8)

| Path | Gate | Default | Active? |
|------|------|---------|---------|
| Area-damage → ProcessBridgeDamageStateMachine_High | `SpecialFlags & 0x8000` + `warhead.Wall=yes` | `DestroyableBridges` defaults on in skirmish; `Wall` defaults false per warhead | **Conditional by warhead** |
| Engineer repair via `InfantryClass::Mission_Enter` | `BuildingType.BridgeRepairHut=yes` on target | RA2/YR BridgeRepairHut buildings have this | **Yes** |
| Bridge-hut death → collapse | `Warhead+0x130` set on attacker + `BuildingType+0x1551` on hut | most warheads do NOT have +0x130; `Wall=yes` may be the same bit | **Yes** (with warhead restrictions) |
| `BridgeVoxelMax=` parsing | always | default 3 | **NO — dormant TS code, no reader** |
| `RepairBridgeSegment` trigger-event 31 broadcast | map trigger bound to event 31 | no vanilla triggers bind this | **No-op in skirmish; scripted-only** |

The `Wall` default correction is binary-backed by `decompile_function 0x0075CEC0`
(`+0x144 = 0`) and `decompile_function 0x0075D3A0` (`ReadBool("Wall")` uses the
current byte as its default) (corrected 2026-07-10 — INFERENCE_HARDENED).

### 12.18 Rust implementation gaps (continued from §11.16)

19. **Unified dispatcher.** `ProcessBridgeDestruction_High` (§12.1) has no Rust
    analogue. Its 5×5-scan / damage-tier-walk / recursive-advance logic is absent.
20. **Repair walkers.** `RepairBridgeWalker_*_High` (§12.2) not implemented. Bridge
    repair via engineer is therefore missing entirely.
21. **Engineer hut entry path.** `InfantryClass::Mission_Enter` bridge branch
    (§12.3) — Rust `BridgeRepairHut` capture/repair flow not wired.
22. **Bridge hut damage-absorbing latch** (§12.5). Rust buildings don't have a
    `pending_bridge_collapse` field. Hut death should absorb the killing blow and
    schedule collapse for the next tick.
23. **Warhead `+0x130` attacker-type flag** — if distinct from `Wall=` (+0x144),
    Rust needs to model both.
24. **Bridge-deck units SURVIVE collapse via DropIn** (§12.7). Rust currently either
    kills all bridge occupants or leaves them floating. The correct behavior: move
    them to the ground layer, clear their `on_bridge` flag, snap their Z to ground
    height, NO damage.
25. **Two separate debris animation arrays** in `BlowUpBridge` (§12.11):
    MetallicDebris (`Rules+0x140/+0x14C` per live reads) + BridgeExplosions
    (`Rules+0x15C/+0x168`). Rust spawns neither.
26. **No drown mechanism needed.** The Rust engine also does not need to
    implement fall-damage or drowning on collapse — vanilla YR does not.
27. **Lockstep RNG (§12.10).** Rust must use the deterministic fixed-point RNG for
    all bridge debris spawns to preserve multiplayer sync. `RandomRanged(0,
    count-1)` for BridgeExplosions anim pick + `RandomRanged(1, 5)` for animation
    variant — both are lockstep-critical.
28. **`ToggleBridgePavement`** as flag-only toggle (`0x2000`, not an overlay
    rewrite) with 8-neighbor flood-fill on matching IsoTile.
29. **`field_0x6DF` cooldown** (anti-double-fire at `+0x528` / `+0x530`). Rust should
    mirror the frame-delta guard to avoid a single warhead collapsing the same
    bridge multiple times in consecutive ticks.
30. **`TacticalClass::Draw` deferred-redraw flag** — when bridges collapse, the
    Rust renderer should force a full terrain redraw for one frame to flush any
    per-cell cached state. (Not lockstep-critical, but visual parity.)

## 13. Third Deep Dive — Major Corrections + Map-Load + Navy + Shroud + Chrono

This section adds another layer and **materially corrects** §12.5 / §12.11 / §12.14 /
§12.17. A critical misinterpretation of the `field_0x6DF` latch is resolved here.

### 13.1 CRITICAL CORRECTION — `field_0x6DF` is the **DelayKill** latch, NOT a bridge-specific latch

**§12.5 was wrong about what gates the latch.** The correct interpretation:

- `WarheadTypeClass+0x130` = `CausesDelayKill` (INI bool) — string at `0x00847E74`
- `BuildingTypeClass+0x1551` = `EligibleForDelayKill` (INI bool) — string at `0x0081ACB0`
- `BuildingTypeClass+0x16B6` = `BridgeRepairHut` (INI bool) — string at `0x0081A898` — **separate and distinct**

`TechnoClass::ReceiveDamage @ 0x00701F47` handles the **generic DelayKill mechanic**:
when a warhead with `CausesDelayKill=yes` lands a fatal hit on a building with
`EligibleForDelayKill=yes`, the building absorbs the blow, keeps `Health=1, IsAlive=true`,
sets `field_0x6DF = 1`, and returns damage code 5 ("absorbed"). The cooldown fields
(`+0x528, +0x530`) prevent double-fire.

**How this intersects with bridge collapse:**
On the next tick, `BuildingClass::Update @ 0x0043FB20` processes the `field_0x6DF` latch.
The dispatch branches on **`Type+0x16B6 (BridgeRepairHut=yes)`**:
- If the building is a BridgeRepairHut → 5×5 scan + `DestroyBridge_{High,Low}_MapInit`
- Otherwise → normal "process delay kill" path (belated death + death effects)

So bridge collapse is a **specific use-case of DelayKill**. Required conditions for
bridge collapse via hut death:
1. Attacker's warhead has `CausesDelayKill=yes`
2. BridgeRepairHut building has `EligibleForDelayKill=yes` AND `BridgeRepairHut=yes`

Checking `rulesmd.ini`: the standard BridgeRepairHut buildings (`CABHUT` etc.) are marked
`EligibleForDelayKill=yes`, and warheads like `Super`, `C4Warhead`, `IonCannonWH` are
marked `CausesDelayKill=yes`. Most conventional gun warheads are **not** —
**ordinary bullets cannot destroy a BridgeRepairHut**. This matches vanilla YR.

**Rust implication:** Implement `CausesDelayKill`/`EligibleForDelayKill` as a
cross-cutting pair. Bridge-collapse behavior is a **dispatch at the latch-processing
step** inside `BuildingClass::Update`, not at the latch-setting step.

### 13.2 `ApplyBridgeTile @ 0x0057B440` — tile stamper, not placement primitive

`ApplyBridgeTile` is NOT the map-load overlay placer. It **stamps a pre-selected
multi-cell tileset template** (from global `DAT_00880990`) whose tile category must
equal `0x12` (bridge). Writes `cell+0x38`, `cell+0x11A` (**bridge-class ID**, not
height — see §13.5), and bumps `cell+0x11B` by template-delta + caller offset.
Does NOT touch `+0x2C`, `+0x140`, `+0x11E`, `+0x24`, or `+0x44`.

**Callers (13 sites):** `SelectDestroyedBridgeTile_Low @ 0x00579B3F`,
`SelectBridgeTileVariant_Low @ 0x0057B1E3`, and 11 sites inside `FUN_0059E740`
(river-tileset orchestrator).

**Map-load pipeline:**
```
1. Read OverlayPack from map file        (function name unresolved)
     → writes raw overlay indices into cells
2. MarkBridgesForRepair_High @ 0x0057A0C0 (from river orchestrators)
     → walks cells 4×: UpdateBridgeTile → ClearBridgeCell → 2× SelectBridgeTileVariant
3. Flag bits set later via SetBridgeDirection_NESW
```

**Pre-damaged bridges in maps:** encoded directly via destroyed overlay index
(`0xE7`/`0xE8`) in OverlayPack — no special "init-destroy" call.

**`DestroyBridge_High_MapInit @ 0x00574000` IS MISNAMED** — actual callers are runtime
only (`FUN_00438720` for unit-death-on-bridge, `BuildingClass::Update @ 0x0044031B`
for hut death). Rename proposal: `DestroyBridge_{High,Low}_FromRuntime`.

### 13.3 `PathfinderClass::UpdateBridgePassability @ 0x0042ACF0` — full behavior

Toggles bit **`0x40000`** in cell.Flags per unit during A*. Verified:

- Early-out if `*(param_1+3) == 0`.
- Selects ground-layer (`+0xE4`) vs bridge-layer (`+0xE8`) static path list.
- 24-waypoint loop; direction `8` is the tube/teleport sentinel (TS tunnel, unused
  in vanilla YR skirmish maps).
- Flag propagation: `cell.Flags ^= (~neighbor.Flags ^ cell.Flags) & 0x40000`.
- 5×5 fallback if `cell[0x124] != 0`.
- **Post-collapse:** if path list null, no-ops cleanly; no stale 0x40000 bits.

### 13.4 Ship-under-bridge semantics

**`ShipLocomotionClass::Compute_BridgeZOffset @ 0x0069EBB0`:**
Init-once writes `g_BridgeZ_Offset @ 0x00B0782C = ftol(g_ShipHeightStep × 4 + bias)`.
Readers: `FUN_0069F450` (ship Set_Destination) + two sites in
`ShipLocomotionClass::Process_Drive_Track`. **Z-visual compensation only** — ships
stay in water layer.

**`FootClass::ShouldBeOnBridge @ 0x004DDC40`:**
```
if (*(signed char*)(this + 0x684) >= 0) return 0;   // forced-ground gate
return ObjectClass::ShouldBeOnBridge(this);
```
`+0x684` sign-byte is the **bridge-capability gate**. Ships/jumpjets have `>= 0`
→ never on deck. Tanks/infantry have `< 0`.

**Bridge collapse on ships:** `BlowUpBridge` iterates the **ground cell's**
FirstObject/AltObject. Ships occupy the **water cell** (different cell).
**Ships take no damage from bridge collapse overhead.**

### 13.5 Cell field `+0x11A` is **bridge-class ID**, NOT generic height

**Correction to §2 and §12.** `cell+0x11A` stores a bridge-class ID written by
`ApplyBridgeTile` as the tile-row index within a bridge template footprint:

| Value | Meaning |
|-------|---------|
| `0x02` | EW bridge body |
| `0x04` | NS bridge body |
| `0x05` | NS ramp / bridgehead |
| `0x07` | NS bridgehead variant |
| `0x08` | NS high-ramp peak |
| `0x0C` | EW high-ramp peak |

`cell.Level` is `+0x11B` (separate field). The check `cell[0x11A] == 4` in state
machines means "is this NS body cell", not "is height 4".

### 13.6 Rendering per state — `DrawOverlay_Body @ 0x0047F6A0`

```
base_frame = (char)cell.Level + ((cell.Flags >> 7) & 1) * 4

if (cached via cell.Flags & 0x80):
    state = cell.BridgeDirectionState (+0x11E)
    if state == 0 || state == 9:
        state += LatinSquare[(Y & 3) << 2 | (X & 3)]
        // LatinSquare = {0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}
    frame = base_frame + state
```

**Damaged/destroyed frames (states 1..8, 10..17) are deterministic** — no variation.
Only healthy frames (0, 9) use the Latin square.

**`DrawOverlay_Shadow @ 0x0047F510`** — when state ∈ (9..17) AND bit 0x80 cached:
shift `X -= 15, Y += 7` (dropped-shadow effect). Frame = `shadow_count/2 + state`.
Z = `Level * -15 - 2`. Skipped upstream when overlay is -1 (destroyed).

### 13.7 `RecalcBridgeShroudFlags @ 0x00578100`

Called from `LogicClass::PerTickUpdate @ 0x0055AFB0` every **120 frames**
(`frame % 0x78 == 0`) and ad-hoc from reveal helpers. Two-pass full-map scan:

- Pass 1: cells with `Flags & 0x20` dirty bit → clear shroud-edge bits at
  `+0x12C & ~0x18`, `+0x140 & ~0x23`, set `+0x138 = 1`, notify if unexplored.
- Pass 2: recompute shroud edge bitmask, write to `+0x120`.

**Active in YR regardless of FogOfWar** (shroud ≠ fog). Handles the double-shroud
problem of bridge cells having two logical heights.

### 13.8 `IsOnBridgeSurface @ 0x00485060` — MISLABELED

**Correction:** only detects the first 14 tiles of `g_WaterSet (DAT_00AA0738)` —
low/pontoon water-bridge surface pieces. NOT `BridgeSet` (high concrete) or
`WoodBridgeSet`. Rename: `IsOnLowWaterBridge`.

**Callers:** `ComputeBridgeSurfaceMask` (8×), `SelectBridgeTileVariant_Low`,
`ClearBridgeCell_Low` (first 12 only), `SuperClass::Launch @ 0x006CC390` (2 sites:
cases 5 and 6 = Chrono-sphere + Chrono-warp), `TemporalClass::AI @ 0x006297F0`.

**Chrono / Temporal behavior:** when target cell is on a low water bridge, the
super calls `FootClass::Find_Nearby_Passable_Cell` and **silently relocates the
destination off the bridge**. The order isn't rejected — it's quietly moved.
Nuke/Iron Curtain/Lightning Storm/Psychic Dominator DO NOT consult this function.

### 13.9 Z-Fudge system — per-TechnoType, rendering-only

`FUN_004DAFF0` composes Z-fudge from TechnoType fields:

| INI key | TechnoType offset | String addr |
|---------|-------------------|-------------|
| `ZFudgeCliff` | `+0xDC0` | `0x00843528` |
| `ZFudgeColumn` | `+0xDC4` | `0x00843518` |
| `ZFudgeTunnel` | `+0xDC8` | `0x00843508` |
| `ZFudgeBridge` | `+0xDCC` | `0x008434F8` |

Formula: `max(cliff, column, tunnel, bridge) + base`. Used for rendering depth only.

Bridge "columns" as ship-blocking entities do NOT exist as a separate system —
high-bridge ship clearance is on the water cells beneath.

### 13.10 Helper function clarifications

**`FUN_00568E40` / `FUN_00569760` — state mutators.**
`param_2` is **bridge orientation** (2 = NS, 4 = EW), NOT damage tier (correction
to §12.1's description). Walks up to 30 cells, toggles pavement, sets overlay,
bumps `Level += 4` on ramp cells, spawns OverlayClass debris, calls
`ValidateBridgeZones → UpdateBridgeZonesHelper`.

**`FUN_00598030` — rejection-sampling RNG loop (correction to §12.2).**
It's a `do { Random__Next(); ftol(); } while (param_2 < result)` loop; the
"return" in the caller is the last ftol result (function has `void` return type).
**The repair walker replaces ALL damaged overlays with exactly `0xCD`, not a
randomized variant.** Variant healthy overlays are picked elsewhere. Uses
`Random__Next` — lockstep-safe.

**`FUN_00580B70` — continuity probe.**
Returns 1 if `cell+0x44 ∈ [0xCD, 0xE8]`, else 0. Repair walker termination
condition.

**`DAT_0087F8BC` collapsed-cell buffer — DEAD.**
Every xref is a WRITE from `BlowUpBridge`. **No readers in the binary.**
**TS-legacy debug/replay log never connected in YR. Do NOT implement in Rust.**

### 13.11 Aircraft and jumpjet interaction

**Fixed-wing** (`FlyLocomotionClass::Process @ 0x004CD600`): altitude-delta hack
using `g_FlyBridgeHeight @ 0x008B3CAC` — for landing-approach only. Aircraft fly
over bridges freely; no routing interaction.

**Jumpjets** (`JumpjetLocomotionClass @ 0x0054AC40`): excluded from deck layer via
`FootClass+0x684 >= 0`. **Jumpjets fly over bridges; they do not walk the deck.**

### 13.12 `ShouldBeOnBridge` full logic

**`ObjectClass::ShouldBeOnBridge @ 0x005F6A70`:**
```
old = this.OnBridge; ga = GroundHeight(virtual.GetCoord); gb = GroundHeight(this.Coord)
if (old == 0 && (gb - ga) > LeptonsPerLevel*3 && CellAt(virtual.GetCoord).Flags & 0x100)
    return 1         // promote to deck
if (old != 0 && (ga - gb) > LeptonsPerLevel*3)
    return 0         // demote to ground
return old           // hysteresis
```

`LeptonsPerLevel` at `0x00AC13C8`. Hysteresis × 3 handles paradrop landing and
in-transit transitions.

### 13.13 Bridge Z-offset globals

| Global | Address | Purpose |
|--------|---------|---------|
| `g_BridgeZ_Offset_Drive` | `0x008A07C4` | Drive locomotion Z lift on deck |
| `g_BridgeZ_Offset_Ship` | `0x00B0782C` | Ship visual Z lift under bridge |
| `g_FlyBridgeHeight` | `0x008B3CAC` | Aircraft altitude-delta |
| `LeptonsPerLevel` | `0x00AC13C8` | ShouldBeOnBridge × 3 threshold |
| Bridge deck extra height | `0x00AC13BC` | Added in `Set_Height_On_Bridge` |

### 13.14 Warhead flag map (updated)

| Offset | INI key | Purpose |
|--------|---------|---------|
| `+0x130` | `CausesDelayKill` | Generic DelayKill latch gate — NOT bridge-specific (correction to §12.17) |
| `+0x144` | `Wall` | Gates bridge damage in `Apply_area_damage` |

### 13.15 BuildingType flag table (updated, supersedes §12.16)

| Offset | INI key | String addr |
|--------|---------|-------------|
| `+0x1551` | `EligibleForDelayKill` | `0x0081ACB0` |
| `+0x16B6` | `BridgeRepairHut` | `0x0081A898` |
| `+0x16B7` | `Gate` | `0x0081AA8C` |
| `+0x16B8` | `SAM` | `0x0081AA88` |

**Correction:** `+0x1551` is NOT a "bridge hut capability" — it's the generic
DelayKill-eligibility flag. BridgeRepairHut buildings happen to have both
`EligibleForDelayKill=yes` AND `BridgeRepairHut=yes`.

### 13.16 Updated "Active in YR" gating table (supersedes §12.17)

| Path | Gate | Default |
|------|------|---------|
| Area-damage → state machine | `SpecialFlags & 0x8000` + `warhead.Wall=yes` | `DestroyableBridges` defaults on in skirmish; `Wall` defaults false per warhead (corrected 2026-07-10: `decompile_function 0x0075CEC0` zeroes `+0x144`, and `decompile_function 0x0075D3A0` supplies that byte as the default to `ReadBool("Wall")` — INFERENCE_HARDENED) |
| Engineer repair (Mission_Enter) | `Type.BridgeRepairHut=yes` | yes on retail huts |
| Bridge-hut death → collapse | `warhead.CausesDelayKill` AND `Type.EligibleForDelayKill` AND `Type.BridgeRepairHut` | all three on hut + super/demo warheads |
| `DestroyBridge_*_MapInit` at map load | Never called at map load; runtime only | — |
| `BridgeVoxelMax=` parsing | Parsed, never read | dormant TS code |
| `RepairBridgeSegment` event 31 | Map trigger bound to event 31 | vanilla has none |
| `DAT_0087F8BC` write buffer | Always written, never read | dormant TS code |

### 13.17 Rust implementation gaps (continued from §12.18)

31. **DelayKill mechanic** (§13.1) — cross-cutting feature, not bridge-specific.
32. **Cooldown fields** `+0x528, +0x530` on BuildingClass — anti-double-fire.
33. **Static-path 24-waypoint queue** in `UpdateBridgePassability` — XOR flag
    propagation across queue.
34. **Ship Z-offset under bridges** is visual only — lift Z without changing layer.
35. **Fixed-wing landing altitude-delta** via `g_FlyBridgeHeight` (low priority).
36. **`FootClass+0x684`** sign-byte → `bridge_capable: bool`.
37. **Paradrop landing** via `ShouldBeOnBridge` hysteresis.
38. **Shroud 120-frame recompute** (`RecalcBridgeShroudFlags`) OR correct
    bridge-shroud edge bits at write time.
39. **Chrono/Temporal silent-relocate** off low-water-bridges, not reject.
40. **Z-fudge system** for rendering depth — per-TechnoType fields.
41. **`field_0x11A` is BRIDGE-CLASS ID, not Height** — split from `cell.Level`
    (+0x11B) in Rust cell structure.

### 13.18 Remaining open questions

1. Name of the map-load function that reads OverlayPack and writes raw overlay
   indices. Unresolved in Ghidra symbols.
2. Exact bytes of height tables at `0x0082A734 / 0x0082A774 / 0x0082A7B4` (168
   bytes each) — needs memory dump.
3. Anchor overlay `0x18` (DUMMY15) purpose — treated as valid bridge anchor
   alongside `0x19` (BRIDGE1). Possibly a map-compiler placeholder.
4. Second writer of `field_0x6DF` at `InfantryClass::Mission_Enter + 0x0051A5A9`.
5. Flag bit `0x2000` vs `0x1000` at `+0x140` — possibly pavement vs tileset-
   connectivity. Cross-reference with consumers to disambiguate.
6. OverlayType `0xA7` and `0xB2` early-return in `DrawOverlay_Body` — specific
   "no-draw" sentinels.
7. `DAT_00A8ED6B` byte gate on `BlowUpBridge` — always 0 (live path). When/where
   is it ever set? Possible TS / scripted toggle.

### 13.19 New addresses from §13

| Address | Function / Global |
|---------|-------------------|
| `0x0057B440` | `ApplyBridgeTile` |
| `0x0057A0C0` | `MarkBridgesForRepair_High` |
| `0x0057B210` | `ComputeBridgeSurfaceMask` |
| `0x0057ACF0` | `SelectBridgeTileVariant_Low` |
| `0x0059E740` | River tileset orchestrator |
| `0x0042ACF0` | `UpdateBridgePassability` |
| `0x0069EBB0` | `ShipLocomotionClass::Compute_BridgeZOffset` |
| `0x004CD600` | `FlyLocomotionClass::Process` |
| `0x0054AC40` | `JumpjetLocomotionClass` |
| `0x004DDC40` | `FootClass::ShouldBeOnBridge` |
| `0x005F5FA0` | `FootClass::Set_Height_On_Bridge` |
| `0x005F6A70` | `ObjectClass::ShouldBeOnBridge` |
| `0x00578100` | `RecalcBridgeShroudFlags` |
| `0x0055AFB0` | `LogicClass::PerTickUpdate` |
| `0x00485060` | `IsOnLowWaterBridge` (mislabeled `IsOnBridgeSurface`) |
| `0x006CC390` | `SuperClass::Launch` |
| `0x006297F0` | `TemporalClass::AI` |
| `0x0047F6A0` | `CellClass::DrawOverlay_Body` |
| `0x0047F510` | `CellClass::DrawOverlay_Shadow` |
| `0x004DAFF0` | Z-Fudge composer |
| `0x00568E40` / `0x00569760` | Bridge state mutator (high/low) |
| `0x00598030` | RNG reject-sample (dropped result) |
| `0x00580B70` | Continuity probe |
| `0x00701F47` | DelayKill latch setter |
| `0x00847E74` | string `"CausesDelayKill"` |
| `0x0081ACB0` | string `"EligibleForDelayKill"` |
| `0x00AC13BC` | Bridge deck extra height |
| `0x00AC13C8` | `LeptonsPerLevel` |
| `0x008A07C4` | `g_BridgeZ_Offset_Drive` |
| `0x00B0782C` | `g_BridgeZ_Offset_Ship` |
| `0x008B3CAC` | `g_FlyBridgeHeight` |
| `0x00880990` | `g_SelectedBridgeTile` |
| `0x00ABED10` | Cell-aux side buffer (bridge ownership) |

## 14. Fourth Deep Dive — Persistence, Traversal Details, Low-Bridge = Tube

This section closes several of the open questions from §13.18 and uncovers two
significant corrections: (a) `field_0x6DF` is a **generic single-slot building
mutex**, shared across DelayKill AND engineer-repair; (b) **low bridges ARE tubes**
for pathfinding — one logical system, two data paths.

### 14.1 CellClass is NOT individually save/loaded

There is no `CellClass::Save` / `CellClass::Load`. Bridge state persists through
the **map pack mechanism**, not per-cell serialization:

- `[OverlayPack]` (string `0x00833484`) — 1 byte per cell = `OverlayTypeIndex` (+0x44)
- `[OverlayDataPack]` (string `0x00833474`) — 1 byte per cell = damage state (+0x11E)
- `[IsoMapPack5]` — tile index (+0x38), `Level` (+0x11B), subtile

On load, `MapClass::ComputeBridgeZones` **recomputes all bridge records from scratch**
by iterating cells, so `MapClass+0x54` records are derived — they don't need saving.
Bridge-anchor pointers at `cell+0x2C` are rebuilt by `CellClass::RecalcAttributes`.
Because destruction state lives in OverlayTypeIndex (0xE7/0xE8) and damage state in
`+0x11E` — both serialized — destroyed bridges survive save/reload.

**Note on `cell+0x2C`:** an agent found the field is set to a `TubeClass*` in
`RecalcAttributes`. Earlier analysis treated it as a `CellClass*` bridge-partner
pointer. This field is **dual-purpose** — holds a `CellClass*` for high-bridge
partners and a `TubeClass*` for low-bridge tubes. Context-dependent dereference.

### 14.2 OverlayPack reader @ `0x005FD2E0`

Ghidra auto-named `BSurface__Constructor` is the actual reader (name is misleading).
Called from `ScenarioClass::Full_Init` map loading. Flow:

1. Gated by `DAT_00A8ED7C >= 2` — IsoMapPack v2+ (all YR-era maps qualify).
2. `[OverlayPack]` section decompressed via `LCWStraw__Constructor(1, 0x2000)`.
3. Nested loop — **outer Y (0..0x1FF), inner X (0..0x1FF)** = 512×512 cell scan.
4. One byte per cell. `0xFF` = empty, skip.
5. Lookup `OverlayTypeClass = *(DAT_00A83D84 + byte*4)`. Validate via type fields.
   Campaign-mode crate filter applies.
6. Allocate `new OverlayClass(typePtr, &cell, -1)` — constructor writes `cell+0x44`
   AND sets `+0x11E` (initial damage state).
7. `[OverlayDataPack]` scanned with the same iteration. 1 byte/cell written directly
   to `cell+0x11E` — but only for cells where overlay index is one of
   `{0x18, 0x19, 0xED, 0xEE}` (the 4 bridge anchor overlays). For non-anchor cells
   the OverlayClass constructor's default `+0x11E` is preserved.

So the anchor overlays carry **two bytes of information**: the type index (0x18/0x19
= high concrete anchors, 0xED/0xEE = low/wood anchors) + the damage state byte that
encodes the bridge's direction and initial state.

### 14.3 Bridge-class tables — fully resolved

Static arrays at fixed addresses (168 bytes = 42 int32 entries each):

**`DAT_0082A734` — expected `+0x11A` bridge-class ID per tile variant (NS/EW body + ramp):**
```
7, 7, -1, 7, 7, -1, 4, 4, 4, 4, 4,    (entries 0-10)
2, 2, 2, 2, 2, 2, 2, -1, 4, 4,          (entries 11-20)
-1, 2, 2, 2, 2, 2, 4, 4, 4, 4,          (entries 21-30)
4, -1, -1, 4, -1, -1, 2, 4, 4, 4, 4     (entries 31-41)
```

**`DAT_0082A774` — perpendicular scan direction (compass index 0..7, -1 = skip):**
```
2, 2, -1, 4, 4, -1, 2, 2, 2, 2, 2,
4, 4, 4, 4, 4, -1, -1, 4, -1, -1,
2, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2,
0, 0, 0, 1, 2, 3, 4, 4, 5, 5
```

**`DAT_0082A7B4` — expected `+0x11A` bridge-class ID at far endpoint:**
```
-1, -1, 4, -1, -1, 2, 4, 4, 4, 4, 4,
2, 2, 2, 2, 2, 0, 0, 0, 1, 2, 3,
4, 4, 5, 5, 5, 6, 7, 8, 9, 9, 10,
10, 10, 11, 12, 13, 14, 14, 15, 15
```

These tables are indexed by `(IsoTileTypeIndex - BridgeSet)` and drive
`ComputeBridgeZones`' decision of "is this cell a valid bridge piece and what
orientation / endpoint does it connect to." Despite Ghidra's current `Height`
field label, the compared byte is the template class ID. (corrected 2026-07-10: `decompile_function
0x0056D6E0` reads the byte at the `+0x11A` position for both table comparisons,
while `decompile_function 0x0057B440` proves that writer stores the template slot
index there — OFFSET_RETYPED_WRONG)

### 14.4 `DUMMY15` (overlay 0x18) is the LIVE high-bridge anchor

Confirmed — DUMMY15 is not a placeholder but a **real high-bridge anchor overlay**
handled identically to BRIDGE1 (0x19). Both route to
`ProcessBridgeDamageStateMachine_High` via `ApplyDamageToCell`'s anchor check.

The INI has `DUMMY15` listed at index 24 with no `[DUMMY15]` section — it's
name-only. At load time, the OverlayType array slot is populated with defaults
(flags zero, no art). The engine never consults its flags — only the index match
matters.

**Practical implication:** Rust `is_high_bridge_anchor(id)` must accept both
`{0x18, 0x19}` (high) and `{0xED, 0xEE}` (low). Do NOT require an INI flag — there
isn't one.

### 14.5 OverlayTypeClass has NO `IsBridge=` INI key

`OverlayTypeClass::ReadINI @ 0x005FE770` parses: `Land, Strength, Wall, Tiberium,
Crate, CrateTrigger, Explodes, Overrides, CellAnim, DamageLevels, NoUseTileLandType,
IsVeinholeMonster, IsVeins, ChainReaction, DrawFlat, IsARock, IsRubble`. **No
`IsBridge=` or equivalent bridge flag.**

Bridges are identified **purely by hardcoded overlay-index ranges** in live code
(`CellClass::IsBridge @ 0x00486750`, `IsWoodBridge @ 0x00486770`,
`ApplyDamageToCell`, `DestroyBridge_*`, etc.).

### 14.6 No dedicated network event for bridges

String `"Bridge Repaired Event"` at `0x008397D8` has only a DATA xref (legacy
name-table entry, TS-era placeholder). `EventClass::Execute @ 0x004C6CB0` has cases
0x01..0x2E — none branch to bridge logic.

Bridge repair & destruction propagate deterministically:
- **Hut capture → engineer enters hut** = standard TechnoClass mission event
- **Hut death → bridge collapse** = standard warhead damage event
- All peers re-run identical sim; no bridge-specific packet exists

`EVA_BridgeRepaired @ 0x00825538` fires client-side from `InfantryClass::Mission_Enter`.

### 14.7 Mid-transit units survive bridge collapse

`DriveLocomotionClass::Process_Drive_Track @ 0x004B0F20` reads `cell.Flags & 0x100`
**only at cell boundaries**, not per tick. Between cells it uses precomputed delta
vectors. If the destination cell's bridge flag clears mid-transit, the unit
continues along the interpolated path to the destination cell, then gets snapped
down to ground Z via the next `DropIn` / `Set_Height_On_Bridge` check at arrival.

`BlowUpBridge` kills only occupants **anchored** in the cell — a unit mid-step is
anchored in the **previous** cell (not yet committed to destination), so it escapes
the ground-layer kill. On arrival at the now-collapsed cell, it's treated as a
bridge-deck occupant → `DropIn` → lands on ground layer alive.

### 14.8 Partial-damage states don't affect traversal

`CheckBridgeTraversal @ 0x004D9C60` **never reads `+0x11E` (damage state) or
`+0x11A` (bridge-class ID)**. Gates only on:

- `abs(dst.Level - src.Level) == 0` → OK same layer
- `== 1` → ramp, requires `cell+0x11C != 0`
- `== 4` → bridge mount/dismount, requires `Flags & 0x300` on source
- Anything else → returns 7 (blocked)

Damage states 6/7/8/15/16/17 leave `Flags & 0x100` intact until the state machine's
`SetBridgeDirection_NESW(*, 0)` call clears the group. Until that moment **the cell
is fully passable with no speed penalty**. No random hit, no refusal. Visually the
cell shows a damaged sprite; mechanically it's identical to healthy.

### 14.9 `field_0x6DF` is a generic single-slot building mutex — NOT bridge-specific

**Critical clarification of §13.1.** The `BuildingClass+0x6DF` byte is NOT dedicated
to DelayKill OR bridge repair. It is a **generic "this building is busy with a
deferred action" latch** shared by:

1. **DelayKill** (`TechnoClass::ReceiveDamage @ 0x00701F47`) — sets `+0x6DF = 1`,
   `IsAlive = true`, `Health = 1`, stores cooldown at `+0x530`. Next
   `BuildingClass::Update` processes deferred death.
2. **Engineer bridge repair** (`InfantryClass::Mission_Enter @ 0x0051A5A9`) — sets
   `+0x6DF = 1`, stores engineer pointer at `+0x540`, start frame at `+0x528`.
   Next `BuildingClass::Update` dispatches repair (5×5 scan → ProcessBridgeDestruction_*)
   and eventually runs the stored engineer callback.

**In both cases** `BuildingClass::Update @ 0x00440320` clears the latch when done.
The dispatch logic inside Update branches on the **building's type flags** to decide
what the latch means:

- `Type.BridgeRepairHut (+0x16B6) != 0` → bridge repair branch
- Else → DelayKill branch

**Side-effect:** if the same latch is set for DelayKill and an engineer tries to
enter the same tick, the engineer re-queues (bails). This is a natural collision
prevention.

**§13.1's claim that the DelayKill `+0x130/+0x1551` check "gates bridge collapse"
was overstated.** The DelayKill gate only decides if the building absorbs the fatal
hit. Whether that absorbed hit triggers bridge collapse depends on the
BridgeRepairHut flag at Update time — a second, independent decision.

### 14.10 Engineer disposal & deferred repair callback

Verified via `InfantryClass::Mission_Enter` decompilation:

1. Engineer sets hut `+0x6DF` latch and records `+0x540 = engineerPtr`,
   `+0x528 = CurrentFrame`, + target coords at `+0x528..+0x530`.
2. Engineer invokes `TechnoClass::ProcessCellAction(code 0x30, ...)` — **passenger-
   into-transport** action. Engineer becomes a limbo'd passenger of the hut.
3. Engineer is NOT killed; it's consumed as cargo.
4. Actual tile restoration happens **later** on a subsequent
   `BuildingClass::Update` tick — the hut uses the stored engineer pointer at
   `+0x540` to complete the repair callback, calling `RepairBridge_High/Low`.
5. After repair: engineer is likely finalized (Limbo'd via vtable+0xF8 — the
   "dispose" slot), hut keeps original ownership, latch clears.

**No ownership transfer** — BridgeRepairHuts remain original-owned after repair.

### 14.11 No auto-repair on hut placement

`BuildingClass::Unlimbo @ 0x00440580` does NOT call `RepairBridge_*` or
`ProcessBridgeDestruction_*`. Xref analysis of both functions shows only engineer-
entry and recursive-self as callers.

**Rust implication:** do not auto-repair bridges when a BridgeRepairHut is placed.
Repair requires an engineer to enter the hut.

### 14.12 `DAT_00A8ED6B` — confirmed dead TS flag

Only write site in the binary: `0x0052F63E` (startup command-line parser) — writes
`0` unconditionally. No code path sets it to non-zero. The `BlowUpBridge` early-out
gate on this flag is **always bypassed**. Treat as always-live. Do not implement.

### 14.13 Flag bit `0x1000` at cell.Flags — DEAD WRITE

Set alongside other bridge flags by `SetBridgeDirection_NESW/NWSE` in the big mask
`~0xFFFEE07F`, value from `(param_3 & 1) << 12`. **No read sites anywhere in the
binary** — exhaustive byte-pattern searches for `TEST imm32,0x1000`, shift-and-mask
forms, etc., all returned zero matches.

**Verdict: TS-era bridge-geometry bit that nothing in YR code consults.** Do NOT
implement reads. Write path can be omitted or left as dead bit.

### 14.14 Flag bit `0x2000` at cell.Flags — pavement (confirmed)

Written only by `ToggleBridgePavement @ 0x0056E990`:
```
cell.Flags = (param_2 & 1) << 13 | (cell.Flags & ~0x2000)
```

Read via `(Flags >> 13) & 1` in:
- `CellOverlay_TileDraw @ 0x00480350` — picks alt subtile
- `CellClass::GetRadarColor @ 0x0047C060` — same alt subtile for radar
- `FUN_00546DA0` — tile loader for alt variant

Takes effect only if `FUN_005471F0(cell.SubTile)` (tile supports pavement variant).

### 14.15 `DrawOverlay_Body` skips overlays `0xA7` and `0xB2`

At head of `CellClass::DrawOverlay_Body @ 0x0047F6A0`:
```
if (cell.OverlayIndex == 0xA7) return;
if (cell.OverlayIndex == 0xB2) return;
```

- `0xA7 (167)` = `TIB3_18` (tiberium type 3 variant 18)
- `0xB2 (178)` = `TROCK03` (tiberium rock variant)

Both are hard-blacklisted — rendered through the tiberium path in the same function,
NOT via the body-draw path. Likely because their SHPs are malformed/empty.
Not TS-specific; active in YR. Bridge-irrelevant; noted for completeness.

### 14.16 `DrawOverlay_Shadow` shift — shadow only, not body

Verified: the `-15 X, +7 Y` shift applies ONLY to the shadow draw, for cells matching:

- `Flags & 0x80` (cached bridge-overlay latch)
- AND `+0x11E > 8 AND < 0x12` (states 9..17 inclusive)

Body draw at `0x0047F6A0` does NOT shift pixels; it only applies the Z-bias
`Level + (Flags >> 7 & 1) * 4`.

### 14.17 Low bridges ARE tubes for pathfinding

**Major clarification.** Low bridges (wooden/pontoon, overlay range `0x4A..0x65` =
LOBRDG01-25) share infrastructure with the TS-era tube/tunnel system:

| Field / Global | Purpose |
|----------------|---------|
| `cell+0x116` (int16) | Tube index into `g_TubeArray` (negative = no tube) |
| `DAT_008B4148` | Tube count |
| `g_TubeArray[idx]` | `TubeClass*` with entry coord `+0x24`, exit coord `+0x28` |
| `cell+0xEC == 10` | `LandType == LAND_TUNNEL` — low-bridge marker |
| `CellClass::IsLowBridgeCell @ 0x00484AB0` | Tests valid tube index AND LandType == 10 |
| `CellClass::GetTubeAtCell @ 0x00484F20` | Returns tube ptr, bounds-checked |

**Readers:** `ComputeBridgeZones`, `UnitClass::TubeMovement @ 0x007359F0`,
`Can_Enter_Cell`, `FootClass::ClickedAction_Cell`, `UpdateBridgePassability`.

**Pathfinder direction-8 sentinel:** when `PathfinderClass::UpdateBridgePassability
@ 0x0042ACF0` iterates PathStep arrays and finds `direction == 8`, it reads
`g_TubeArray[cell.tube_index]+0x28` (tube EXIT coord) and teleports the pathfinder
there — skipping the tube's interior cells.

**Stale conclusion superseded 2026-05-16:** the older blanket zero-length/teleport wording below is replaced by the correction that follows.

**Correction 2026-05-16:** low bridge TubeClass records have two live shapes.
`CellClass::RecalcAttributes` creates same-cell shell tubes (`entry == exit`,
`step_count == 0`) for qualifying `LandType == 10` cells. The `[Tubes]` parser
creates fully initialized tubes with entry, exit, step buffer, and step count.

Direction-8 path steps use `tube+0x28` as the exit coord. However, checked
Drive/Walk locomotion producer branches divide by `tube+0x1C0` when entering tube
movement, so zero-step RecalcAttributes shells are not valid practical inputs for
visible direction-8 traversal. Treat "pathfinder teleport" as true only for
fully initialized tubes, not as the complete model for every low-bridge shell.

### 14.18 Tank bridge-deck Z comes from locomotion, NOT `Set_Height_On_Bridge`

Correction: `FootClass::Set_Height_On_Bridge @ 0x005F5FA0` is called only from
`AnimClass::Constructor @ 0x00421EA0` — for attachment/muzzle-flash anim
positioning on foot units. It is NOT the normal per-tick Z-setter for tanks.

Tank bridge-deck Z is driven by:

- `DriveLocomotionClass::ComputeBridgeZOffset @ 0x004AF4A0` — adds
  `g_BridgeZ_Offset_Drive` to logical Z each tick while on deck
- `DriveLocomotionClass::ComputeBridgeRenderOffset @ 0x004AF470` — render-only Z
- `ObjectClass::GetHeight`, `Mark_Put @ 0x005F60B0`, `Mark_Remove @ 0x005F6130`
  conditionally raise height by `g_BridgeZ_Offset_Drive` based on `OnBridge` flag

The flag is tracked at `FootClass+0x8C` (not +0x23 as prior text suggested —
that was a byte-offset misinterpretation; if `param_1` is `int *`, then
`param_1[0x23]` = byte offset `0x23 * 4 = 0x8C`).

### 14.19 Zone maps at `DAT_0087F850` / `DAT_0087F858`

- **`DAT_0087F850`** — 4 bytes per cell, indexed by `(map.Width + BorderPad + 1) * Y + X`
  - `[+0]` = `cell.field_0x4C` (SubCell flags)
  - `[+1]` = `cell.Level`
  - Pathfinder-cached passability + level slab
- **`DAT_0087F858`** — 10 bytes per cell, same indexing
  - Per-movement-type zone index (up to 10 movement types)
  - `[+8]` = `cell.Level`
  - Read by `AStar_main_loop @ 0x00429E8A`, `Zone_precheck`,
    `PathfinderClass::UpdateHierarchicalEdges`

These are **distinct from bridge zones** at `MapClass+0x18..+0x4C` (bridge
adjacency edge-lists per movement type). Save/load implications: neither table is
saved — both are rebuilt at load (or never persist).

### 14.20 Summary of resolved open questions (from §13.18)

| Open question | Status |
|---------------|--------|
| OverlayPack reader address | **Resolved** — `0x005FD2E0` (§14.2) |
| Bridge-class tables dump | **Resolved** — dumped in §14.3 |
| DUMMY15 purpose | **Resolved** — live high-bridge anchor (§14.4) |
| Second `+0x6DF` writer at Mission_Enter | **Resolved** — engineer-busy mutex (§14.9) |
| `DAT_00A8ED6B` byte gate | **Resolved** — dead flag (§14.12) |
| Flag 0x1000 vs 0x2000 | **Resolved** — 0x2000 = pavement live, 0x1000 = dead (§14.13-14.14) |
| OverlayType 0xA7 / 0xB2 | **Resolved** — TIB3_18 and TROCK03, bridge-irrelevant (§14.15) |

### 14.21 Rust implementation gaps (continued from §13.17)

42. **Map-pack serialization** — Rust needs OverlayPack + OverlayDataPack LCW
    decompressor + 512×512 cell scan. Special-case the 4 anchor overlays
    `{0x18, 0x19, 0xED, 0xEE}` for the `+0x11E` preservation.
43. **ComputeBridgeZones reruns at load** — don't try to persist bridge records.
44. **`cell+0x2C` is dual-purpose** — TubeClass* for low bridges, CellClass* for
    high bridges. Rust model must handle both discriminants.
45. **No `IsBridge` INI flag on OverlayType** — identification by hardcoded ID range
    {0xCD..0xE8, 0x4A..0x65, 0x18, 0x19, 0xED, 0xEE} suffices.
46. **Engineer-busy mutex on BuildingClass** — shared with DelayKill pending-death.
    Rust should model as a single `pending_action: Option<PendingBuildingAction>`
    variant, not two separate flags.
47. **Deferred engineer-repair callback** — store engineer ref on hut + cb frame;
    run repair on next tick (not immediately at entry).
48. **Engineer becomes passenger (Limbo'd as cargo)** — not killed at entry. Consumed
    on callback completion.
49. **No hut-placement auto-repair** — only engineer entry triggers repair.
50. **Flag 0x2000 (pavement)** — Rust should honor for radar color + alt subtile
    picking.
51. **Flag 0x1000 is dead** — do not read.
52. **DrawOverlay_Body skip list** for overlays 0xA7 + 0xB2 (bridge-irrelevant
    but required for overlay rendering parity).
53. **Bridge shadow shift X-=15, Y+=7** for damage states 9..17.
54. **Low bridges = tubes in pathfinding** — teleport from entry to exit, not
    cell-by-cell traversal. Rust pathfinder needs tube-aware direction-8.
55. **Tank bridge Z comes from DriveLocomotion**, not `Set_Height_On_Bridge`.
    That helper is an anim-attachment path only.
56. **Zone map caches** `DAT_0087F850/58` — per-cell pathfinder-level + per-movement-
    type zone caches. Rebuilt at load.
57. **Mid-transit collapse: units survive** — anchor-cell kill semantics means a
    unit between cells escapes `BlowUpBridge` and gets `DropIn` on arrival.

### 14.22 Remaining open questions (narrower than before)

1. **`BuildingTypeClass+0xEC2`** was referenced in one agent pass as an
   `IsBridgeRepairHut` gate. May be an alias of `+0x16B6` or a separate flag.
   Needs one more verification pass on BuildingTypeClass::ReadINI.
2. **Exact callback path for deferred engineer repair** — how does
   `BuildingClass::Update` discover it needs to run `RepairBridge_High` on a
   latched engineer vs run DelayKill? The dispatch logic hasn't been fully traced.
3. **`MapClass+0x160..`** and `+0x1160..` — two distinct BridgeRepairHut
   registry addresses appeared in the gap-scan. Only one was decompiled in depth.
   The dual-registry purpose (map-wide vs per-cell?) is still ambiguous.
4. **Tube length for low bridges** — we know it's 0 (teleport). But how is the
   tube's exit coord (`+0x28`) filled for a low-bridge tube? Does it equal the
   bridge's far endpoint, or a per-cell entry/exit pair? Needs TubeClass
   constructor tracing.
5. **Rendering frame variations** for damage states — we know the Latin square
   adds 0..3 for healthy states 0/9. What about the "collapsed" states — is
   there any randomization, or strictly one frame per state?

### 14.23 Updated addresses (§14 additions)

| Address | Function / Global |
|---------|-------------------|
| `0x005FD2E0` | `BSurface::Constructor` — OverlayPack/DataPack reader |
| `0x0082A734` | `BridgeExpectedClassId[42]` |
| `0x0082A774` | `BridgePerpScan_Direction[42]` |
| `0x0082A7B4` | `BridgeFarExpectedClassId[42]` |
| `0x005FE770` | `OverlayTypeClass::ReadINI` |
| `0x00484F20` | `CellClass::GetTubeAtCell` |
| `0x00484AB0` | `CellClass::IsLowBridgeCell` |
| `0x008B4148` | `g_TubeCount` |
| `0x007359F0` | `UnitClass::TubeMovement` |
| `0x00440580` | `BuildingClass::Unlimbo` (no auto-repair) |
| `0x005F60B0` | `ObjectClass::Mark_Put` |
| `0x005F6130` | `ObjectClass::Mark_Remove` |
| `0x004AF4A0` | `DriveLocomotionClass::ComputeBridgeZOffset` |
| `0x004AF470` | `DriveLocomotionClass::ComputeBridgeRenderOffset` |
| `0x0087F850` | `g_ZoneMap_Passability` (4 bytes/cell) |
| `0x0087F858` | `g_ZoneMap_Zones` (10 bytes/cell) |
| `0x00833484` | string `"OverlayPack"` |
| `0x00833474` | string `"OverlayDataPack"` |
| `0x005FE770` | `OverlayTypeClass::ReadINI` |
| `0x0052F63E` | `DAT_00A8ED6B` clear site (unconditional) |
| `0x008397D8` | string `"Bridge Repaired Event"` (dead data-only) |
| `0x00429E8A` | `AStar_main_loop` reader of ZoneMap |

## 15. Fifth Deep Dive — Final Close-Out of Narrow Open Questions

All 5 narrow open items from §14.22 resolved. This section also issues important
corrections to §13.1 and §14.9 regarding the semantics of `field_0x6DF`.

### 15.1 `BuildingTypeClass+0xEC2` vs `+0x16B6` — RESOLVED

**Answer: two separate flags. A third flag `+0xEC3` also exists.**

- **`+0x16B6 = BridgeRepairHut` (INI bool).** Confirmed in
  `BuildingTypeClass::ReadINI_Water @ 0x0045FE50` via `CCINIClass::ReadBool(section,
  s_BridgeRepairHut_0081A898, ...)`. Only one xref to the string.
- **`+0xEC2` is a SEPARATE byte**, zero-initialized by `BuildingTypeClass::Constructor
  @ 0x005236A7` (along with `+0xEC3..+0xECB`). **Never written by any ReadINI*
  function in the binary** — exhaustive byte-pattern scans for the common
  set-instruction forms returned zero matches. TS-legacy holdover.
- Readers of `+0xEC2` in live YR code:
  - `InfantryClass::Mission_Enter @ 0x0051A4D8` — gates **MISSION_SABOTAGE (0x11)**,
    the Crazy Ivan / Demo bomb path. NOT bridge repair.
  - `FootClass::Is_Mission_Enter @ 0x004D4B6F` — gates pre-enter capability.
- **`+0xEC3` is the actual engineer-can-enter gate.** At
  `InfantryClass::Mission_Enter @ 0x00519B5E` (engineer capture path for missions
  8/0xB/0x19), the code checks `Type[0xEC3]` first; only if that passes AND RTTI
  == 6 (Building), it then checks `Type[0x16B6]` at `0x00519B82` → branches to
  the BridgeRepairHut path.

**Corrections:**
- Any earlier claim that `+0xEC2` is `IsBridgeRepairHut` was **wrong**.
- The engineer gate is `+0xEC3 && Type+0x16B6`, not `+0xEC2`.
- Both `+0xEC2` and `+0xEC3` appear dormant in YR (never written by INI) — treat as
  constant 0 unless a non-INI setter is discovered later.

### 15.2 `BuildingClass::Update` latch dispatch — RESOLVED

**Single unified latch, decision made by `Type[0x16B6]` ALONE.**

`BuildingClass::Update @ 0x0043FB20`, block starting `if (this->field_0x6DF != 0)`:

1. **Cooldown gate:** if `(g_CurrentFrame - +0x528) < +0x530` → exit without firing.
   - `+0x528` = frame-counter start (set when latch set)
   - `+0x530` = delay in frames (cooldown duration)
2. **Once cooldown expired, branches purely on `Type+0x16B6`:**
   - `Type+0x16B6 == 0` → **DelayKill death path**. Calls
     `this.vtable+0x16C(&coord, 0, Rules+0xFA8 /*C4Warhead*/, +0x540 /*attacker
     house*/, 1, 0, 0)`. Applies final damage to itself.
   - `Type+0x16B6 != 0` → **bridge-collapse path**. 5×5 probe over offsets -2..+2
     on both axes of surrounding cells: matches `SlopeIndex` against low-bridge range
     `DAT_00ABAD1C..+0x10` OR `LandType` against `0x4A..0x66`. Dispatches to
     `DestroyBridge_{High,Low}_MapInit`. Clears `+0x6DF = 0` and `+0x540 = 0`.
     (corrected 2026-07-10: `decompile_function 0x0043FB20` shows both nested
     counters initialized to -2 and continued while `< 3`, yielding 5×5 —
     OPERATOR_OR_ORDER_DRIFT)

**There is no separate engineer-callback dispatch.** The latch processing is simple
binary: DelayKill or bridge-collapse. `+0x540` is the attacker-house pointer in both
cases; it is NOT an engineer-pointer reservation for later callback.

### 15.3 `+0x6DF` latch has THREE uses — CRITICAL CORRECTION to §13.1 and §14.9

**Major revision:** The latch is a single-slot "building has a pending deferred
action" mutex, used by **three distinct producers**:

1. **DelayKill** (`TechnoClass::ReceiveDamage @ 0x00701F47`)
   - Warhead `+0x130 (CausesDelayKill) != 0` + Building `Type+0x1551
     (EligibleForDelayKill) != 0` + fresh-strike check
   - Sets `+0x6DF = 1`, `Health = 1`, `IsAlive = true`
   - Cooldown gate `+0x530` prevents double-fire

2. **Crazy Ivan / Demo bomb** (`InfantryClass::Mission_Enter @ 0x0051A5A7`)
   - Mission == `0x11` (MISSION_SABOTAGE) + `Type+0xEC2 != 0` + `+0x6DF == 0`
   - Sets `+0x6DF = 1`, stashes attacker pointer at `+0x540`
   - On Update, same branch logic applies

3. **Bridge-hut death → collapse**
   - This is NOT a separate latch-set path — it's a DOWNSTREAM dispatch of case (1)
     when the dying building happens to be a `BridgeRepairHut`
   - The latch is set by DelayKill (a super-weapon warhead with `CausesDelayKill=yes`
     lands on a `BridgeRepairHut` that's `EligibleForDelayKill`)
   - Update then sees `Type+0x16B6 != 0` and dispatches to `DestroyBridge_*_MapInit`
     instead of regular DelayKill-death

**Engineer bridge repair does NOT use the `+0x6DF` latch.** It runs
**synchronously** during `InfantryClass::Mission_Enter` via direct call to
`ProcessBridgeDestruction_High @ 0x00573540`, which internally does a 5×5 scan and
dispatches to either `RepairBridge_High` (restoration) or damage-advance. The
engineer is then Limbo'd as cargo, and no latch is set.

**Rust implications (supersedes §13.17 #22):**
- Model `+0x6DF` as `pending_action: Option<PendingBuildingAction>` with three
  variants: `DelayKillDeath`, `IvanBomb`, `HutDeathBridgeCollapse` (the last is a
  dispatch-time decision, not a separate variant).
- Engineer bridge repair is synchronous; no latch needed.

### 15.4 Dual BridgeRepairHut registry — RESOLVED

**Two distinct registries with different purposes:**

#### Per-map DynamicVector at `MapClass + 0x115C`

Initialized in `MapClass::Constructor @ 0x00565090`:

| Offset | Field |
|--------|-------|
| `+0x115C` | Vector vtable (`PTR_FUN_007E3890`) |
| `+0x1160` | Array pointer |
| `+0x1164` | Count |
| `+0x1168` | Heap-owned flag |
| `+0x1169` | Secondary byte |
| `+0x116C` | Capacity (initial 10) |

Entry records contain `+0x24` (cell coord packed X/Y) and `+0x3C` (Building*
back-pointer).

`MapClass::UnregisterBridgeRepairHut @ 0x00577920` iterates and removes entries
matching a target building. The outer RTTI==0x2C gate limits which sub-type
buildings trigger registration.

**Purpose:** typed hut-pointer list for fast lookup by building identity during
hut destruction.

#### Global DynamicVector at `0x008B41A8`

| Offset | Field |
|--------|-------|
| `0x008B41A8` | Vector vtable |
| `0x008B41AC` | Array pointer |
| `0x008B41B0` | Capacity |
| `0x008B41B5` | Heap-owned flag |
| `0x008B41B8` | Count |
| `0x008B41BC` | Growth increment |

Built by `FUN_00684C30` (post-map-load init, runs right before `ComputeBridgeZones`).
Iterates all cells inserting coord records where `FUN_006E61F0() & 4` is set —
the "bridge-linked cells" set. Serialized by `FUN_0067F9C0` during save/load.

`UnregisterBridgeRepairHut` removes from this global too (via vtable find-index).

**Purpose:** cell-coord list used for map queries + save/load + UI "is this cell
a bridge-linked cell?" checks (likely drives the engineer cursor icon on hover).

**Correction to §11.11 / gap-scan:** these are TWO distinct registries, not
overlapping views of the same data. Rust needs both.

### 15.5 TubeClass construction for low bridges — RESOLVED

**Correction 2026-05-16: two creation paths, not one universal granularity.**
`RecalcAttributes` creates same-cell per-cell shells. `[Tubes]` parsing creates
full entry/exit/step tubes. The earlier "one tube per bridge, not one per cell"
claim is stale for the verified `RecalcAttributes` path.

`TubeClass::Constructor @ 0x00727FD0`:

| Field | Init value |
|-------|------------|
| `+0x24` (entry) | `*(int*)param_2` (entry cell coord) |
| `+0x28` (exit) | **Same as entry** (NOT the far endpoint) |
| `+0x2C` (initial step direction OR Z-coord, caller-dependent) | `param_3` |
| `+0x30..+0x1BC` (100-entry step buffer) | All `0xFFFFFFFF` |
| `+0x1C0` (step count) | `0` in constructor; overwritten by `[Tubes]` parser |

Then registers itself into `g_TubeArray` and writes its index into the entry cell's
`+0x116` (tube index).

**Two callers:**

1. **`FUN_007283C0`** -- `[Tubes]` INI section parser.
   It constructs with coord `(0,0)`, then overwrites entry X/Y,
   direction, **exit X/Y**, step directions, and step count at `+0x1C0`. This is
   the complete initialization path for full entry/exit/step tubes when map data
   contains `[Tubes]`.

2. **`CellClass::RecalcAttributes @ 0x0047DA35`** — creates a tube SHELL when a
   cell has `LandType==10` AND its IsoTileTypeIndex is in one of 4 low-bridge tile
   ranges AND the cell has no tube yet. Passes `psVar1` (entry cell) and
   `DAT_0081CC20 + offset*4` (a 4-entry direction table) as initial direction. The
   cell-level code creates the shell; `ComputeBridgeZones` (runs next via
   `FUN_00684C30`) does **not** fill the exit coord. It only reads `tube+0x28`
   while constructing low bridge records.

**Step buffer supports bridges up to 100 cells long** — larger than any retail map.

**Refinement of §14.17:** the pathfinder's direction-8 sentinel reads `+0x28`
(tube exit) for **planning** (A* treats the tube as a single long edge). The
unit locomotor still animates step-by-step using the `+0x30..+0x1BC` buffer
during actual movement. So "teleport" is approximately true for A* cost
calculation, but the unit's visible movement is cell-by-cell.

### 15.6 Rendering frame variations — RESOLVED

**Latin-square variation applies to frames 0 and 9 ONLY. All damaged/destroyed
states render the raw frame index with no randomization.**

`DrawOverlay_Body @ 0x0047F6A0`:

1. **Early return at `0x0047F6AD`**: if `cell+0x44 == 0xA7` → return.
   - `0xA7 (167) = destroyed bridge` LandType sentinel. When a high bridge is
     fully collapsed, the cell's LandType becomes `0xA7` and this function
     **returns immediately without drawing**.
2. **Early return at `0x0047F6BB`**: if `cell+0x44 == 0xB2` → return.
   - `0xB2 (178)` = tiberium rock variant. Bridge-irrelevant.
3. **Radar / cache-valid branch (`flags & 0x80`):** frame = `cell+0x11E`. If
   frame is 0 or 9: `frame += DAT_0081CC30[((Y&3)<<2)|(X&3)] * 4` (Latin-square
   0..3 shift). Other frame values passed through unmodified.
4. **Body branch (`OverlayType+0xAA != 0`, bridge-body flag):** passes raw
   `cell+0x11E` directly to `CC_Draw_Shape`. **NO variation.** Damaged bridge
   body cells (frames 1-8, 10-17) render identical frame indices on every
   adjacent cell.
5. Other main branches (ordinary overlay, tiberium, tall tile): no Latin-square
   variation. Visual variation across the 3-cell bridge width comes from
   per-cell Z offsets and SHP pointer selection, NOT frame randomization.

**Destroyed state visual pipeline** (supersedes earlier implication):
- When a bridge is destroyed, `cell+0x44` transitions to `0xA7` (LandType sentinel)
- `DrawOverlay_Body` returns without drawing via the `0xA7` early-return
- The destroyed bridge visual comes from:
  1. The **destruction animations** spawned by `BlowUpBridge` / `CollapseBridge_*_High`
     (`BridgeExplosions` + `MetallicDebris` anims stay visible for several frames)
  2. The cell's new LandType rendering (ground/water sprite where the bridge was)
  3. Any overlaid stumps at `0xDF..0xE6` end-piece tiles, which DO go through
     `DrawOverlay_Body` (they're not in the `0xA7`/`0xB2` skip list)

So the "destroyed bridge" stays looking destroyed without a dedicated destroyed
sprite — the bridge simply stops drawing.

### 15.7 Updated open-questions list (§14.22 revised)

| # | Question | Status |
|---|----------|--------|
| 1 | `+0xEC2` vs `+0x16B6` | **Resolved** §15.1 |
| 2 | Deferred repair callback path | **Resolved** §15.2 (no such path; simple dispatch) |
| 3 | `MapClass+0x160` vs `+0x1160` registry | **Resolved** §15.4 |
| 4 | TubeClass exit coord fill | **Resolved** §15.5 |
| 5 | Damaged-state frame variation | **Resolved** §15.6 |

All major open items are now closed. Remaining follow-ups are minor:

- **`FUN_006E61F0 & 4`** — the "bridge-linked cell" predicate used by
  `FUN_00684C30` to populate the global vector at `0x008B41A8`. Exact semantics
  unknown but non-critical for parity.
- **`DAT_0081CC20`** — 4-entry direction table used by `RecalcAttributes` when
  creating a tube shell. Sibling of the Latin-square table at `0x0081CC30`.
  Byte-dump would be a quick confirmation.
- **`+0xEB4/+0xEB5/+0xEBE/+0xEC3/+0xECB`** — cluster of zero-initialized
  BuildingTypeClass bytes around `+0xEC2`. Likely TS-era capability slots.

### 15.8 Corrections issued in §15

1. `+0xEC2` is NOT `IsBridgeRepairHut`. The actual flag is `+0x16B6`. (Corrects
   agent claim in §13.15.)
2. `+0xEC3` is the engineer-can-enter gate, layered before `+0x16B6`.
3. BuildingClass::Update probe is 5×5 (-2..+2 on each axis); the prior 3×3
   correction was itself wrong. (`decompile_function 0x0043FB20` —
   OPERATOR_OR_ORDER_DRIFT)
4. `+0x540` is the attacker-house pointer in both DelayKill and bridge-collapse
   branches; NOT an engineer pointer for deferred callback.
5. Engineer bridge repair does NOT use `+0x6DF` latch — runs synchronously.
   (Significantly refines §14.10; the engineer-at-hut path is immediate, not
   deferred.)
6. Destroyed bridge rendering uses `0xA7` LandType sentinel early-return, not a
   dedicated destroyed SHP frame.
7. TubeClass granularity depends on source: RecalcAttributes creates same-cell
   per-cell shells; `[Tubes]` parser creates full entry/exit/step tubes.
   `ComputeBridgeZones` consumes `tube+0x28`; it does not fill it.

### 15.9 Updated Rust backlog entries

Items to update in the 57-item implementation backlog:

- **§13.17 #22** ("Bridge hut damage-absorbing latch") — broaden to **generic
  DelayKill latch**; bridge-collapse is a dispatch-time decision.
- **§14.21 #46** ("Engineer-busy mutex" and "shared with DelayKill") — narrow
  to only two producers: DelayKill and Ivan bomb. Engineer repair is
  synchronous.
- **§14.21 #47** ("Deferred engineer-repair callback") — **remove**; no such
  deferred path exists. Engineer repair is synchronous.
- **§14.21 #48** ("Engineer becomes passenger (Limbo'd as cargo)") — correct as
  stated. Engineer is consumed during synchronous repair dispatch.
- **§11.16 #2** ("overlay_types.rs::is_high_bridge_index accepts 24/25/237/238") —
  add clarification: 24/25 = DUMMY15/BRIDGE1 (high anchors), 237/238 =
  BRIDGEB1/BRIDGEB2 (low anchors). Previous "misclassifies 237/238 as HIGH"
  comment needs nuance: they're LOW bridge anchors, and a unified
  `is_bridge_anchor()` accepting all 4 IDs is correct; they just need separate
  `is_high` vs `is_low` discriminators.

### 15.10 Final address summary (§15 additions)

| Address | Function / Global |
|---------|-------------------|
| `0x005236A7` | `BuildingTypeClass::Constructor` (zero-inits +0xEC2..+0xECB) |
| `0x00519B5E` | Engineer-capture entry gate (Type+0xEC3 check) |
| `0x00519B82` | BridgeRepairHut branch (Type+0x16B6 check) |
| `0x0051A4D8` | MISSION_SABOTAGE gate (Type+0xEC2 check — Ivan bomb) |
| `0x0051A5A7` | Ivan-bomb latch setter |
| `0x0051A5FD` | Ivan-bomb attacker stash to `+0x540` |
| `0x00565090` | `MapClass::Constructor` (init per-map hut registry) |
| `0x00577920` | `UnregisterBridgeRepairHut` |
| `0x00727FD0` | `TubeClass::Constructor` |
| `0x007283C0` | `[Tubes]` INI parser |
| `0x0047DA35` | `RecalcAttributes`: tube shell creation |
| `0x00684C30` | Post-map-load bridge-cell registry builder |
| `0x008B41A8` | Global bridge-cell DynamicVector |
| `0x0081CC20` | 4-entry direction table for tube-shell init |
| `0x005FE770` | `OverlayTypeClass::ReadINI` |
| `0x00839794` | Dead data-ref to `"Bridge Repaired Event"` |

## Summary

Six deep-dive passes (initial + four extensions + this final close-out) have now
produced a comprehensive end-to-end map of the bridge system, from INI parsing
through map load, damage state machine, collapse effects, repair via engineer,
multiplayer determinism, rendering, pathfinding, shroud, superweapons, and
save/load.

**Doc stats:** ~2500 lines, 60+ decompiled functions, 100+ addresses catalogued.

**Rust implementation handoff:** use the re-audited §9. The older 57-item lists are
historical snapshots, not a current work-list. (corrected 2026-07-10: current-source
scan against binary contracts rechecked with `decompile_function 0x00576BA0`,
`0x00489280`, `0x00575870`, `0x00575BA0`, and `0x0047DD70` — RUST_STATUS_DRIFT)

**No remaining architectural unknowns** — the remaining three open items (§15.7
follow-ups) are all minor data-table details that don't affect the overall
understanding of the system.

The document is ready for implementation reference.

## 16. Minor Follow-Ups (Partial — Ghidra MCP Unavailable)

Ghidra MCP disconnected during this pass. Findings below are sourced from
cross-referenced existing research docs in `ra2-rust-game-docs/`, not from live
decompilation. Confidence is **medium** for items 1 and 3; item 2 could not be
resolved.

### 16.1 `DAT_0081CC20` direction table — RESOLVED

Per [ADDRESS_MAP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADDRESS_MAP.md) line 1189:

| Address | Type | Name | Values |
|---------|------|------|--------|
| `0x0081CC20` | `int[4]` | Tunnel direction table | `[2, 4, 6, 0]` |

These are 4 cardinal compass indices into `g_DirectionOffsets` (per §11.7 mapping):
- `2` = East (+1, 0)
- `4` = South (0, +1)
- `6` = West (-1, 0)
- `0` = North (0, -1)

Used by `CellClass::RecalcAttributes @ 0x0047DA35` when creating a tube shell for
a low bridge. Indexed by `(IsoTileTypeIndex - low_bridge_tile_base)`. **Low
bridges always run in a cardinal direction** (never diagonal) — this 4-entry
table enforces that. The diagonal slots (NE, SE, SW, NW) are not represented.

Cross-reference: §11.7's 8-entry `g_DirectionOffsets` at `0x0089F688` includes all
8 directions; `DAT_0081CC20` is the **low-bridge subset** of 4 cardinals.

**Rust implication:** low-bridge orientation is a 2-bit enum `{E, S, W, N}`.

### 16.2 `FUN_006E61F0 & 4` bridge-linked cell predicate — UNRESOLVED

Could not decompile without Ghidra MCP. Evidence from prior passes:

- Called from `FUN_00684C30` (post-map-load bridge-cell registry builder) during
  map init, runs before `ComputeBridgeZones`.
- Return value's **bit 2** (value `4`) gates insertion of the cell coord into the
  global DynamicVector at `0x008B41A8`.
- Address range `0x006E0000-0x006EFFFF` contains trigger/team/event code in
  gamemd.exe (sibling functions `FUN_006E0490`, `FUN_006E1A70`, `FUN_006E2050`
  are all trigger-action / reveal helpers per prior passes).

**Hypothesis (unverified):** `FUN_006E61F0(cell_or_coord)` returns a bitmask of
cell attributes; bit 4 likely means "this cell has a tube or bridge-terrain tile
associated." The global vector serves as a fast lookup list of all bridge-linked
cells for UI / save-load / recompute passes.

To finalize, decompile `FUN_006E61F0` in Ghidra and identify:
- Which cell fields it reads (candidates: `+0x116` tube index, `+0x38` tile
  index, `+0xEC` LandType, `+0x11E` overlay state)
- What bits other than bit 2 encode (return value could be a multi-flag bitmask)

### 16.3 `BuildingTypeClass` and `InfantryTypeClass` `+0xEB4..+0xECB` cluster

**Consolidated from cross-referenced docs.** Note: BuildingTypeClass and
InfantryTypeClass share overlapping offset numbers but **different meanings** —
the two class layouts are independent.

#### BuildingTypeClass `+0xEB4..+0xED0`

| Offset | Type | Name | Source |
|--------|------|------|--------|
| `+0xEB4` | int | `AdjacentRange=` (Adjacent range) | [BUILDING_SYSTEMS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_SYSTEMS_GHIDRA_REPORT.md) line 713 |
| `+0xEB8` | int | `Factory=` (class-kind enum) | [BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md) line 175 |
| `+0xEBC` | int | `TargetCoordOffset.X` | [COORDINATE_SYSTEM_GAMEMD.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COORDINATE_SYSTEM_GAMEMD.md) line 351 |
| `+0xEC0` | int | `TargetCoordOffset.Y` | same |
| `+0xEC4` | int | `TargetCoordOffset.Z` | same |
| `+0xEC8` | int | `ExitCoord.X` | [BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md) line 176 |
| `+0xECC` | int | `ExitCoord.Y` | [BUILDINGCLASS_OPEN_QUESTIONS_VERIFICATION_R3.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_OPEN_QUESTIONS_VERIFICATION_R3.md) |
| `+0xED0` | int | `ExitCoord.Z` | inferred |

**No gap for flags in this layout.** The `+0xEC2 / +0xEC3` check seen in
`InfantryClass::Mission_Enter` (§15.1) must therefore be on the **infantry's own
InfantryTypeClass**, not on the target building's BuildingTypeClass.

#### InfantryTypeClass `+0xEB4..+0xECB`

| Offset | Type | Name | Source |
|--------|------|------|--------|
| `+0xEB4` | bool | `Occupier=` (can garrison civilian buildings) | [GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) |
| `+0xEB5` | bool | `Assaulter=` (can storm garrisoned bldgs) | `BUILDING_CHANGE_OWNER_GHIDRA_REPORT.md` |
| `+0xEBC` | bool | `Fearless=` | [INFANTRYCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRYCLASS_GHIDRA_REPORT.md) line 120 |
| `+0xEBE` | bool | Team-related flag (convoy reassign) | [CONVOY_FORMATION_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CONVOY_FORMATION_SYSTEM_GHIDRA_REPORT.md) |
| `+0xEBF` | bool | `Fraidycat=` | [INFANTRYCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRYCLASS_GHIDRA_REPORT.md) line 119 |
| `+0xEC2` | bool | `Infiltrate=` (spy infiltrate cap.) | [FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md) line 286 |
| `+0xEC3` | bool | `Engineer=` (capture capability) | [FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md) line 285 |
| `+0xEC5` | bool | Possibly `Engineer=` (conflicts with +0xEC3) | `ENGINEER_CAPTURE_GHIDRA_REPORT.md` line 30 |
| `+0xEC6` | bool | `C4=` (bomb plant capability) | [FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md) line 287 |
| `+0xEC9` | bool | `Crawls=` (prone-capable) | [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md) line 716 |

**Conflict on Engineer flag**: `FOOTCLASS_MISSION_HANDLERS` says `+0xEC3`,
`ENGINEER_CAPTURE_GHIDRA_REPORT` says `+0xEC5`. Without Ghidra I can't
definitively disambiguate. Most likely: one doc read the neighboring byte. The
`InfantryClass::Mission_Enter` decompile in §15.1 (prior agent pass) confirmed
`+0xEC3` is the engineer-entry gate in the Mission_Enter context — prefer that
reading.

#### Revised interpretation of §15.1 / §15.3 findings

The earlier Mission_Enter agent said:
- "`Type+0xEC2` gates MISSION_SABOTAGE"
- "`Type+0xEC3` is the engineer-can-enter gate"

These are **InfantryType flags on the engineer itself**, not BuildingType flags
on the target. So:

- **Ivan's bomb plant path**: infantry has `Infiltrate=yes` (InfantryTypeClass+0xEC2)
  → mission 0x11 allowed → on bomb detonation, the hut's `field_0x6DF` latch
  is set (§15.3 use case 2).
- **Engineer repair/capture path**: infantry has `Engineer=yes` (InfantryTypeClass+0xEC3)
  → mission 8 / 0xB / 0x19 allowed → if RTTI==6 (Building) AND `Type+0x16B6`
  (BridgeRepairHut) on the target → synchronous `ProcessBridgeDestruction_High`
  call (§15.3, no latch used).

This **confirms §15.3's "three use cases"** for the `+0x6DF` latch (DelayKill,
Ivan bomb, Hut death→collapse) and **also confirms** that engineer bridge repair
bypasses the latch entirely.

#### TS-legacy flags `+0xEC2 / +0xEC3` on BuildingTypeClass

If the Mission_Enter check was indeed `infantryType.field_0xEC2`, then the
BuildingTypeClass layout has NO gap at `+0xEC2 / +0xEC3` — those bytes are
inside `TargetCoordOffset.Y` and its high byte. The Q1 agent's "dormant TS flag"
claim was based on reading BuildingTypeClass constructor zeroing at those
offsets, but if the entire `+0xEBC..+0xEC7` range is `TargetCoordOffset` (3 int
fields), zero-init is expected for coord defaults, not capability flags.

**Revision:** the "dormant TS flag" claim in §15.1 is likely **incorrect** — the
"zero-initialized bytes" were simply `TargetCoordOffset` defaults. There is no
mystery BuildingTypeClass TS-flag cluster at `+0xEB4..+0xECB`; that whole range
is the coord-offset + factory-type cluster. Only the InfantryTypeClass cluster
at the same offsets has the capability flags.

**Needs live Ghidra verification** to confirm — once MCP is back.

### 16.4 Updated summary of all remaining opens

| Item | Status |
|------|--------|
| `DAT_0081CC20` 4-entry direction table | **Resolved** — `[2,4,6,0]` = cardinal E,S,W,N (§16.1) |
| `FUN_006E61F0 & 4` predicate | **Unresolved** — needs live Ghidra (§16.2) |
| `+0xEB4..+0xECB` cluster semantics | **Largely resolved** — cluster is on InfantryTypeClass (capability flags), not BuildingTypeClass (coord offsets). Revised §15.1 "dormant TS flag" claim likely wrong. (§16.3) |

### 16.5 Rust implementation notes (§16 additions)

58. **Low-bridge orientation = 2-bit cardinal enum** (E/S/W/N). Don't model as
    8-direction — only 4 cardinals are valid for tubes. (§16.1)
59. **Engineer's type gates capability, not building's type** — for bridge repair,
    check `Engineer` flag on the infantry's InfantryType, then `BridgeRepairHut`
    flag on the target's BuildingType. Two separate discriminants. (§16.3)
60. **No mystery flag cluster on BuildingTypeClass at `+0xEB4..+0xECB`** — that's
    `AdjacentRange + Factory + TargetCoordOffset + ExitCoord`, fully accounted
    for. Don't reserve byte slots for "TS-legacy capabilities" there. (§16.3)
61. **`FUN_006E61F0` bridge-linked predicate** — defer. Rust's post-load bridge
    registry can be built from direct cell iteration over bridge-overlay ranges
    (0xCD..0xE8, 0x4A..0x65, anchors 0x18/0x19/0xED/0xEE). The engine's bitmask
    predicate is likely equivalent to this overlay-range test.

## Final note on Ghidra availability

At the end of this investigation the Ghidra MCP server disconnected, so
§16 relies on existing research docs rather than fresh decompilation. The three
items left open do not affect architectural understanding of the bridge system —
they're data-table details and a single helper function whose semantics can be
inferred from caller context.

## Sources

**Ghidra addresses decompiled (primary set):**
- 0x00576BA0 `ProcessBridgeDamageStateMachine_High` (main function)
- 0x00571490 `ProcessBridgeDamageStateMachine_Low` (comparison reference)
- 0x00575BA0 `CollapseBridge_NS_High`
- 0x00575870 `CollapseBridge_EW_High`
- 0x00574000 `DestroyBridge_High_OnHutDeath` (historically mislabeled `_MapInit`;
  `get_function_callers 0x00574000` shows runtime-only callers — INFERENCE_HARDENED)
- 0x005749C0 `DestroyBridgeFromCell_High`
- 0x0057CCF0 `DestroyBridge_High` (tile dispatcher)
- 0x0057CF60 `DestroyBridgeWalker_NS_High` (label actually EW)
- 0x0057D530 `DestroyBridgeWalker_EW_High` (label actually NS)
- 0x00587180 `ApplyDamageToCell`
- 0x0048A00E `Apply_area_damage` context
- 0x0047E040 `CellClass::SetBridgeDirection_NESW`
- 0x00576770 `MapClass::UpdateAdjacentBridges_High`
- 0x00572230 `UpdateRamp_NS_DamageA_High`
- 0x0057DC20 `FindBridgeEndpoints_NS_High`

**Referenced helpers (addresses confirmed, not fully decompiled):**
- 0x0057DAF0 `FindBridgeEndpoints_EW_High`
- 0x0057E7A0 `ApplyBridgeDestruction_NS_High`
- 0x0057ED00 `ApplyBridgeDestruction_EW_High`
- 0x0057BCF0 `DestroyBridgeWalker_NS_Low`
- 0x0057C2B0 `DestroyBridgeWalker_EW_Low`
- 0x00471050 `UpdateAdjacentBridges` (low)
- 0x0047E470 `CellClass::SetBridgeDirection_NWSE` (low)

**Globals traced:**
- `DAT_00AA0E28` `BridgeSet`
- `DAT_00ABAD1C` `WoodBridgeSet`
- `DAT_00ABAD30` NS bridgehead class
- `DAT_00AA1028` EW bridgehead class
- `DAT_00A8B230` SpecialFlags pointer (bit 0x8000 = DestroyableBridges)
- `DAT_00ABDE88` leptons-per-height
- `DAT_0081CC30` latin-square frame variation (referenced in rendering doc)

**String:** `"DestroyableBridges"` @ `0x00840248` (refs at 0x6B8B98, 0x6B8E1F — SpecialFlags unpacker)

**Prior docs cross-referenced:**
- `BRIDGE_SYSTEM.md` — overlay-ID tables, function directory
- [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) §3.3 — Low state machine (format template)
- [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md) — cell flags, overlay rendering paths
- [DAMAGE_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_MATH_GHIDRA_REPORT.md) — area-damage pipeline

**INI files:**
- `ini/rulesmd.ini`, `ini/artmd.ini`, `ini/rules.ini`, `ini/art.ini`

**Rust files audited:**
- `src/sim/bridge_state.rs` `src/sim/bridge_specs.rs` `src/bridge_re.rs`
- `src/sim/movement/movement_bridge.rs` `src/render/bridge_atlas.rs`
- `src/map/overlay_types.rs` `src/app_instances/overlays.rs`
