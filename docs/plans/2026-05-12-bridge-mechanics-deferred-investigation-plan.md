# Bridge Mechanics — Deferred Items Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass covering four
> bridge-mechanic items deferred from the 2026-05-11 bridge-locomotor-layer
> design ("Known Parity Boundary" §1, §2, §3, §5). Execute it by running
> `/re-investigate bridge mechanics deferred` with this plan loaded as context,
> OR dispatch the function inventory to subagents in batches.

**Topic:** Bridge mechanics — diff-1 SlopeIndex ramp passability, two-pass
`Can_Enter_Cell` at bridgeheads, RecalcAttributes cell-flag write path,
SetBridgeDirection_NESW/_NWSE caller graph.

**Scope Size:** Medium — ~28 functions, ~0 new INI keys, 4 verified
inter-doc conflicts to resolve.

**Est. Effort:** ~6-10h of `/re-investigate` work
(~15-30 min per FULL function × 6, ~5-10 min per MEDIUM × 10, ~2-5 min per
LIGHT × 12).

**Prior Research:**
- [BRIDGE_SYSTEM.md](../../../ra2-rust-game-docs/BRIDGE_SYSTEM.md) — has §CheckBridgeTraversal and §SetBridgeDirection; primary source for current claims
- [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) (2026-05-11 entries) — claims RecalcAttributes writes +0x11B at 0x47D94E; conflicts with fresh scoping read
- [UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md) — Phase 6 documents the two-pass switch condition
- [CELLCLASS_STRUCT_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/CELLCLASS_STRUCT_GHIDRA_REPORT.md) — offset table for CellClass fields
- [LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md) — TMP_ReadSlopeType @ 0x005471B0
- [docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md](../gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md) — G2/G3/G4/G6 origin, G6 = the two-pass gap
- [docs/plans/2026-05-11-bridge-locomotor-layer-correctness-design.md](2026-05-11-bridge-locomotor-layer-correctness-design.md) §"Known Parity Boundary" §1-5 — deferral list

**Expected Output:** updated/extended sections in
[BRIDGE_SYSTEM.md](../../../ra2-rust-game-docs/BRIDGE_SYSTEM.md) (the natural
home — this topic is bridge-internals), plus a new report
`BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md` if the volume warrants splitting.

**Next Pipeline Step:** depends on findings —
- If diff-1 SlopeIndex turns out to matter for retail (very rare), `/brainstorm` then implement.
- If two-pass `Can_Enter_Cell` produces observably different output than our pre-decided layer, `/brainstorm` a fix.
- If the RecalcAttributes write path mutates cell flags at gameplay time (not just map load), that's a runtime parity bug — `/brainstorm` then implement.
- If SetBridgeDirection runtime cascade affects pathfinding (e.g., updating bit 0x80 mid-game on bridge damage), `/brainstorm` a Rust port.
- If nothing has observable effect in retail YR, document as "audit-only, no implementation needed" and close the open question.

---

## 1. Goal

Answer these specific questions with binary evidence, so the 2026-05-11 design's
"Known Parity Boundary" can be either closed out or escalated to brainstorm:

1. **Diff-1 SlopeIndex** — Which byte does `CheckBridgeTraversal` actually read
   at height-diff==1: `cell+0x11A`, `cell+0x11B`, or `cell+0x11C`? What does
   zero vs non-zero on that byte gate, and in which directions (uphill /
   downhill / both)? Does it ever block a retail-map move that our Rust A*
   currently allows?

2. **Two-pass `Can_Enter_Cell`** — Decompile `UnitClass__Can_Enter_Cell` at
   `0x73F0A0` end-to-end. Confirm the precise condition for the layer-switch
   re-read, the exact state mutated between pass 1 and pass 2, and verify that
   our pre-decided `target_layer` from A*'s `path_layers` produces the same
   final pass/block verdict on retail bridge cells. If the verdict differs in
   any case, name the case.

3. **RecalcAttributes write path** — Resolve the doc/binary conflict: does
   `RecalcAttributes (0x47D2B0)` write `cell+0x11A`, `cell+0x11B`, both, or
   neither? At what instruction address(es)? Are the bridge-relevant cell
   fields (`+0x11A` sub-type, `+0x11B` height, `+0x11C` slope) ever mutated
   at runtime (not just map-load), and if so, by what triggers?

4. **SetBridgeDirection callers** — Enumerate every caller of `0x47E040`
   (NESW) and `0x47E470` (NWSE), categorize each by trigger (map-init / repair
   / damage / save-load / unknown), and identify what bits/fields the function
   writes to each visited cell. Specifically resolve the three uncategorized
   callers at `0x5FC5F0`, `0x5FC600`, `0x5FC62C`. Determine whether bit 0x80
   (bridge_walkable) is ever flipped at gameplay time (vs. being a map-load
   one-shot) — because Rust currently treats `PathCell.bridge_walkable` as
   static-from-map-load.

The report must conclude with **"Active in retail YR: yes / no / conditional"**
for each subsystem, and a one-line **observable-impact verdict** ("does a
player ever see this fire in normal play"). Per the parity bar, internals
without observable consequence don't need a Rust port.

---

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| BRIDGE_SYSTEM.md §CheckBridgeTraversal | Height-diff 0/1/4 rules at 0x4D9C60 | HIGH | Says +0x11C = SlopeIndex; conflicts with fresh Ghidra (+0x11A) — must resolve |
| BRIDGE_SYSTEM.md §SetBridgeDirection | Function-pair behavior, byte-identity | HIGH | Caller graph incomplete; 3 uncategorized sites in 0x5FC region |
| BRIDGE_SYSTEM.md §RecalcAttributes Bridge Correction | Says +0x11B is the bridge-height byte | MEDIUM | Doesn't enumerate write sites; AUDIT_LOG claim at 0x47D94E conflicts with fresh read |
| [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) 2026-05-11 | RecalcAttributes writes +0x11B at 0x47D94E | HIGH (claimed) | Possibly wrong — Agent D's fresh scope says +0x11A. Re-verify the instruction at 0x47D94E. |
| [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) 2026-05-11 | NESW/NWSE byte-identical | HIGH | Verified, no gap |
| UNIT_CAN_ENTER_CELL §Phase 6 | Two-pass switch condition (`prevFacing == cell.height + 4 AND flags & 0x100`) | HIGH | Doesn't decompile UnitClass__Can_Enter_Cell at 0x73F0A0 end-to-end — only summarizes Phase 6. Need full pass-1/pass-2 state diff. |
| CELLCLASS_STRUCT_GHIDRA_REPORT | Offset table: +0x11A=sub_type, +0x11B=height, +0x11C=slope_type | HIGH (claimed) | Same conflict as above; the offset table may be the canonical source but fresh read disagrees. |
| LAT_GROUPS_AND_SLOPE_FIXUP | TMP_ReadSlopeType at 0x005471B0, writes "result to cell+0x11C" | HIGH (claimed) | Conflicts with Agent D's claim that the write site is +0x11A; must verify with disassembly at 0x47D2B0's TMP_ReadSlopeType call. |
| disparity-scan G6 | Pre-decision vs post-switch design gap | HIGH | No claim about whether it produces observably different output in retail |

**Conflicts between reports (LOAD-BEARING — resolve FIRST in execution):**

- **Conflict A — Which CellClass offset is the SlopeIndex?** BRIDGE_SYSTEM.md /
  CELLCLASS_STRUCT_GHIDRA_REPORT / LAT_GROUPS_AND_SLOPE_FIXUP all say `+0x11C`.
  Fresh scoping read (Agent D) says `+0x11A`. The CheckBridgeTraversal diff-1
  branch reads one of these. **Must verify with disassembly first**; everything
  else in Subtopic 1 hinges on this.
- **Conflict B — What instruction at 0x47D94E?** AUDIT_LOG says
  `MOV [ESI+0x11B], AL`; fresh scoping read disagrees. Re-disassemble.
- **Conflict C — What does RecalcAttributes actually write?** Various docs
  attribute writes to +0x11B, +0x11C, +0x11D, +0x11E. Need a definitive
  per-instruction write map.

---

## 3. Function Inventory

| #  | Phase | Address    | Current Name                                                | Scope Reason                                                                     | Depth Target | TS-Legacy Risk |
|----|-------|------------|-------------------------------------------------------------|----------------------------------------------------------------------------------|--------------|----------------|
| 1  | 1     | 0x4D9C60   | `CheckBridgeTraversal` (vtable +0x1B0)                      | Subtopic 1+2 entry. Owns the diff-1 SlopeIndex branch and mutates outparams for Subtopic 2's two-pass | **FULL** | Low |
| 2  | 1     | 0x73F0A0   | `UnitClass__Can_Enter_Cell`                                 | Subtopic 2 primary. The "two-pass" lives here; vtable dispatch to #1 is mid-function | **FULL** | Low |
| 3  | 1     | 0x47D2B0   | `CellClass__RecalcAttributes`                               | Subtopic 3 primary. Must enumerate ALL write instructions to +0x11A/+0x11B/+0x11C/+0x11D/+0x11E | **FULL** | Medium — `g_RulesClass+0x664` gate inside |
| 4  | 1     | 0x47E040   | `CellClass__SetBridgeDirection_NESW`                        | Subtopic 4 primary. Need full bit/field write map for the anchor + 4 neighbor cells | **FULL** | Low |
| 5  | 1     | 0x47E470   | `CellClass__SetBridgeDirection_NWSE`                        | Confirm byte-identity to #4; verify via structural check, no full re-decompile  | LIGHT | Low |
| 6  | 2     | 0x005471B0 | `TMP_ReadSlopeType`                                         | Source of the byte that ends up at +0x11A or +0x11C — disambiguates Conflict A  | MEDIUM | Low |
| 7  | 2     | 0x47DD70   | `CellClass__BlowUpBridge`                                   | Called from SetBridgeDirection_* when health_state=0. Mutates cell-flag state at runtime — directly relevant to Subtopic 3's "runtime cell-flag mutation" question | MEDIUM | Low |
| 8  | 2     | 0x415B10   | `AircraftClass__Can_Enter_Cell`                             | Vtable peer of #2. Confirm whether it has the same two-pass or skips it (aircraft don't care about bridge layer)  | LIGHT | Low |
| 9  | 2     | 0x55ABF0   | `LocomotionClass__Can_Enter_Cell`                           | Vestigial stub (returns 0). Confirm it's dead in retail YR  | LIGHT | **Medium — likely TS-legacy base** |
| 10 | 2     | 0x4AF4A0   | `DriveLocomotionClass::ComputeBridgeZOffset`                | Reads +0x11B (height_level) for Z-offset. Cross-references Subtopic 3's write path — if +0x11B is mutated mid-game, this read sees the new value | LIGHT | Low |
| 11 | 2     | 0x4B0F20   | `DriveLocomotionClass::Process_Drive_Track`                 | Reads +0x11B as part of on_bridge predicate (already implemented in Rust). Confirm read offset matches whatever Subtopic 3 nails down as the canonical height byte | LIGHT | Low |
| 12 | 2     | 0x47D94E   | (write instruction inside #3)                               | The specific instruction the AUDIT_LOG claims writes +0x11B. Disassemble this exact instruction to resolve Conflict B  | MEDIUM | Low |
| 13 | 2     | 0x47d35e   | (TMP_ReadSlopeType call site inside #3)                     | BRIDGE_SYSTEM.md cites this site as setting +0x11C. Disassemble and confirm dest offset  | MEDIUM | Low |
| 14 | 3     | 0x576BA0   | `ProcessBridgeDamageStateMachine_High`                      | Caller of #4 (NESW). 2 call sites. Damage-tick path — gameplay-time invocation = Subtopic 4's key question  | MEDIUM | Low |
| 15 | 3     | 0x571490   | `ProcessBridgeDamageStateMachine_Low`                       | Caller of #5 (NWSE). Damage-tick path for low bridges  | MEDIUM | Low |
| 16 | 3     | 0x57F200   | `RepairBridge_Low`                                          | Repair path caller (per BRIDGE_SYSTEM.md; address from doc, verify exists)  | LIGHT | Low |
| 17 | 3     | 0x57F440   | `RepairBridge_High`                                         | Repair path caller (per BRIDGE_SYSTEM.md; verify)  | LIGHT | Low |
| 18 | 3     | 0x572400   | `MapClass__UpdateRamp_NS_CollapseA_High`                    | Damage cascade caller — confirm trigger conditions, what state is mutated  | LIGHT | Low |
| 19 | 3     | 0x572800   | `MapClass__UpdateRamp_NS_CollapseB_High`                    | Damage cascade caller  | LIGHT | Low |
| 20 | 3     | 0x572D00   | `MapClass__UpdateRamp_EW_CollapseA_High`                    | Damage cascade caller  | LIGHT | Low |
| 21 | 3     | 0x573200   | `MapClass__UpdateRamp_EW_CollapseB_High`                    | Damage cascade caller  | LIGHT | Low |
| 22 | 3     | 0x576200   | `MapClass__UpdateBridgeEdgeTiles_High`                      | Edge fixup after damage/repair  | LIGHT | Low |
| 23 | 3     | 0x56EFD0, 0x56F370, 0x56F940, 0x56FD10 | `MapClass__UpdateRamp_*_Low` (×4)              | NWSE-side mirrors. Confirm structural symmetry  | LIGHT | Low |
| 24 | 3     | 0x570AE0   | `MapClass__UpdateBridgeEdgeTiles_Low`                       | Low-bridge edge fixup  | LIGHT | Low |
| 25 | 3     | 0x565C10   | `FUN_00565C10` (map-init/construction loop)                 | Map-load caller of both #4 and #5. Confirm it's the canonical map-init bridge-flag path; identify where bit 0x80 is set (Rust currently assumes here)  | MEDIUM | Low |
| 26 | 3     | 0x5FC5F0   | (unknown — call site of #4)                                 | Uncategorized. Identify what enclosing function this is in, what triggers it  | MEDIUM | **Unknown — possibly save/load or TS editor** |
| 27 | 3     | 0x5FC600   | (unknown — call site of #4)                                 | Uncategorized. Likely sibling of #26  | MEDIUM | **Unknown** |
| 28 | 3     | 0x5FC62C   | (unknown — call site of #5)                                 | Uncategorized. NWSE side of #26/#27 trio  | MEDIUM | **Unknown** |

**Phase 1 checkpoint:** After functions #1-5 are done, the executor must
summarize:
- The resolved CellClass byte map (which offset is SlopeIndex, which is
  height, what does RecalcAttributes actually write where).
- The exact two-pass mechanism in #2 (what state is read in pass 1 vs pass 2).
- The bit/field write map of SetBridgeDirection.

If Phase 1 surfaces that the conflicts can't be resolved without going deeper
(e.g., the SlopeIndex byte is read in multiple places that disagree), the
plan is revised before Phase 2.

---

## 4. Detail Checklist

The executor must record each of these explicitly:

**Magic numbers / constants to decode:**
- The exact CellClass byte offset(s) read by `CheckBridgeTraversal` at
  height-diff == 1 (resolves Conflict A).
- The constant subtracted/compared at height-diff == 1: is it `!= 0`,
  `>= some threshold`, or a multi-bit mask?
- In `SetBridgeDirection_*`: the mask `0xFFFEE07F` (which bits it clears),
  the value of `param_3` that selects bit 0x80, and the bits 8, 9, 11, 12,
  16 that are written.
- `g_RulesClass+0x664` (read in RecalcAttributes) — the actual default in
  retail YR rulesmd.ini. Is this a known INI key (e.g., a passability flag),
  or an internal field? **TS-legacy risk** — confirm before implementing
  any of its branches.
- The constant `+ 4` in the two-pass condition `prevFacing == cell.height + 4`
  — this is the 4-level bridge offset already in our Tiny-Detail Ledger
  (ledger #5). Confirm it's the same constant.

**Bit flags / masks:**
- CellClass `+0x140` Flags bits: 0x80 (bridge_walkable anchor), 0x100 (cell
  is bridge body), 0x200 (cell is bridgehead). Enumerate every read/write site
  in #1, #2, #3, #4, #7.
- The `prevFacing` value at -1 vs >= 0 — what set of facings actually invoke
  the two-pass? `prevFacing == cell.height + 4` only makes sense if facing
  and height are numerically comparable; verify the semantic.

**State machine states / branches:**
- `CheckBridgeTraversal`'s return value space: 0 (passable), 7 (blocked), and
  any others. Map each to a "block reason."
- `UnitClass__Can_Enter_Cell`'s return space (0-7 claimed); enumerate.

**INI keys to verify:**
- `g_RulesClass+0x664` — locate the rules.ini key that maps here. Confirm
  default value. If it gates retail-affecting behavior, expand scope.
- (Otherwise this topic has no INI surface — bridge mechanics are tile/cell-
  driven, not rules-driven.)

**Struct offsets to extract:**
- Definitive CellClass map for the +0x11A..+0x11E region:
  ```
  +0x11A: ?  (sub_type per docs; SlopeIndex per fresh scope — RESOLVE)
  +0x11B: ?  (height_level i8 per docs; mutation site per AUDIT_LOG — VERIFY)
  +0x11C: ?  (slope_type per docs; separate ramp-passability byte per fresh
              scope — RESOLVE)
  +0x11D: ?  (computed from height_raw - 30 / 15 per CELLCLASS_STRUCT — VERIFY)
  +0x11E: ?  (0 or 9 per fresh scope on SetBridgeDirection — VERIFY)
  ```
- For `param_1` typing inside Ghidra: which functions use `int *` (indexed by
  4) vs `int` (byte offset)? Per CLAUDE.md, this is a known foot-gun.

**Clamps, rounding, off-by-ones:**
- The `(height_raw - 30) / 15` formula in RecalcAttributes — what
  happens at heights below 30? Negative result, clamp to zero, or wraparound
  on a signed byte?
- The signed/unsigned typing of +0x11B reads (`MOVSX` per AUDIT_LOG ledger #3
  in the 2026-05-11 design) — confirm at the actual disassembly.

**Edge cases to test:**
- height-diff = 1 going **uphill** vs **downhill** — does CheckBridgeTraversal
  read the SAME cell's SlopeIndex (current's? destination's?) in both cases?
  BRIDGE_SYSTEM.md line 1322-1323 suggests destination going down, source
  going up — verify.
- A cell that is both `bridge_walkable` (0x100) and has nonzero SlopeIndex —
  which predicate fires first?
- SetBridgeDirection called on a cell that's already had its bit 0x80 set —
  is the call idempotent, or does it toggle?
- BlowUpBridge invoked from SetBridgeDirection_* when health=0 — what cell
  state remains after? (relevant to Rust runtime invalidation question)

**Timing / ordering:**
- When during the game tick does `ProcessBridgeDamageStateMachine_*` fire?
  Before or after pathfinder updates its grid view? (If after, paths in
  flight may pass over a cell whose 0x80 was just cleared — observable as a
  unit walking onto a collapsing bridge.)
- The map-init order: bit 0x80 set by `FUN_00565C10` happens before A* is
  ever invoked? Confirm.
- Inside `UnitClass__Can_Enter_Cell` at 0x73F0A0: precise sequence of
  pass-1 occupancy read → vtable call to #1 → pass-2 re-read. Document the
  three-step sequence with addresses.

**TS-legacy flags:**
- `g_RulesClass+0x664` (RecalcAttributes neighbor-Level → LandType=3
  block) — likely a TS-era cliff-impassability flag. Verify default.
- `LocomotionClass::Can_Enter_Cell` (#9) returning 0 is suspicious — likely
  TS-base vestigial.
- Cell capability bytes `+0x16AE`, `+0x16AD` etc. read in `Can_Enter_Cell`
  passability ladder — several map to TS bits (tiberium-crusher, etc.).
  Flag any encountered in the bridge-relevant branches.

**Vtable dispatches:**
- `(**(code **)(*param_1 + 0x1b0))(...)` in #2 — confirm this is the
  vtable slot for `CheckBridgeTraversal` and resolve which classes override it.
  Vtable DATA xrefs from Agent D: 0x7E2454, 0x7E8E44, 0x7EB208, 0x7F5E20.
  Identify the class for each.

---

## 5. INI Keys in Scope

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|---------|-------------------|----------------------------|
| (none directly) | | | Bridge mechanics are tile/cell-data driven, not INI-driven | N/A |
| (potential) `g_RulesClass+0x664` flag | [General] or [Bridges]? | Unknown | Read in RecalcAttributes; gates a "set LandType=3 if neighbor Level too low" block. Likely a TS-era cliff/impassability toggle | Unknown — locate by xref to RulesClass field offset 0x664 |

This is fundamentally an internals/RE investigation. No INI keys should be
parsed in Rust as a direct result of this work (the bridge-pathfinding-relevant
INI surface — `Foundation=`, `BridgeRepairHut=`, etc. — is out of scope; that's
the bridges-tier1-ini-parser plan from 2026-05-06).

If `g_RulesClass+0x664` turns out to be a documented rules.ini key that
**defaults to enabled in YR** AND gates retail-visible behavior, the scope
expands to include it; otherwise it stays in the TS-legacy register.

---

## 6. Caller & Integration Map

### Callers of `CheckBridgeTraversal` (0x4D9C60)

| Caller | Calls Into | When Invoked | Should Executor Decompile? |
|--------|-----------|--------------|----------------------------|
| `UnitClass__Can_Enter_Cell` (0x73F0A0) | #1 via vtable+0x1B0 | Every A* edge expansion + every unit Can_Enter check | YES (function #2) |
| 3 other vtable DATA xrefs (0x7E8E44, 0x7EB208, 0x7F5E20) | #1 | Unknown vtable slots | LIGHT — identify owning classes only |

### Callers of `UnitClass__Can_Enter_Cell` (0x73F0A0)

Out of scope to enumerate exhaustively — but spot-check that A* pathfinder
(0x429A90) is one, and that `Process_Drive_Track` is NOT (it uses the
cell-flag predicate directly, per the 2026-05-11 design).

### Callers of `RecalcAttributes` (0x47D2B0) — >40 total

Categorize each as one of: **map-init**, **overlay-change** (place /
destroy), **bridge-event** (build/damage/repair/collapse), **save-load**,
**editor-only**, **other**. Only **bridge-event** callers and **runtime
overlay-change** callers need MEDIUM depth; the rest are LIGHT.

| Caller (sample) | Category | Notes |
|-----------------|----------|-------|
| PlaceBuilding | overlay-change runtime | Could touch bridge cells if building placed near bridgehead — check |
| DestroyOverlay | overlay-change runtime | Bridge destruction overlay? — check |
| Reduce_Tiberium | overlay-change runtime | Off-topic but confirms RecalcAttributes runs every tib reduction |
| MapEditor calls | editor-only | Skip |
| BlowUpBridge cascade | bridge-event runtime | **Critical for Subtopic 3** |

Full enumeration is a Phase 3 task; Phase 1 just needs to confirm the
**runtime bridge-event callers exist** (else Subtopic 3 is moot).

### Callers of SetBridgeDirection_NESW / _NWSE

Full enumeration (28 total: 10+8 + uncategorized) is in the function inventory
table (#14-#28). Categorization above.

### Rust integration today

- Bridge cell flags (`bridge_walkable`, `transition`, `bridge_deck_level`)
  are set **once** at map load in
  [src/map/resolved_terrain.rs:196-599](../../src/map/resolved_terrain.rs#L196-L599)
  and never mutated thereafter.
- Bridge runtime state lives in
  [src/sim/bridge_state/mod.rs](../../src/sim/bridge_state/mod.rs) as a
  separate damage-state tracker — does NOT write back to `PathGrid`.
- The Rust A* in [src/sim/pathfinding/core.rs:425-451](../../src/sim/pathfinding/core.rs#L425-L451)
  pre-decides the layer at neighbor-expansion time using
  `is_at_bridge_level(current.height, neighbor)` — the equivalent of gamemd's
  post-two-pass output, but computed at push (not at pop).
- `PathCell` carries no SlopeIndex byte
  ([src/sim/pathfinding/core.rs:665-671](../../src/sim/pathfinding/core.rs#L665-L671)).
  The terrain-resolve layer has `slope_type` and `canonical_ramp` on
  `ResolvedTerrainCell`
  ([src/map/resolved_terrain.rs:68-130](../../src/map/resolved_terrain.rs#L68-L130))
  but these are not propagated to pathfinding.

### Callers NOT to be investigated

- All non-bridge-relevant RecalcAttributes callers (most of the >40 — Phase 3
  filtering only).
- The full Can_Enter_Cell caller graph (every A* and every unit movement
  decision — implicit, no value in enumerating).
- The full vtable usage of CheckBridgeTraversal beyond identifying overriding
  classes.

---

## 7. TS-Legacy Risk Register

Every TS-legacy concern surfaced by scoping, to be cross-checked during execution:

1. **`g_RulesClass+0x664` flag** — read 3× in `RecalcAttributes (0x47D2B0)`,
   value 1 or 2, gates a neighbor-Level→LandType=3 block. **Locate the rules
   key, verify default in YR rulesmd.ini.** If default is off (TS-only) →
   skip the branch in any Rust port.
2. **`LocomotionClass::Can_Enter_Cell` (0x55ABF0)** stub returning 0 — likely
   TS-era base class fallback. Confirm no live YR caller hits it.
3. **Cell capability bytes `+0x16AE`, `+0x16AD`, `+0x16AB`, `+0x16A9`,
   `+0x1701`, `+0x16BF`, `+0x16C0`, `+0x1570`, `+0x16B6`, `+0x16B7`** read in
   `UnitClass__Can_Enter_Cell`'s passability ladder — several map to TS bits
   (tiberium-crusher, vein, etc.). Filter the bridge-relevant branches from
   the TS-legacy ones during decompile of #2.
4. **Uncategorized callers at `0x5FC5F0` / `0x5FC600` / `0x5FC62C`** of
   SetBridgeDirection — possibly save/load or a TS-era editor/debug path.
   **Identify the enclosing function and at least one caller-of-caller**
   before assuming it's a hot retail path.
5. **`SpecialFlags & 0x1000`** (fog) — not encountered in any scoped function,
   but flag if it appears during execution.
6. **CheckBridgeTraversal return code 7** — confirm 7 is the standard A*
   "blocked" code and not a TS-era special value with another meaning.

---

## 8. Current Rust Implementation Surface

(Detailed inventory at end of Section 6, integration block. Summary here:)

- **Diff-1 SlopeIndex:** NOT implemented in pathfinding. `slope_type` exists
  on `ResolvedTerrainCell` (terrain-resolve layer) but is not propagated to
  `PathCell`. A* only handles height-diff 0 and height-diff 4 explicitly
  ([src/sim/pathfinding/core.rs:123-153](../../src/sim/pathfinding/core.rs#L123-L153)).
- **Two-pass Can_Enter_Cell:** PARTIAL (design-different). Rust pre-decides
  layer at A* push-time
  ([src/sim/pathfinding/core.rs:425, 444-451](../../src/sim/pathfinding/core.rs#L425-L451))
  rather than re-reading occupancy at cell-entry. The 2026-05-11 design's
  Tiny-Detail Ledger #11 claims the outputs match for retail cases; this
  investigation must verify that claim.
- **RecalcAttributes runtime write-back:** NOT implemented. All bridge cell
  flags are static-from-map-load
  ([src/map/resolved_terrain.rs:538-599](../../src/map/resolved_terrain.rs#L538-L599)).
  Runtime bridge state is tracked in
  [src/sim/bridge_state/mod.rs](../../src/sim/bridge_state/mod.rs) but does
  not propagate to `PathGrid`.
- **SetBridgeDirection:** NOT implemented. A `Direction` enum exists in
  [src/sim/bridge_state/mod.rs:149-188](../../src/sim/bridge_state/mod.rs#L149-L188)
  but there is no anchor-pattern cell walker. Bit 0x80 is absorbed into the
  boolean `PathCell.bridge_walkable`.

---

## 9. Deferred Open Questions

Questions scoping surfaced but didn't resolve — the investigation must answer
each:

1. **Conflict A** — Which offset(s) hold the SlopeIndex byte read at
   `CheckBridgeTraversal` diff-1? +0x11A or +0x11C? (Or both, in different
   contexts?)
2. **Conflict B** — What does the instruction at `0x47D94E` actually do? Does
   AUDIT_LOG's `MOV [ESI+0x11B], AL` match the disassembly today?
3. **Conflict C** — Definitive write map of RecalcAttributes per cell byte.
4. **Two-pass semantic** — Does the pre-decided `target_layer` in our A*
   produce the *same final pass/block verdict* as gamemd's post-pass-2 result
   for every retail bridge cell configuration?
5. **Runtime mutation of bit 0x80** — Does SetBridgeDirection ever flip bit
   0x80 (or +0x11A/+0x11B) during gameplay, or only at map-init / during
   bridge destruction (which Rust handles separately via `BridgeRuntimeState`)?
6. **Uncategorized callers** — What enclosing functions own `0x5FC5F0`,
   `0x5FC600`, `0x5FC62C`? Are they retail-reachable or save/load/editor-only?
7. **`g_RulesClass+0x664`** — What INI key, what default, retail-relevant?
8. **Diff-1 SlopeIndex retail observability** — Are there retail YR maps
   where unit pathing would observably differ if Rust ignored vs respected
   the diff-1 SlopeIndex check? (Likely none — retail uses bridges with
   the same SlopeIndex convention everywhere — but verify.)

---

## 10. Execution Strategy

**Recommended: Batched subagents, two batches.**

- **Batch 1 (Phase 1, sequential within batch):** Functions #1, #2, #3, #4, #5.
  These resolve the three load-bearing conflicts (A/B/C) and produce the
  definitive byte-offset map. Sequential because #3 and #4 both touch the
  same cell-byte region, and #2's two-pass mechanism depends on understanding
  #1.
- **Checkpoint:** Executor pauses and writes a Phase-1 summary section. User
  reviews. If conflicts unresolved, plan revised.
- **Batch 2 (Phase 2 + 3, parallelizable):** Dispatch the remaining 23
  functions in 4 parallel subagents:
  - Agent X: #6, #7, #12, #13 (RecalcAttributes write-path detail)
  - Agent Y: #8, #9, #10, #11 (Can_Enter_Cell siblings + bridge-Z readers)
  - Agent Z: #14, #15, #16, #17, #18-#23 (NESW/NWSE damage/repair callers)
  - Agent W: #24, #25, #26, #27, #28 (Uncategorized callers + map-init)

A single-session `/re-investigate` is feasible if the executor has a long
context budget, but the function count (28) is on the high side for one pass.

---

## 11. Success Criteria

The executed research document must:

- Resolve **Conflict A** with a disassembly excerpt — name the actual
  offset(s) read by CheckBridgeTraversal at diff-1.
- Resolve **Conflict B** with the disassembly at 0x47D94E.
- Produce a definitive per-byte write map for `CellClass+0x11A..+0x11E`
  during `RecalcAttributes`.
- Decompile #2 (`UnitClass__Can_Enter_Cell`) end-to-end with the two-pass
  state diff documented (what changes between pass 1 and pass 2 reads).
- Enumerate every caller of #4 and #5 with category (map-init / repair /
  damage / save-load / editor / unknown).
- Identify the enclosing function for each of the 3 uncategorized 0x5FC*
  call sites and provide one-line role descriptions.
- For each subsystem (diff-1, two-pass, RecalcAttributes, SetBridgeDirection),
  state **"Active in retail YR: yes / no / conditional"** and **one-line
  observable-impact verdict** ("does a player see this fire").
- Update the [BRIDGE_SYSTEM.md](../../../ra2-rust-game-docs/BRIDGE_SYSTEM.md)
  byte-offset table and §CheckBridgeTraversal section in-place to match
  verified findings (correcting the doc, not just appending to it). Note
  every correction in the AUDIT_LOG.
- State Ghidra addresses for every HIGH-confidence claim; mark MEDIUM/LOW
  where applicable.
- Re-document every deferred question from §9 as either RESOLVED (with
  answer) or UNRESOLVED (with reason).

## Sources

- **Ghidra addresses sampled (light scoping pass):** 0x4D9C60, 0x73F0A0,
  0x47D2B0, 0x47D94E, 0x47d35e, 0x47E040, 0x47E470, 0x47DD70, 0x415B10,
  0x55ABF0, 0x4AF4A0, 0x4B0F20, 0x005471B0, 0x565C10, 0x576BA0, 0x571490,
  0x572400, 0x572800, 0x572D00, 0x573200, 0x576200, 0x56EFD0, 0x56F370,
  0x56F940, 0x56FD10, 0x570AE0, 0x5FC5F0, 0x5FC600, 0x5FC62C.
  Vtable DATA xrefs: 0x7E2454, 0x7E8E44, 0x7EB208, 0x7F5E20.
- **Docs searched:** BRIDGE_SYSTEM.md, [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) (2026-05-11 entries),
  [UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md), CELLCLASS_STRUCT_GHIDRA_REPORT.md,
  [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md), [LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md),
  [PATHFINDERCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATHFINDERCLASS_GHIDRA_REPORT.md), HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md.
- **In-repo plans/gap-scans:**
  docs/plans/2026-05-11-bridge-locomotor-layer-correctness-design.md,
  docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md.
- **INI files checked:** ini/rulesmd.ini (no direct keys; `g_RulesClass+0x664`
  to be located by xref during execution).
- **Related plans:**
  - 2026-05-11-bridge-locomotor-layer-correctness — the parent plan whose
    "Known Parity Boundary" this investigation finishes.
  - 2026-05-06-bridges-tier1-ini-parser — the INI-side bridge work (already
    done; not affected by this).
  - 2026-05-10-asset-parsing-bridges-investigation-plan — the TMP/IsoTile
    side; complementary, not overlapping.
