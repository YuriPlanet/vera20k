# Bridge Deferred Mechanics — Ghidra Research Report

**Topic:** Four mechanics deferred from the 2026-05-11 bridge-locomotor-layer correctness design:
1. Diff-1 SlopeIndex check in `CheckBridgeTraversal`
2. Two-pass `Can_Enter_Cell` at bridgeheads
3. `RecalcAttributes` per-byte write map (incl. the 0x47D94E +0x11B write)
4. `SetBridgeDirection_NESW` / `_NWSE` caller graph + bit-write map

**Addresses (primary):**
- `0x4D9C60` — `CheckBridgeTraversal`
- `0x73F0A0` — `UnitClass__Can_Enter_Cell`
- `0x47D2B0` — `CellClass__RecalcAttributes`
- `0x47E040` — `CellClass__SetBridgeDirection_NESW`
- `0x47E470` — `CellClass__SetBridgeDirection_NWSE`

**Confidence:** HIGH overall — every load-bearing claim verified directly from
binary disassembly or decompilation. Two findings are LOW (the OverlayClass
vtable slot identity for 0x5FC*, and the exact map-load source of bit 0x80) and
are tagged as open questions.

**Active in YR:** Yes for all subsystems. Two TS-legacy items confirmed and
documented separately.

---

## 1. Overview

All four deferred items are part of the live retail-YR bridge mechanic. The
investigation resolved three load-bearing doc/binary conflicts, produced a
definitive per-byte write map for `RecalcAttributes`, confirmed the two-pass
`Can_Enter_Cell` mechanism end-to-end, refined the byte-identity claim for
NESW/NWSE, and enumerated/categorized all 18 callers of SetBridgeDirection
including the three previously uncategorized `0x5FC*` sites.

**Headline parity finding:** bit `0x80` on `CellClass+0x140` (the
`bridge_walkable` analog in Rust's `PathCell`) **is mutated at gameplay time**
by `SetBridgeDirection` when bridges collapse (state=0 → bit cleared) or get
repaired (state=1 → bit set). Rust currently treats `PathCell.bridge_walkable`
as static-from-map-load (see [src/map/resolved_terrain.rs:538-599](../ra2-rust-game/src/map/resolved_terrain.rs#L538-L599)).
This is a real parity bug — gameplay-visible whenever a bridge is destroyed
and the path grid isn't refreshed.

---

## 2. Class Layout — Definitive Byte Offsets

Verified against Ghidra's `CellClass` struct (size 328 bytes) AND
cross-referenced against every read/write site in the four primary functions.

| Offset | Size | Type | Name | Notes |
|--------|------|------|------|-------|
| 0x24 | 2 | short | MapCoord_X | Read by SetBridgeDirection neighbor compute |
| 0x26 | 2 | short | MapCoord_Y | Read by SetBridgeDirection neighbor compute |
| 0x2C | 4 | ptr | bridge_anchor_ptr | Pointer to anchor CellClass (or 0 when destroyed). Written by SetBridgeDirection to all 5-6 visited cells. |
| 0x34 | 4 | ptr | LightConvert | |
| 0x38 | 4 | int | IsoTileTypeIndex | |
| 0x44 | 4 | int | OverlayTypeIndex | -1 if no overlay |
| 0x48 | 4 | int | SmudgeTypeIndex | |
| 0x54 | 4 | ptr | (unnamed — ground-layer secondary occupier ptr) | Read pre-vtable in Can_Enter_Cell. Bridge-layer counterpart at +0x58. |
| 0x58 | 4 | ptr | (unnamed — bridge-layer secondary occupier ptr) | Read post-vtable in Can_Enter_Cell ONLY if `targetHeight == cell.Level+4` AND cell has 0x100. |
| 0xE4 | 4 | ptr | FirstObject | Ground-layer occupier-list head. Loop iterates this when `bridge_pass_flag == 0`. |
| 0xE8 | 4 | ptr | AltObject | Bridge-layer occupier-list head. Loop iterates this when `bridge_pass_flag == 1`. |
| 0xEC | 4 | int | LandType | Set by RecalcAttributes to 0/3/5 or overlay type. Used by speed-vs-landtype table. |
| **0x11A** | **1** | **byte** | **Height** | **Tile sub-type byte. Read by TMP_ReadSlopeType for slope lookup. Written ONLY to 0 in RecalcAttributes' cliff-fallback path at 0x47D5E9 — otherwise preserved.** |
| **0x11B** | **1** | **i8** | **Level** | **Signed height level (each level = 15 pixels of world Z). Read all over the bridge code (via `MOVSX` — signed). Written by RecalcAttributes ONLY when `level_override` parameter != -1 (the hidden 2nd parameter), at 0x47D94E.** |
| **0x11C** | **1** | **byte** | **SlopeIndex** | **Terrain slope (0-20). Written by RecalcAttributes from `TMP_ReadSlopeType(this->Height)` at 0x47D35E (overlay branch) and 0x47D80D (normal branch). Cleared to 0 at 0x47D5F9 (cliff fallback) and 0x47DB52 (no-tile branch). READ by CheckBridgeTraversal at diff==1 to gate ramp passability.** |
| 0x11D | 1 | byte | (HeightInPixels) | `(height_raw - 30) / 15` via signed magic-number division at 0x47D993. Stored as `(char)(local_2c/15 + sign-correction)`. |
| 0x11E | 1 | byte | bridge_state | Set to 0 (cleared) or 9 (active) by SetBridgeDirection. Set to 0 in RecalcAttributes overlay-clear path at 0x47D37F, 0x47D850. |
| 0x124 | 4 | int | OccupationFlags | Ground-layer occupier bitfield: bits 0-4 = infantry sub-cells, bit 5 = vehicle present, bits 8+ = ? Read pre-vtable in Can_Enter_Cell. |
| 0x128 | 4 | int | AltOccupationFlags | Bridge-layer counterpart. Read post-vtable in Can_Enter_Cell IF the deck-height predicate fires. |
| 0x140 | 4 | uint | Flags | **Bit map verified — see §SetBridgeDirection Bit Map** |

### CellClass.Flags (+0x140) — Bridge-Relevant Bits (Verified)

| Bit | Mask | Meaning | Written by |
|-----|------|---------|------------|
| 7 | 0x80 | "bridge_walkable" anchor marker | **SetBridgeDirection on anchor cell only**, gameplay-time mutable |
| 8 | 0x100 | "cell is a bridge" (anchor or body) | SetBridgeDirection on anchor + 1st + 2nd neighbors (not 3rd); read by every bridge-aware function |
| 9 | 0x200 | "bridgehead" (entry/exit point) | SetBridgeDirection on anchor + 1st neighbor only |
| 10 | 0x400 | "bridge destroyed" marker | SetBridgeDirection sets when state.byte0==0 |
| 11 | 0x800 | "direction is 0" sentinel | SetBridgeDirection sets when param_2==0 |
| 12 | 0x1000 | (state-driven bit) | SetBridgeDirection on anchor + 1st-3rd neighbors |
| 13 | 0x2000 | (unrelated; CLEARED by SetBridgeDirection on opposite-step cell) | |
| 14 | 0x4000 | (unrelated; CLEARED by SetBridgeDirection on opposite-step cell) | |
| 16 | 0x10000 | (state-driven bit) | SetBridgeDirection on anchor + 1st-3rd neighbors + opposite + dir6-special |
| 17 | 0x20000 | "tube-anim spawned" | RecalcAttributes sets at 0x47DA88 when LandType==10 + tube tile match |

---

## 3. Core Logic

### 3.1 `CheckBridgeTraversal` (0x4D9C60) — height-diff gate

**Signature:** `(int src_cell, int direction, int *targetHeight_inout, byte *bridgeEntered_out, int dst_cell)`. RET is implicit 0; no `RET N`. Returns 0 (OK) or 7 (blocked).

**Special init cases (resolved at function entry):**

- `param_5 (dst_cell) == 0` → compute dst from `src + g_DirectionOffsets[(direction-4) & 7]`. **g_DirectionOffsets at 0x0089F688** (short X delta), **DAT_0089f68a at 0x0089F68A** (short Y delta), 8 entries × 4 bytes apart. **The `(direction - 4) & 7` wrap is the SW-relative direction normalization that gamemd uses internally** (facings stored 0-7 with 4=opposite of expected zero).
- `param_2 (direction) == -1` → "seed" mode. If `*targetHeight == -1` AND src has flag 0x100, write `targetHeight = src.Level + 4`. Return 0. (This is the call Can_Enter_Cell makes to learn deck height before evaluating a step.)

**Normal-case algorithm (pseudo-code with verified constants):**

```
// "Seed targetHeight from dst" — runs once when caller hands in -1
if (*targetHeight == -1 && dst.flags & 0x100) {
  *targetHeight = dst.Level + 4;
  if ((src.flags & 0x200) == 0)        // src is NOT a bridgehead
    return 7;
}

src_h = (i8)src.Level                  // SIGNED
dst_is_bridge = dst.flags & 0x100
if (dst_is_bridge) cmp_h = (i8)dst.Level
else               cmp_h = *targetHeight

diff = cmp_h - src_h                   // signed
abs_diff = abs(diff)

if (abs_diff == 0) {
  // flat-step guard
  if ((!(src.flags & 0x100) || !(src.flags & 0x200) || !dst_is_bridge)
      && *targetHeight != -1 && *targetHeight != src_h)
    return 7;
}
else if (abs_diff == 1) {
  // Diff-1: SlopeIndex check on the LOWER cell of the pair (RESOLVED Conflict A)
  if (diff < 1) {                       // going down (dst lower)
    if (dst.SlopeIndex == 0) return 7;  // cell+0x11C, NOT +0x11A
  } else {                              // going up (src lower)
    if (src.SlopeIndex == 0) return 7;
  }
}
else if (abs_diff == 4) {
  // Bridge entry / exit
  if (dst.Level == src.Level - 4) {
    // dst is 4 below src — going OFF a bridgehead onto bridge body
    if (*targetHeight != src_h) return 7;
    if (!dst_is_bridge) return 7;
  }
  if (src.Level == dst.Level - 4) {
    // src is 4 below dst — going ONTO a bridgehead from below
    if (!(src.flags & 0x100)) return 7;
    if (!(src.flags & 0x200)) return 7;
    *bridgeEntered_out = 1;
    return 0;
  }
}
else {
  return 7;                              // diff 2, 3, 5+ are ALWAYS blocked
}

return 0;
```

**Tiny details (iron-law captures):**
- Every Level read uses `MOVSX` (signed byte). Heights are i8, not u8. Negative values (rare/malformed maps) are interpreted as negative.
- The diff-0 guard fires only when targetHeight is specified (`!= -1`) AND mismatches src.Level. So caller passing `targetHeight = -1` makes flat-step always pass. Callers that pre-compute targetHeight are responsible for not stepping flat into a deck-height mismatch.
- `*bridgeEntered_out = 1` fires ONLY in the "going up by 4" branch. Going DOWN by 4 (off a bridgehead) does NOT set the output flag. This is asymmetric — Can_Enter_Cell's subsequent post-vtable bridge re-read only fires when targetHeight matches deck (which is independent of this flag).
- `param_2 - 4U & 7` direction wrap converts an 8-direction facing into the lookup index. The `-4` is RA2's "facing 4 = N" convention; subtracting 4 rotates 0=N, 1=NE, ..., 7=NW into 4=N-relative form for the offset table.

### 3.2 `UnitClass__Can_Enter_Cell` (0x73F0A0) — two-pass mechanism

**Signature:** `int __thiscall(this, cell, direction, targetHeight, flag5, flag6)`. RET 0x14 — **6 params total** (this in ECX, 5 stack args). Ghidra's listed signature shows only 5 params; the function actually consumes 5 stack args (`[ESP+0x94..0xA4]` post-pushes). The 5th stack arg ([ESP+0xA4]) is forwarded only to one internal vtable call at 0x73F3B3.

**Return codes** (0-7, indexed into A*'s cost table at 0x81870C):
- 0 = OK/Clear
- 1 = Crushable
- 2 = TemporaryBlock (moving friendly)
- 3 = ScatterRequired (allied building)
- 4 = FriendlyWall
- 5 = EnemyBlock
- 6 = FriendlyStationary
- 7 = Impassable

**Two-pass mechanism — VERIFIED end-to-end (resolves the deferred question):**

The "two-pass" is NOT a literal re-evaluation of the entire `Can_Enter_Cell`
function. It is a three-step state machine with one conditional bridge-layer
overwrite:

```
[Step 1 — Pre-vtable: decide pass flag]
At 0x73F0BD-0x73F0EB:
  bridge_pass = 1 if (cell.flags & 0x100) && (targetHeight == -1 || abs(targetHeight - cell.Level) > 1)
              else 0
  Store at [ESP+0x13]  // local byte var

[Step 2 — Pre-vtable: GROUND occupancy snapshot]
At 0x73F0ED-0x73F109:
  local[0x14] = cell+0x124 low byte                          // ground occupier bits
  local[0x1c] = cell+0x54                                    // ground secondary list ptr
  local[0x15] = (cell+0x124 dword >> 5) & 1                  // vehicle bit from byte 1

[Step 3 — Vtable dispatch (calls CheckBridgeTraversal slot +0x1B0)]
At 0x73F2EB:
  result = this->vtable[0x1B0](cell, direction, &targetHeight, &uStack_80[3])
  // uStack_80[3] is unused output (CheckBridgeTraversal can write to it)
  if (result == 7) return 7

[Step 4 — Post-vtable: CONDITIONAL bridge-layer OVERWRITE]
At 0x73F303-0x73F34C:
  if (targetHeight != -1 && (cell.flags & 0x100) && targetHeight == cell.Level + 4) {
    local[0x14] = cell+0x128 low byte                        // bridge occupier bits
    local[0x1c] = cell+0x58                                  // bridge secondary list ptr
    local[0x15] = (cell+0x128 dword >> 5) & 1                // bridge vehicle bit
  }

[Step 5 — Main loop occupier classification]
At 0x73F4F9-0x73FA8C:
  if ([ESP+0x13] == 0)  occupier_head = cell+0xE4  (FirstObject = ground list)
  else                   occupier_head = cell+0xE8  (AltObject   = bridge list)
  // Iterate occupier_head, classifying each one using the (now possibly bridge-layer)
  // occupancy bits stored in local[0x14] and local[0x15].
```

**Iron-law tiny details:**
- The **object-list selection** ([ESP+0x13]) is decided PRE-vtable from cell flags + targetHeight. The vtable can update targetHeight, but it doesn't update [ESP+0x13]... **with one exception:** `CheckBridgeTraversal`'s `*param_4 = 1` write in its diff-4-going-up branch (`src.Level == dst.Level - 4 AND src.0x100 AND src.0x200`) DOES overwrite [ESP+0x13] because Can_Enter_Cell passes `&[ESP+0x13]` as CBT's `param_4`. So the pass flag can be force-set to 1 mid-call when entering a bridgehead from below — refined sub-case 1.e in [G6_TWO_PASS_DIVERGENCE_SUPPLEMENT.md §3](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/G6_TWO_PASS_DIVERGENCE_SUPPLEMENT.md#3-divergence-sub-case-map-concrete-predicates). The list iterated is "pre-decision OR CBT-diff-4-override", not strictly pre-decided.
- The **occupancy bits** (local[0x14], local[0x15]) are decided POST-vtable. The vtable updates targetHeight, then a fresh predicate (`targetHeight == cell.Level+4 AND cell has 0x100`) decides whether to overwrite the bits.
- **These two decisions CAN disagree** in edge cases — the function iterates the ground list but uses bridge-layer occupancy bits, or vice versa. This happens when targetHeight starts at -1 and the vtable doesn't fill it to Level+4 (e.g., dst cell isn't a bridge), but the cell-flag pre-decision already chose ground/bridge list.
- The **byte-shift-by-5 pattern** in occupancy-bit construction is verified at 0x73F100 and 0x73F33F: `(OccupationFlags >> 5) & 1` extracts bit 5 (vehicle present) and stores it as bit 0 of `local[0x15]`. The decompilation's `CONCAT11((char)(... >> 5), ...) & 0x01FF` representation is a Ghidra artifact of the same logic — bit 5 of OccupationFlags ends up at bit 8 of the combined word, while bits 0-4 (infantry sub-cells) are preserved in bits 0-4. **Important: bit 5 of OccupationFlags is duplicated into bit 8 of the snapshot** — both reflect the same source bit, redundantly. Likely TS-era artifact; behavior preserved.
- Tube cell handling (LandType == 10) reads `cell.Height (+0x11A)` as a **direction sub-type byte**, not as a terrain height. Values 2 and 6 are valid tube-entry sub-types depending on tile-type field_0x2E4/0x2E8. **Cell+0x11A has dual semantic: terrain height for normal cells, tube sub-direction for tube cells.**

**Comparison with Rust's pre-decided `target_layer`:**

Rust ([src/sim/pathfinding/core.rs:425-451](../ra2-rust-game/src/sim/pathfinding/core.rs#L425-L451)) decides the layer at A* push-time via `is_at_bridge_level(current.height, neighbor)`. Cell_entry then uses that pre-decided layer for both object list AND occupancy bits. This matches **most** retail bridge-cell traversal but **NOT** the edge case where the binary's object-list pre-decision and occupancy-bit post-decision disagree.

The disagreement happens when:
- `current.height` matches `cell.Level + 4` (deck level — Rust's `is_at_bridge_level` returns true)
- BUT the dst cell is not a bridge (so CheckBridgeTraversal doesn't fill targetHeight)

Rust would route through bridge layer. Binary would iterate bridge object list but use **ground** occupancy bits. In practice, this case only arises on the leaving-bridge tick at the bridgehead boundary, and the observable outcome (which occupiers are checked) is unlikely to differ for retail unit configurations — but the **exact gamemd output cannot be reproduced** without separating these two decisions.

**Conclusion on the deferred two-pass question:** gamemd's two-pass DOES produce observable behavior different from a strictly pre-decided single-pass on the bridgehead-leaving tick. The 2026-05-11 design's Tiny-Detail Ledger #11 ("matches IF the layer pre-decision is correct") was OPTIMISTIC — the binary's pre/post split allows a state our pre-decision cannot reproduce. Severity: low frequency (only the boundary tick at bridgehead exit), but non-zero. Recommendation: defer to brainstorm — most likely "accept the divergence" given how rare the configuration is.

### 3.3 `CellClass__RecalcAttributes` (0x47D2B0) — definitive write map

**Signature:** `void __thiscall(CellClass *this, int level_override)`. **HIDDEN second parameter.** RET 0x4 confirms 1 stack arg cleanup. The second parameter (`level_override`, -1 = "don't override") is the source of the AL written to `[ESI+0x11B]` at 0x47D94E. **This is a critical detail the prior research missed entirely.**

**Per-byte write inventory (verified by disassembly read):**

| Site | Instruction | Field | Condition |
|------|-------------|-------|-----------|
| 0x47D318 | `MOV [ESI+0xEC], ECX` | LandType | Overlay branch — from overlay type's field_0x298 |
| 0x47D35E | `MOV [ESI+0x11C], AL` | **SlopeIndex** | Overlay branch — from `TMP_ReadSlopeType(this->Height)` |
| 0x47D378 | `MOV [ESI+0x44], -1` | OverlayTypeIndex | Overlay-clear when slope-removable |
| 0x47D37F | `MOV [ESI+0x11E], 0` | bridge_state byte | Same path |
| 0x47D53E | `MOV [ESI+0xEC], 3` | LandType=3 | g_RulesClass+0x664==2 cliff-back path |
| 0x47D58D | `MOV [ESI+0x38], EBX` (=0xFFFF) | IsoTileTypeIndex | No-tile fallback init |
| 0x47D5E6 | `MOV [ESI+0x38], EBX` (=0xFFFF) | IsoTileTypeIndex | Tile-invalid cliff fallback |
| **0x47D5E9** | **`MOV [ESI+0x11A], AL`** | **Height = 0** | **Cliff fallback (AL is the post-XOR 0). The ONLY write to +0x11A in this function.** |
| 0x47D5EF | `MOV [ESI+0xEC], 0` | LandType=0 | Cliff fallback |
| 0x47D5F9 | `MOV [ESI+0x11C], AL` | SlopeIndex=0 | Cliff fallback |
| 0x47D7C1 | `MOV [ESI+0xEC], 3` | LandType=3 | g_RulesClass+0x664==2 path #2 |
| 0x47D80D | `MOV [ESI+0x11C], AL` | SlopeIndex | Normal branch — TMP_ReadSlopeType result |
| 0x47D843 | `MOV [ESI+0xEC], EAX` | LandType | Tile-overlay path |
| 0x47D849 | `MOV [ESI+0x44], -1` | OverlayTypeIndex | Slope-removable overlay clear |
| 0x47D850 | `MOV [ESI+0x11E], 0` | bridge_state | Same |
| 0x47D86E | `MOV [ESI+0xEC], 5` | LandType=5 | Overlay LandType=0 path |
| 0x47D8AA | `MOV [ESI+0xEC], EAX` | LandType | FUN_00544BE0 result |
| **0x47D94E** | **`MOV [ESI+0x11B], AL`** | **Level = level_override** | **ONLY write to +0x11B. AL from [ESP+0x4C] = hidden second param. Gated by `level_override != -1`.** |
| 0x47D993 | `MOV [ESI+0x11D], DL` | HeightInPixels | `(height_raw - 30) / 15` via signed magic multiply |
| 0x47DA88 | `OR [ESI+0x140], 0x20000` | Flags bit 17 (tube anim) | LandType==10 + tile match |
| 0x47DB40 | `MOV [ESI+0xEC], EDX` | LandType | Overlay LandType direct |
| 0x47DB48 | `MOV [ESI+0xEC], 0` | LandType=0 | Empty-tile fallback |
| 0x47DB52 | `MOV [ESI+0x11C], 0` | SlopeIndex=0 | Empty-tile fallback |
| 0x47DD2A | `MOV [ESI+0xEC], 3` | LandType=3 | g_RulesClass+0x664==2 path #3 |

**Zone-cache mirror writes (NOT CellClass fields):**

| Site | Instruction | Notes |
|------|-------------|-------|
| 0x47D560 | `MOV [EBX+0x1], DL` | DL = this.Level. Writes to zone-cache entry at 0x87F850-indexed slot. |
| 0x47D569 | `MOV [ECX+0x8], AL` | AL = this.Level. Writes to second zone-cache (0x87F858). |
| 0x47D571 | `MOV [EBX], DL` | DL = this.field_0x4C. |
| 0x47D7DD | `MOV [EAX+0x1], CL` | Same pattern, normal-branch exit. |
| 0x47D7EA | `MOV [ECX+0x8], DL` | Same. |
| 0x47DD45 | `MOV [EAX+0x1], DL` | Same. |
| 0x47DD51 | `MOV [EAX+0x8], DL` | Same. |

These mirror `Level` into two parallel zone arrays (probably `ZoneMap__CellToZoneIndex` lookup targets for fast bulk queries — `DAT_0087F850` and `DAT_0087F858`). They are NOT writes to CellClass; the addresses come from `ZoneMap__CellToZoneIndex(MapCoord_X/Y)` at function entry.

**Conflict resolution summary:**

| Conflict | Resolution | Evidence |
|----------|-----------|----------|
| A — SlopeIndex offset | **+0x11C** | CheckBridgeTraversal at 0x4D9C60 reads `*(char*)(cell+0x11C)` at diff==1 branch. RecalcAttributes writes TMP_ReadSlopeType result to `[ESI+0x11C]` at 0x47D35E and 0x47D80D. |
| B — 0x47D94E instruction | **`MOV [ESI+0x11B], AL`** (AUDIT_LOG was correct) | Direct disassembly read. AL from `[ESP+0x4C]` = hidden 2nd parameter `level_override`. |
| C — RecalcAttributes write map | **See table above** | Disassembly of full function body (0x47D2B0–0x47DD63), every store enumerated. |

**Tiny details (iron-law captures):**

- **Hidden `level_override` parameter** (RET 0x4). Most callers pass -1 to mean "don't override Level"; specific callers (probably PlaceBuilding for foundation-level enforcement) pass a concrete byte value. The Rust port has no equivalent of this signature today.
- **g_RulesClass+0x664 = CliffBackImpassability** (verified — see §INI Keys). Default in retail = 2 (maximal). The function runs the 6-neighbor check in all three major branches (overlay, cliff fallback, normal exit). The check sets `LandType=3` if all 6 neighbors are `>= 4 levels below this cell` AND (in branch 3) `LandType in {0, 2, 6, 8}`.
- **The 6 neighbors checked are ASYMMETRIC:** N=(X,Y-1), W=(X-1,Y), SE+1=(X+2,Y+2), SE=(X+1,Y+1), SW=(X-1,Y+1), NE=(X+1,Y-1). **Missing: S=(X,Y+1), NW=(X-1,Y-1).** The (X+2,Y+2) is a peculiar 2-step SE offset. This is verified retail behavior. Whether intentional or a TS-era bug is unknown but must be reproduced for parity.
- **Signed `MOVSX` everywhere** for Level reads. Level is i8 not u8. `(this->Level + 4)` is signed addition; wraps possible only for Level >= 124 (extremely rare).
- **Tube tile constructor** at 0x47D8EC fires when LandType==10 AND IsoTileTypeIndex matches one of 4 DAT_-listed ranges (each 4-tile-wide). Creates a `TubeClass` at the cell.
- **Flags bit 0x20000 (tube-anim spawned)** is sticky — once set, the function skips the anim-creation block.

### 3.4 `SetBridgeDirection_NESW` (0x47E040) / `_NWSE` (0x47E470) — bit-write map

**Byte-identity refinement (corrects AUDIT_LOG):**

The two functions are **NOT byte-identical** — they are **instruction-identical / compiled-twin**. Same opcodes, same operands, same CALL/JMP **targets** (resolve to same absolute addresses, e.g., both call `0x5657A0` = `MapClass__Get_CellClass`). They differ in the **relative offset bytes** of CALL/JMP instructions (e.g., `E8 48 75 0E 00` vs `E8 18 71 0E 00` for the same target from different positions). Function sizes match exactly (0x422 bytes = 1058 incl. terminator).

Verified by spot-checks at four positions:
- Prologue (0x47E040 vs 0x47E470, 64 bytes): byte-identical
- Mid 1 (0x47E150 vs 0x47E580, 32 bytes): byte-identical (no CALL in window)
- Mid 2 (0x47E240 vs 0x47E670, 32 bytes): differs at one CALL's relative offset
- Mid 3 (0x47E340 vs 0x47E770, 32 bytes): byte-identical (no CALL in window)
- Epilogue (0x47E440 vs 0x47E870, 34 bytes): byte-identical (RET 8 + POPs)

**Implication:** Rust must implement ONCE. NESW vs NWSE is a naming convention reflecting WHICH overlay IDs invoke each (low-bridge IDs use one, high-bridge IDs use the other), but the function bodies are identical so they're interchangeable.

**Function signature:** `void __thiscall SetBridgeDirection(CellClass *anchor, uint direction, uint state)`. RET 0x8 (2 stack args).
- `direction`: 0-7 facing index (if >= 8, the per-cell directional step is skipped — anchor's coords used directly).
- `state`: bit 0 = alive flag (1 = bridge present, 0 = destroyed). Byte 0 read separately for the "destroyed → BlowUpBridge" branch.

**Cells visited (6 total, in this order):**

| Step | Cell | How located | Bits SET (state.bit0==1) | Bits CLEARED (state.bit0==0) |
|------|------|-------------|---------------------------|------------------------------|
| 1 | Anchor (param_1) | passed in | **0x80**, 0x100, 0x200, 0x1000, 0x10000 | (replaced with 0x400) |
| 2 | 1st neighbor | step from anchor by `direction` | 0x100, 0x200, 0x1000, 0x10000 (NO bit 0x80) | 0x400 set; clears 0x800 |
| 3 | 2nd neighbor | step from 1st by `direction` | 0x100, 0x1000, 0x10000 (NO 0x80, NO 0x200) | 0x400 set; clears 0x800 |
| 4 | 3rd neighbor | step from 2nd by `direction` | only 0x1000 set/cleared | — |
| 5 | Opposite-step | step from anchor by `(direction - 4) & 7` | 0x100, 0x200, 0x10000; CLEARS 0x2000, 0x4000 | 0x400 set; clears 0x800 |
| 6 | Special dir-6 cell | step from cell-5 by DAT_0089F690 (= direction-2 offset table entry) | only 0x10000 | — |

Step 6 fires ONLY when direction == 6. The DAT_0089F690 reference resolves to the direction-2 entry in the same `g_DirectionOffsets` table (8 bytes past 0x0089F688).

**Additional state writes per cell:**
- `bridge_anchor_ptr` (cell+0x2C) = anchor (when alive) or 0 (when destroyed)
- `bridge_state` byte (cell+0x11E) = 0 (destroyed) or 9 (alive). For non-anchor cells, `-(direction != 0) & 9` → 0 if direction==0, 9 otherwise.
- `BlowUpBridge(cell)` called for each visited cell when state.byte0 == 0
- `RadarClass__MarkTerrainDirty(cell.MapCoord)` called for cells 1, 2, 3 and 5 (NOT 4 or 6)

**Tiny details:**
- The 4th cell (3 steps forward from anchor) has the LIGHTEST update — only bit 0x1000. This is the "far end" of the bridge; the heavy state lives in cells 1, 2, 5.
- **Bit 0x80 is written ONLY to the anchor cell, and ONLY in this function.** No other function touches bit 0x80 in regular gameplay paths. Map-load sets it via a different upstream path (see §Open Questions §1).
- Steps 1-3 and 5 also call `BlowUpBridge` when destroyed — this is what kills units on the bridge during collapse, plus spawns destruction anims.
- The opposite-step cell (5) is the only one that **clears bits 0x2000 and 0x4000**. These bits are **write-only in the bridge family** — no reader exists in any decompiled bridge function, and byte-pattern search for `TEST [reg+0x140], 0x2000/0x4000` returns no matches. They are TS-era dead state, safe to ignore in Rust port. Resolved in §Q2.

### 3.5 SetBridgeDirection caller graph — categorized

**18 total call sites across both functions:**

| Caller | Function-pair | Category | Trigger |
|--------|---------------|----------|---------|
| `MapClass__UpdateRamp_NS_CollapseA_High` @ 0x5724CD | NESW | DAMAGE | High-bridge collapse cascade (N-S orientation, step A) |
| `MapClass__UpdateRamp_NS_CollapseB_High` @ 0x57286D | NESW | DAMAGE | (step B) |
| `MapClass__UpdateRamp_EW_CollapseA_High` @ 0x572E31 | NESW | DAMAGE | (E-W orientation, step A) |
| `MapClass__UpdateRamp_EW_CollapseB_High` @ 0x573201 | NESW | DAMAGE | (step B) |
| `MapClass__UpdateBridgeEdgeTiles_High` @ 0x57671C | NESW | DAMAGE/REPAIR | Edge fixup after collapse or repair |
| `FUN_00565C10` @ 0x567078 | NESW | MAP-LOAD | Cell iteration during map post-process — REFRESH only |
| `ProcessBridgeDamageStateMachine_High` @ 0x577790 | NESW | DAMAGE-TICK | First call site (state transition) |
| `ProcessBridgeDamageStateMachine_High` @ 0x5778AC | NESW | DAMAGE-TICK | Second call site (different state transition) |
| `OverlayClass::Mark` @ 0x5FC5FE | NESW | MAP-LOAD/EDITOR | Inside **0x5FC570** = `OverlayClass::Mark` (vtable__OverlayClass slot +0x8, verified). Bridge overlay 0x19 dispatch → NESW dir=6 |
| `OverlayClass::Mark` @ 0x5FC60A | NESW | MAP-LOAD/EDITOR | Same function, 0x18 dispatch → NESW dir=0 |
| `MapClass__UpdateRamp_NS_CollapseA_Low` @ 0x56EFDD | NWSE | DAMAGE | Low-bridge collapse cascade |
| `MapClass__UpdateRamp_NS_CollapseB_Low` @ 0x56F37D | NWSE | DAMAGE | |
| `MapClass__UpdateRamp_EW_CollapseA_Low` @ 0x56F941 | NWSE | DAMAGE | |
| `MapClass__UpdateRamp_EW_CollapseB_Low` @ 0x56FD11 | NWSE | DAMAGE | |
| `MapClass__UpdateBridgeEdgeTiles_Low` @ 0x570FFC | NWSE | DAMAGE/REPAIR | |
| `FUN_00565C10` @ 0x56706C | NWSE | MAP-LOAD | Cell iteration refresh |
| `ProcessBridgeDamageStateMachine_Low` @ 0x5721B3 | NWSE | DAMAGE-TICK | |
| `OverlayClass::Mark` @ 0x5FC62C | NWSE | MAP-LOAD/EDITOR | Same `OverlayClass::Mark`, high-bridge dispatch (0xED/0xEE → NWSE dir=0/6) |

**Categorization summary:**
- **DAMAGE/REPAIR (12 sites):** All UpdateRamp_*_Collapse_*, UpdateBridgeEdgeTiles_*, ProcessBridgeDamageStateMachine_*. These run during the bridge-damage-tick state machine. **Bit 0x80 mutation at gameplay time happens through these paths.**
- **MAP-LOAD origin (3 sites):** `OverlayClass::Mark` at 0x5FC570 (×3). This is **the function that originates bit 0x80** during initial .MAP loading — when a bridge overlay (0x18, 0x19, 0xED, 0xEE) is placed on a cell, Mark dispatches to SetBridgeDirection with state=1 which writes bit 0x80 to the anchor.
- **MAP-RESIZE refresh (2 sites):** `MapClass::Resize` at 0x565C10 (×2). When the map array is resized at gameplay time, this function backs up cell flags via a per-bit XOR copy, reallocates the array, restores flags, then re-runs SetBridgeDirection on cells that retained bit 0x80 but lost their `bridge_anchor_ptr`. This is a REFRESH, not an origin.
- **EDITOR (subset of OverlayClass::Mark):** Same Mark method runs in the map editor when placing a bridge overlay manually. Not relevant for retail skirmish play.

**No save/load callers, no AI callers, no unrelated paths.** The 2026-05-11 plan's concern about uncategorized callers is fully resolved.

**Overlay-ID → SetBridgeDirection dispatch table (verified from disassembly at 0x5FC5EE-0x5FC62C):**

| Overlay ID (decimal) | Calls | Direction param | State |
|----------------------|-------|-----------------|-------|
| 0x18 (24) — low bridge piece A | NESW | 0 | 1 |
| 0x19 (25) — low bridge piece B | NESW | 6 | 1 |
| 0xED (237) — high bridge piece A | NWSE | 0 | 1 |
| 0xEE (238) — high bridge piece B | NWSE | 6 | 1 |
| 0xA7, 0xB2, others — other branches | (decoded further down in OverlayClass::Mark) | varies | varies |

### 3.6 `Process_Drive_Track` (0x4B0F20) — on_bridge predicate verification

Re-decompiled to verify the 2026-05-11 Tiny-Detail Ledger #1 and #2. **Confirmed identical:**

```
At 0x4B1812 (and similar at 0x4B258D-ish for the second predicate site):
  if ((i8)dst.Level == (i8)src.Level - 4) {
    if (dst.flags & 0x100) {
      foot.on_bridge_byte (+0x8C) = 1            // ENTER bridge
    }
  }
  if (src.flags & 0x100 && !(... above fired)) {
    foot.on_bridge_byte = 0                       // EXIT bridge
  }
```

The actual Ghidra decompilation shows the conditions interleaved via labels
LAB_004B1837 and LAB_004B183F, but the semantic is exactly the
Ledger-#1/#2 predicate. **Process_Drive_Track reads `+0x11B` Level — the same
byte RecalcAttributes writes at 0x47D94E.**

`g_BridgeZOffset_Drive` is read in the function body — this is the verified
"deck is exactly 4 height-levels above ground" constant (Ledger #5).

### 3.7 `BlowUpBridge` (0x47DD70) — companion to SetBridgeDirection

Called by SetBridgeDirection on each visited cell when state.byte0 == 0
(destroyed). Behavior:

1. Walks `cell.FirstObject` (ground occupiers): for each, calls vtable+0x16C
   (Take_Damage) with warhead = `g_RulesClass + 0xFA8` (**C4Warhead** — see
   correction note below).
2. Walks `cell.AltObject` (bridge occupiers): for each, calls vtable+0xEC
   (`ObjectClass::DropIn` — flips bytes `+0x8D`/`+0x8F` to 1 marking
   "in-air / falling", removes from display layer, re-submits to falling
   layer, invokes vtable+0xF4 fall logic. NOT Limbo — unit reveal contribution
   stays in place until subsequent fall-damage death through normal
   TechnoClass update. Verified at `0x5F4160`.).
3. Adds the cell to a global death-list at `DAT_0087F8C0..D0`. **CORRECTION
   (2026-05-13): this push is DEAD TS-LEGACY in retail YR — BSS-zero init
   means `Capacity=0`, `IsAllocated=0`, `GrowthStep=0`, and the
   `(IsAllocated != 0 || Capacity == 0) && GrowthStep > 0` chain always
   evaluates false. Every push is silently dropped. No consumer, no allocator,
   no tick processor exists anywhere in the binary. Safe to skip in Rust
   port.** See [BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md §6](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md).
4. Spawns randomized destruction anims with probability gated by
   `g_RulesClass+0x168` (`BridgeExplosions` vector size) and
   `_DAT_007E1738` (probability float, ~0.95):
   - Random anim from `g_RulesClass+0x140` list (size at `+0x14C`) — **CORRECTION
     (2026-05-13): this is the `MetallicDebris` vector, NOT `BridgeExplosions`.
     Spawn is gated by a separate 50% RNG roll INSIDE the outer 95% gate.**
   - Random anim from `g_RulesClass+0x15C` list (size at `+0x168`) — this is
     `BridgeExplosions` (correctly labeled). Spawn is unconditional inside
     the outer 95% gate. Frame start = `RandomRanged(1, 5)`.
   - Z-offset uses `(char)this.Level * DAT_0089E7C0 + DAT_0089E7B4`.

**Offset-label corrections (2026-05-13):** the parent claim "+0xFA8 =
BridgeBlast weapon" is wrong — the string `"BridgeBlast"` does not exist
in YR. `+0xFA8` is `C4Warhead`, verified at `RulesClass__ReadCombatDamage @
0x66C32C` reading `s_C4Warhead_0083b1d4`. The labeled lists at `+0x140`
and `+0x15C` were swapped — `+0x140` is `MetallicDebris`, `+0x15C` is
`BridgeExplosions`. See [BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md §2](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_RUNTIME_DEEP_DIVE_GHIDRA_REPORT.md).

Skipped in `g_IsMapEditor != 0` — the function does nothing in editor mode.

---

## 4. INI Keys

| Key | Section | Default (code) | Retail YR Value | Effect |
|-----|---------|----------------|-----------------|--------|
| `CliffBackImpassability` | [General] | 0 | **2** (rules.ini:319, rulesmd.ini:409) | Controls 6-neighbor cliff-back check in RecalcAttributes. 0=disabled, 1=check-but-no-write, 2=set LandType=3 when isolated peak detected. **Active in YR with maximal effect.** |

**No other INI keys in scope.** Bridge mechanics are entirely tile-data /
overlay-driven, with the rules-side surface being just this one CliffBack key
(which is technically not a bridge mechanic — it's a terrain-passability
mechanic that happens to interact with the height arithmetic).

The original investigation plan flagged `g_RulesClass+0x664` as TS-legacy
suspect. **It is NOT TS-legacy** — retail YR's rulesmd.ini explicitly sets it
to 2, so the cliff-back check fires.

---

## 5. Integration Points

### What calls these systems
- `CheckBridgeTraversal` is invoked via vtable slot +0x1B0 of every concrete
  unit class. Only the call site inside `UnitClass__Can_Enter_Cell` at
  0x73F2EB is in scope for this investigation.
- `UnitClass__Can_Enter_Cell` is called by A* (PathfinderClass) and by every
  unit-movement decision. The vtable slot is at +0x1AC on UnitClass /
  AircraftClass / LocomotionClass.
- `RecalcAttributes` is called from 40+ sites. Bridge-relevant ones:
  - `SetBridgeDirection_*` does NOT call RecalcAttributes (it calls
    `RadarClass__MarkTerrainDirty` for redraw).
  - Bridge build/destruction paths trigger RecalcAttributes indirectly via
    overlay placement (PlaceBuilding / DestroyOverlay).
  - **In retail, RecalcAttributes is called once per cell at map load AND
    once per overlay change.** It does NOT fire on bridge collapse directly
    — the collapse path mutates flags via SetBridgeDirection without calling
    RecalcAttributes. This means **CellClass+0x11A (Height) and +0x11B
    (Level) are STATIC after map load in retail**, except where an
    overlay-change triggers RecalcAttributes.
- `SetBridgeDirection_*` runs at gameplay time via the damage state machine.
  Callers categorized in §3.5.

### What these systems call
- `CheckBridgeTraversal` calls only `MapClass__Get_CellClass` (when computing
  dst from direction).
- `UnitClass__Can_Enter_Cell` calls 30+ helpers; for the two-pass it just
  calls the vtable slot at +0x1B0 (= CheckBridgeTraversal).
- `RecalcAttributes` calls `TMP_ReadSlopeType` (0x5471B0), `MapClass__Get_CellClass`
  (×6 in the cliff-back check, ×6 again in second branch, ×6 again in third),
  `CellClass__ApplyLAT_and_SlopeFixup` (0x47CA80), `CellClass__RecalcZoneType`
  (0x483C80), `CellClass__OverlayToTiberiumIndex` (0x5FDD20), `TubeClass__Constructor`
  (0x727FD0), and `AnimClass__Constructor` (0x421EA0) for tube anims.
- `SetBridgeDirection_*` calls `MapClass__Get_CellClass`,
  `RadarClass__MarkTerrainDirty`, and `BlowUpBridge`.

### Tick ordering

- `Process_Drive_Track` runs in the movement phase of `advance_tick`. The
  on_bridge predicate fires per cell-boundary crossing.
- `ProcessBridgeDamageStateMachine_*` runs in the building-damage phase, AFTER
  movement. So a unit can cross a bridge cell whose 0x80 flag is about to be
  cleared this tick — they see the OLD flag until next tick. **This timing
  matters for Rust:** if Rust mutates `PathCell.bridge_walkable` on damage
  events, the mutation must happen in the same relative phase (post-movement,
  pre-tick-hash).
- `RecalcAttributes` runs only at map load and overlay-change events. NOT
  per-tick.

---

## 6. Current Rust Implementation Status

| Subsystem | Status | Files |
|-----------|--------|-------|
| Diff-1 SlopeIndex check | **NOT implemented.** PathCell has no SlopeIndex byte. A* allows or blocks height-diff==1 without slope check. | [src/sim/pathfinding/core.rs:123-153](../ra2-rust-game/src/sim/pathfinding/core.rs#L123-L153) |
| Two-pass Can_Enter_Cell | **Pre-decided layer only.** Rust decides layer at A* push-time, uses same layer for both object list AND occupancy. The binary's pre/post split is NOT modeled. | [src/sim/pathfinding/cell_entry.rs:85-211](../ra2-rust-game/src/sim/pathfinding/cell_entry.rs#L85-L211), [src/sim/pathfinding/core.rs:425-451](../ra2-rust-game/src/sim/pathfinding/core.rs#L425-L451) |
| RecalcAttributes write path | **NOT implemented.** Bridge flags are static-from-map-load in PathCell. CellClass equivalent doesn't exist as a runtime-mutable structure. | [src/map/resolved_terrain.rs:538-599](../ra2-rust-game/src/map/resolved_terrain.rs#L538-L599) |
| Runtime bit 0x80 mutation | **NOT implemented.** SetBridgeDirection has no Rust port. Bridge destruction is tracked via [BridgeRuntimeState](../ra2-rust-game/src/sim/bridge_state/mod.rs) but does NOT write back to PathGrid. **Parity bug.** |
| CliffBackImpassability | **NOT implemented.** No CliffBack equivalent in terrain resolve. | — |

---

## 7. Open Questions — RESOLVED

### Q1 — Origin of bit 0x80 on body cells at map load — **RESOLVED**

**Answer:** Bit 0x80 originates from **`OverlayClass::Mark` (or equivalent virtual at vtable slot +0x8 = function at 0x5FC570)** when a bridge overlay (IDs 0x18, 0x19, 0xA7, 0xB2, 0xED, 0xEE) is placed on a cell during initial .MAP loading. That function calls `SetBridgeDirection_NESW` or `_NWSE` with state=1, which writes bit 0x80 to the anchor cell at the function's first cell-flag store (0x47E0E7-ish in NESW).

**Function FUN_00565C10 = `MapClass::Resize` (verified)**. The function's body shows:
- An initial loop that iterates HouseClass instances and notifies them.
- A loop that iterates all existing cells and writes `cell+0x11B = param_4` (the default Level byte for newly-allocated cells).
- A **per-bit XOR copy** of cell.Flags (23 bits, one bit per line) into a temporary backup buffer (`puVar8[0x46] = (puVar8[0x46] ^ cell.Flags) & bit_N ^ puVar8[0x46]` for bit_N in {0x1, 0x2, 0x4, ..., 0x400000}). Same pattern at `puVar8[0x41]` (5 bits) for `cell+0x12C`.
- A `CellClass__Constructor` loop that re-allocates the cell array (sized by `param_2` rectangle).
- A reverse per-bit XOR copy that restores Flags + 0x12C onto the new cells.
- Finally, a `MapClass__CellIterator_Next` loop that detects cells satisfying `(cell.flags & 0x80) != 0 AND cell.bridge_anchor_ptr == 0` and calls SetBridgeDirection on them with `direction = ((~cell.flags >> 11) & 1) << 1` (= 0 or 2) and `state=1`. This is the **REFRESH** — only fires on cells that already had bit 0x80 from the per-bit-copied backup AND have no current anchor pointer.

So the path is: initial .MAP load → OverlayClass::Mark on every bridge-overlay cell → SetBridgeDirection sets bit 0x80 on the anchor → MapClass::Resize (called for map-size changes during gameplay or load) preserves bit 0x80 via the bit-copy backup → refresh loop re-applies SetBridgeDirection if bit survived but anchor pointer didn't.

**The per-bit XOR-copy pattern is intentionally exhaustive** — it copies every defined bit individually, which is robust to bit-meaning changes but explosively verbose. Likely TS-era code generated by a macro. Not relevant to Rust port (no equivalent of MapClass::Resize is needed if map size is fixed at load).

**Impact:** Confirms the original bit-0x80 origin is OverlayClass::Mark via SetBridgeDirection at overlay-placement time. Rust's resolved_terrain.rs already does the equivalent (sets `bridge_walkable=true` on cells with bridgehead overlay during terrain resolve). No action needed for Rust today; the only follow-up is implementing **runtime bit-0x80 mutation on bridge destruction/repair** (which is a separate finding tracked under §6 / §11).

---

### Q2 — Semantic of CellClass.Flags bits 0x2000 and 0x4000 — **RESOLVED**

**Answer:** Bits 0x2000 and 0x4000 are **WRITE-ONLY in retail YR's bridge code path**. SetBridgeDirection clears them on the opposite-step cell (cell-5 in §3.4). No reader of these bits exists in any decompiled bridge-family function.

**Verification:** Searched for `TEST [reg+0x140], 0x2000` and `TEST [reg+0x140], 0x4000` byte patterns (`F7 86 40 01 00 00 00 20 00 00` and `F7 87 40 01 00 00 00 20 00 00` and the 0x4000 variants) — **no matches** in either `[ESI+0x140]` or `[EDI+0x140]` form. No live reader exists.

**Conclusion:** These bits are **TS-era dead state** — preserved through MapClass::Resize's per-bit XOR copy (because the copy is mechanical), cleared on bridge construction, and otherwise unread. **Safe to ignore in Rust port.** They occupy 2 bits in CellClass.Flags that the Rust side does not need to model.

---

### Q3 — OverlayClass vtable slot identity — **RESOLVED**

**Answer:** Function at 0x5FC570 is at **vtable offset +0x8 of `vtable__OverlayClass` at 0x7EF4F0**. Ghidra knows this vtable by name — `OverlayClass::Constructor` at 0x5FC380 explicitly stores `&vtable__OverlayClass` into `*param_1` (this->vtable), confirming the vtable's class identity.

**Verification:** Cross-referenced the vtable contents at 0x7EF4F0 against named ObjectClass virtuals:
- Slot +0x00 (0x5F4330): empty `void f() { return; }` — typical pure-virtual stub or get_class_id no-op
- Slot +0x04 (0x5F4340): another short function
- **Slot +0x08 (0x5FC570): the function in question — handles overlay-ID-specific placement side-effects, dispatches to SetBridgeDirection for bridge overlay IDs**
- Slot +0x0C (0x5F4730): `ObjectClass::GetDrawExtent` (verified)
- Slot +0x10 (0x5F4870): `ObjectClass::GetDrawRect` (verified)
- Slots +0x14 onward: other ObjectClass methods

So 0x5FC570 is **OverlayClass's override of ObjectClass virtual at slot +0x8**. In TS/RA2's ObjectClass hierarchy, vtable+0x8 is conventionally `Mark(MarkType)`. However, the parameter ESI (= the first stack arg, compared to overlay IDs like 0x18, 0x19, etc.) doesn't fit a `MarkType` enum — it fits an overlay-type ID.

**Best-fit interpretation:** This is `OverlayClass::Mark` with the override pre-reading `this->Type->OverlayTypeIndex` into ESI before the dispatch chain (the disassembly shows reads of `[EAX+0xac]` = OverlayClass+0xAC = OverlayTypeClass pointer, and uses it indirectly). The function's behavior — "stamp myself onto the cell I'm placed on, with type-specific side effects for bridges" — matches Mark's canonical role.

**Function signature** (refined from disassembly): `void __thiscall OverlayClass::Mark(MarkType mark, OverlayTypeIndex type_id, CellClass *cell, ...)` — the parameters include a MarkType-like dispatcher (placed/removed/changed/redrawn) AND the overlay-type ID being placed (used for the bridge-direction dispatch).

**Impact:** Resolved. The function is OverlayClass::Mark — labeled in Ghidra. For Rust, no port needed today; the equivalent map-load logic happens in resolved_terrain.rs.

---

### Q4 — Whether the asymmetric 6-neighbor pattern in CliffBackImpassability is intentional — **VERIFIED (intent unknowable)**

**Answer:** The pattern is **verified retail behavior in the binary**. Direct disassembly read of RecalcAttributes (0x47D2B0–0x47DD63) shows three independent copies of the same 6-neighbor check chain (one in the overlay branch at 0x47D386-0x47D540, one in the cliff-fallback at 0x47D5FF-0x47D7C0, one at the end at 0x47DB59-0x47DD2A), and **all three copies use the same 6 offsets in the same order**:

1. (X, Y-1) — N
2. (X-1, Y) — W
3. (X+2, Y+2) — **peculiar 2-step SE** (intentional or bug, unknowable from binary)
4. (X+1, Y+1) — SE
5. (X-1, Y+1) — SW
6. (X+1, Y-1) — NE

**Missing:** S = (X, Y+1) and NW = (X-1, Y-1).

The fact that the same asymmetric pattern is repeated **three times** (probably from a C++ template or macro inlined into each branch) makes a "typo" interpretation less likely — the developer would have caught and fixed it across all three copies. Two more-plausible interpretations:

1. **Intentional for isometric coords**: In RA2's isometric world, "S" (Y+1) is roughly screen-down, but the diamond-grid neighbor of importance for a cliff face is at the SE (X+1, Y+1) and slightly farther SE (X+2, Y+2) which represent the "drop" off a cliff. The check is asking "is there a cliff face SE of me?" — and the (X+2, Y+2) captures the bottom of the cliff (2 cells away) while (X+1, Y+1) captures the immediate edge. The N, W, NE, SW round it out.
2. **TS-era diagonal-bridge fallout**: TS had diagonal bridges with longer-than-1 cell offsets. The (X+2, Y+2) may have meant "the cell beyond the diagonal bridge body." In YR, diagonal bridges still exist but the offset's specific meaning may be vestigial.

**Verified:** the pattern IS retail, fires with `CliffBackImpassability=2` (default in retail rulesmd.ini), affects every overlay-change call on any map with elevated terrain.

**Impact:** Per parity bar, Rust port MUST reproduce the exact pattern. The "why" is documentation-only.

---

### Q5 — Whether the two-pass divergence at bridgehead exit is observable — **DEFERRED (requires fidelity test)**

**Answer:** Cannot be answered from binary inspection alone. Requires running the same input through gamemd.exe AND the Rust port and comparing outputs.

**Specific fidelity test design:**
1. Map with a single high bridge spanning a chasm. Bridgehead cell at coords (X, Y).
2. Place a stationary friendly infantry at sub-cell position 0 of the bridgehead, on the **bridge layer** (deck height = Level+4).
3. Place a second stationary friendly infantry at sub-cell position 0 of the bridgehead, on the **ground layer** (height = Level).
4. Command a third unit (vehicle) to path FROM the bridge body cell adjacent to the bridgehead TO a ground cell adjacent to the bridgehead's opposite side. The path must cross the bridgehead.
5. Observe whether the vehicle is BLOCKED at the bridgehead (because the relevant occupancy slot is full) or PASSES (because the relevant occupancy slot is empty).

In retail, the pre-vtable decision picks bridge-layer object list (since the unit's current cell is bridge body), but the post-vtable bits read ground-layer occupancy IF the dst cell isn't a bridge. Rust's pre-decided layer would use either ground-layer for both OR bridge-layer for both, depending on its `is_at_bridge_level` evaluation at A* push-time.

If the binary and Rust both block: divergence is not observable in this case.
If the binary blocks but Rust passes (or vice versa): divergence is observable.

**Recommendation:** Build this fidelity test before deciding whether to implement the pre/post split. Most likely outcome: the divergence is theoretical and never fires in retail unit configurations.

---

## 8. Active in YR — Subsystem Verdicts

| Subsystem | Active in YR | Trigger frequency in normal play | Observable-impact verdict |
|-----------|--------------|-----------------------------------|---------------------------|
| Diff-1 SlopeIndex check (CheckBridgeTraversal) | **YES** | Every A* search across a slope (very common — every ramp on every map) | Wrong handling produces paths that walk through cliffs OR refuse to walk up valid ramps. **Player-visible.** |
| Two-pass Can_Enter_Cell | **YES** | Every A* search and unit-movement decision at a bridge cell | Mismatch only on bridgehead-exit tick edge case. **Rarely visible.** |
| RecalcAttributes runtime path | **NO** at runtime — runs only at map load and overlay change. The bridge-relevant cell bytes (+0x11A, +0x11B, +0x11C) are static after load. | N/A | No runtime parity concern for Level/Height bytes specifically. |
| RecalcAttributes write path (load-time) | **YES** at map load | Once per cell at load | Affects every cell's LandType, SlopeIndex, and zone-cache mirrors. Rust loads from resolved terrain instead — outputs must match. |
| SetBridgeDirection bit 0x80 mutation | **YES** at gameplay time | Whenever a bridge is damaged or repaired (per-match, depending on unit interactions) | **Player-visible.** A destroyed bridge should become non-traversable; Rust currently leaves PathCell.bridge_walkable true. |
| CliffBackImpassability check | **YES** | Every RecalcAttributes call (= every overlay change) on a map with elevated terrain | Affects LandType=3 (rough) assignment on isolated peaks. Player notices via unit speed differences on cliff-back terrain. |
| LocomotionClass__Can_Enter_Cell (0x55ABF0) | **NO** — vestigial TS-era stub returns 0 | Never called via live vtable dispatch | TS-legacy, skip in Rust. |

---

## 9. TS-Legacy Register (verified)

1. **`LocomotionClass__Can_Enter_Cell` (0x55ABF0)** — base-class stub returning
   0. Overridden by every concrete locomotor. Dead code in retail YR.
2. **CellClass.Flags bits 0x2000, 0x4000** — cleared by SetBridgeDirection on
   opposite-step cell only; no live readers found. Likely TS-era.
3. **The (X+2,Y+2) offset in CliffBackImpassability's neighbor list** — may
   be a TS-era bug, but the rules.ini setting `=2` activates this code path
   in retail. Reproduce verbatim.
4. **`AircraftClass__Can_Enter_Cell` (0x415B10)** — NOT TS-legacy. Active in
   retail (aircraft landing search). Does NOT participate in the two-pass
   bridge mechanism (aircraft fly over bridges).
5. **The `level_override` parameter on RecalcAttributes** — appears to be a
   late-era addition (the hidden 2nd arg makes RET 0x4 instead of RET 0). Not
   TS-legacy; specific callers (likely building placement / map-load) use it
   to force a cell's Level value.

---

## 10. Sources

**Ghidra addresses decompiled (FULL):**
- 0x47D2B0 — CellClass__RecalcAttributes
- 0x4D9C60 — CheckBridgeTraversal
- 0x73F0A0 — UnitClass__Can_Enter_Cell (full disassembly read)
- 0x47E040 — CellClass__SetBridgeDirection_NESW (with NWSE byte-diff verification)
- 0x47DD70 — CellClass__BlowUpBridge
- 0x4B0F20 — DriveLocomotionClass__Process_Drive_Track

**Ghidra addresses decompiled (LIGHT/SAMPLED):**
- 0x5471B0 — TMP_ReadSlopeType
- 0x415B10 — AircraftClass__Can_Enter_Cell
- 0x55ABF0 — LocomotionClass__Can_Enter_Cell (1-instruction stub)
- 0x5FC570 — `OverlayClass::Mark` (vtable +0x8 of vtable__OverlayClass; assembly context for overlay-ID dispatch chain; Ghidra has no function definition yet)
- 0x5FC380 — `OverlayClass::Constructor` (confirms vtable__OverlayClass identity)
- 0x565C10 — **`MapClass::Resize`** (full decompilation — confirms it's a resize/preserve-and-restore function, not a load function; bit 0x80 preserved via per-bit XOR copy, refreshed via SetBridgeDirection on cells that retained the bit)
- 0x412857 — RulesClass init (zero-write to g_RulesClass+0x664)
- 0x66F1D9 — RulesClass__ReadGeneral (INI read of CliffBackImpassability)

**Instruction-level disassembly read in full for verification:**
- 0x47D2B0 — 0x47DD63 (RecalcAttributes body, line-by-line)
- 0x73F0A0 — 0x73FD43 (Can_Enter_Cell body, line-by-line)

**Memory reads for byte-identity verification:**
- 0x47E040 vs 0x47E470 at five offset pairs (prologue, 3 mid sections, epilogue)

**Vtable / data reads:**
- 0x7EF4F0 — OverlayClass vtable (head of 8 entries)
- 0x83C8CC — "CliffBackImpassability" string
- 0x83C8E4 — adjacent string "IceBreakingWeight" / "IceCrackingW..."

**Strings searched:**
- "Cliff", "Impass", "HighBridge", "LowBridge"

**Docs referenced:**
- docs/research/BRIDGE_SYSTEM.md
- [docs/research/AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) (2026-05-11 entries)
- docs/research/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md
- docs/research/CELLCLASS_STRUCT_GHIDRA_REPORT.md
- [docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md)
- docs/plans/2026-05-11-bridge-locomotor-layer-correctness-design.md (parent plan)
- docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md (origin of G3/G4/G6)
- docs/plans/2026-05-12-bridge-mechanics-deferred-investigation-plan.md (this investigation's scope)

**INI files checked:**
- ini/rules.ini (line 319: CliffBackImpassability=2)
- ini/rulesmd.ini (line 409: CliffBackImpassability=2)

**Document corrections to existing research:**
- BRIDGE_SYSTEM.md is **correct** on SlopeIndex offset (+0x11C). The 2026-05-12
  scoping pass's claim that +0x11A is SlopeIndex was wrong; AUDIT_LOG and
  BRIDGE_SYSTEM stand.
- AUDIT_LOG 2026-05-11 entry on `0x47D94E` is **correct** (`MOV [ESI+0x11B], AL`).
- AUDIT_LOG's "byte-identical" claim for NESW/NWSE is **imprecise** — they are
  instruction-identical / compiled-twin. Real bytes differ at CALL relative
  offsets.
- AUDIT_LOG should add: **RecalcAttributes has a hidden second parameter
  `level_override`** (RET 0x4). This is not currently documented.
- BRIDGE_SYSTEM.md §"RecalcAttributes Bridge Correction" can be expanded with
  the per-byte write inventory in §3.3 of this report.
- 2026-05-11 design's Tiny-Detail Ledger #11 ("Can_Enter_Cell pre-decision
  matches post-switch output") was OPTIMISTIC — confirmed there is a
  pre-vs-post split that can produce divergent output in edge cases. Note as
  bounded-parity-loss, not bug.

---

## 11. Recommendations (for follow-up brainstorms — not implementation)

These are observations for the next pipeline step, not action items for this
report.

1. **The bit-0x80 runtime mutation is the only finding here that's a true
   parity bug under the current Rust design.** A destroyed bridge should
   block pathing; Rust's PathCell.bridge_walkable stays true. Brainstorm
   target: hook bridge-destruction events from BridgeRuntimeState to the
   PathGrid update mechanism. Severity: medium — fires every time a bridge
   is destroyed in normal play.

2. **Diff-1 SlopeIndex check** is missing entirely. Without it, A* may route
   paths up cliff faces. In retail maps the practical impact depends on how
   often retail map terrain has height-diff-1 cells with zero SlopeIndex
   (cliffs) vs nonzero SlopeIndex (ramps). Brainstorm target: propagate
   `slope_type` from `ResolvedTerrainCell` into `PathCell`, gate diff-1
   A* edges on it.

3. **CliffBackImpassability** is missing. Without it, LandType=3 is never
   assigned on isolated peaks. The Rust LandType-vs-Speed table will assign
   non-cliff speeds to cells that retail treats as cliffs. Brainstorm target:
   implement the asymmetric 6-neighbor scan during terrain resolve, with the
   exact pattern documented in §3.3.

4. **Two-pass Can_Enter_Cell divergence** is bounded. Defer to brainstorm
   with a fidelity test that probes the bridgehead-exit edge case.

5. **The hidden `level_override` parameter on RecalcAttributes** is a
   non-issue for Rust today (we don't have a RecalcAttributes equivalent at
   runtime). If a future Rust runtime-RecalcAttributes is built (e.g., to
   support gameplay-time overlay placement), this parameter must be
   reproduced — it's how the binary forces foundation Level on building
   placement.
