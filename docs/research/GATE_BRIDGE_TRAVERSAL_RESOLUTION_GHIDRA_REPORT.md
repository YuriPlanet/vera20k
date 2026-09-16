# Gate Resolution: Bridge A4 — CheckBridgeTraversal + Warhead `Wall` (+0x144) + vtable +0x1B0

**Date:** 2026-06-04
**Scope:** Close three linked bridge items for plan P3 (traversal-legality gate) and the `+0x144`/`+0x1B0` inventory entries.
**Mode:** Reverse-engineering research only. Ghidra MCP read-only. No Rust changed.
**Verdict:** **CLOSED** (all three items resolved with unambiguous binary evidence).
**Active in YR:** All three findings are live in retail YR (see TS-legacy note §4).

> Every fact below cites the exact MCP call inline. Authority order: binary → Ghidra → docs.

---

## 0. TL;DR

1. **CheckBridgeTraversal @ 0x004D9C60** — identity confirmed; full decision table verified from the decompile AND the assembly. It is the cell-to-cell bridge-height traversal validator returning `0`=OK / `7`=Blocked, dispatched via vtable slot `+0x1B0`. Decision table in §1.
2. **Warhead +0x144 = `Wall=` boolean.** The prior docs' "tentative Wall" label is now **CONFIRMED**. Parsed by `CCINIClass__ReadBool` from the INI key string `"Wall"` at `0x0081AC58`; read in combat by `Apply_area_damage` for BOTH overlay-wall destruction and as the per-warhead half of the bridge-destruction gate. §2.
3. **vtable +0x1B0** is exactly the slot that holds `CheckBridgeTraversal` for the Foot/Unit/Infantry vtables. Verified by reading the slot words live. §3.

---

## 1. Item (1): CheckBridgeTraversal @ 0x004D9C60 — traversal-legality gate

### 1.1 Identity (confirmed this session)

`get_function_by_address 0x004D9C60` returns `CheckBridgeTraversal`, body `0x004D9C60–0x004D9E66`.
`decompile_function 0x004D9C60` and `disassemble_function 0x004D9C60` confirm the body matches the
documented validator (RET 0x14 at every exit, no other behavior). This is not a stale/polluted label:
the body's structure (direction reconstruction, `-1` branch, diff 0/1/4 switch, `*param_4=1` write,
return 7/0) is internally consistent with the "bridge traversal validator" role.

### 1.2 Signature (verified `disassemble_function 0x004D9C60`, entry block)

```c
undefined4 __stdcall CheckBridgeTraversal(   // stdcall via vtable dispatch; RET 0x14
    CellClass* candidate,      // arg1 (EDI) — the target cell being entered
    int        direction,      // arg2 (EBX) — 0..7, or -1 = "no direction"
    int*       height_in_out,  // arg3 (EBP) — path height, INPUT/OUTPUT, -1 = unset
    uint8_t*   bridge_entered,  // arg4 — output flag, set to 1 only on ascend-onto-deck
    CellClass* parent_or_null  // arg5 (ESI) — predecessor/current cell; may be 0
);
```
Entry: `004d9c61 MOV EBX,[ESP+0xc]` (dir), `004d9c67 MOV ESI,[ESP+0x20]` (parent),
`004d9c6c MOV EDI,[ESP+0x14]` (candidate). Returns `0` (OK) or `7` (Blocked).

### 1.3 Decision table (verified — decompile + asm `0x004D9C60`)

Cell field offsets used: `+0x140` Flags (`0x100`=on-bridge, `0x200`=bridgehead),
`+0x11B` Level (signed, MOVSX), `+0x11C` SlopeIndex (ramp gate). `g_DirectionOffsets @ 0x0089F688`.

| # | Precondition | Action / result | Asm anchor |
|---|---|---|---|
| A | `parent==0` (any dir) | Reconstruct predecessor: `parent = Get_CellClass(candidate.coord + g_DirectionOffsets[(dir-4)&7])`. `(dir-4)&7` = 180° rotation. | `004d9c70`–`004d9cba` (`CALL 0x005657a0` = MapClass::Get_CellClass) |
| B | `dir == -1` | Candidate-only seed: if `*height==-1 && candidate.Flags&0x100` → `*height = candidate.Level+4`. **Return 0.** No bridgehead/diff/slope checks. Parent (even reconstructed) is ignored. | `004d9cbc JZ 0x004d9e3e` → `004d9e3e`–`004d9e64` |
| C | `dir!=-1`, parent==0 OR candidate==0 (after recon) | Return 0 (OK, no checks). | `004d9cc5`/`004d9ccd JZ 0x004d9e5e` |
| D | `*height==-1` & `parent.Flags&0x100` (directed seed) | `*height = parent.Level+4`; then if `candidate.Flags&0x200==0` → **return 7** (can't enter mid-span; must use a bridgehead). | `004d9cd9`–`004d9d05` |
| E | compute `diff = sel - candidate.Level`, where `sel = parent.Level` if `parent.Flags&0x100` else `*height`; `diff_abs = abs(diff)` | switch below | `004d9d0e`–`004d9d3e` |
| E0 | `diff_abs==0` (level move) | Block (**7**) iff `(candidate NOT bridge OR candidate NOT bridgehead OR parent NOT bridge)` AND `*height!=-1` AND `*height!=candidate.Level`. Else OK. | `004d9e08`–`004d9e3b` |
| E1 | `diff_abs==1` (ramp) | If `diff<1`: block iff `parent.SlopeIndex(+0x11C)==0`. Else: block iff `candidate.SlopeIndex==0`. | `004d9dd8`–`004d9e05` |
| E4a | `diff_abs==4`, `parent.Level==candidate.Level-4` (candidate HIGH) | Block (**7**) if `*height!=candidate.Level`; block if parent NOT bridge (`sel-flag 0x100==0`). | `004d9d5c`–`004d9d8c` |
| E4b | `diff_abs==4`, `candidate.Level==parent.Level-4` (candidate LOW) | Block if `candidate.Flags&0x100==0`; block if `candidate.Flags&0x200==0`; else set `*bridge_entered=1`, **return 0**. | `004d9d8f`–`004d9dd5` |
| E* | `diff_abs` ∈ {2,3,5,6,7,…} | **Return 7** (hard block). | `004d9d4e JZ` fallthrough → `004d9d53 MOV EAX,0x7` |
| F | fallthrough | Return 0. | `004d9e5e`–`004d9e64` |

**Parent/current-cell `0` semantics (the gate's explicit question):**
- `parent==0` is **not** "use the mover's current cell." With a valid direction it reconstructs the
  *predecessor of the candidate edge* via `(dir-4)&7` (row A) — for lookahead probes this is the
  previous edge cell, not necessarily the unit's occupied cell.
- `parent==0` with `dir==-1` uses **candidate-only** height seeding (row B) and skips all directed
  checks. The reconstruction in row A still runs (wasted `Get_CellClass` using pseudo-dir `(-1-4)&7=3`)
  but its result is unused by row B.
- A* passes an **explicit** parent (current node cell) + real path height — explicit-parent directed
  traversal, never the null fallback (per [BRIDGE_CHECK_TRAVERSAL_PARENT_FALLBACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CHECK_TRAVERSAL_PARENT_FALLBACK_GHIDRA_REPORT.md),
  A* callsite `0x00429F54`). Runtime locomotors (Drive/Ship/Hover) pass `parent=0`.

**`-1` candidate height-seed (the gate's explicit question):** rows B and D both seed `Level+4`
("bridge deck = ground + 4"), but from **different cells**: `dir==-1` seeds from the *candidate*
(row B, no bridgehead requirement); directed `*height==-1` seeds from the *parent* and then *requires*
the candidate to be a bridgehead `0x200` (row D). These must be modeled separately in Rust.

---

## 2. Item (2): Warhead +0x144 = `Wall=` — CONFIRMED (was tentative)

### 2.1 Parser write site (`decompile_function 0x0075D3A0`, WarheadTypeClass::ReadINI_Body)

```c
*(undefined1 *)(param_1 + 0x144) = CCINIClass__ReadBool(section, key=&DAT_0081ac58, default);
```
The key-string operand is `&DAT_0081AC58`. `read_memory 0x0081AC58` → bytes
`57 61 6C 6C 00` = ASCII **`"Wall"`**. So **warhead+0x144 = the `Wall=` boolean**, parsed by
`CCINIClass__ReadBool`. It sits between `Conventional` (+0x14D) and `WallAbsoluteDestroyer` (+0x145,
string `"WallAbsoluteDestroyer"` confirmed via `read_memory 0x00847E1C`). The "tentative" qualifier in
the prior bridge doc is removed.

`get_field_access_context 0x0081AC58` shows the same `"Wall"` literal is also pushed by
`BuildingTypeClass__ReadINI` (`PUSH 0x81ac58 @ 0x0046049F`) — i.e. `"Wall"` is a shared INI-key string
literal (building `Wall=` flag); the *warhead* consumer is the +0x144 write at `0x0075D3A0`.

### 2.2 Default value

`decompile_function 0x0075CEC0` (WarheadTypeClass::Constructor) does **not** explicitly write +0x144
(it zeros the neighbors +0x145/+0x146/+0x147). The type-class allocation is zero-initialized before
the ctor and `ReadBool` returns 0 when the key is absent, so the effective default is **`Wall=false`**.
(INI evidence: stock `ini/rulesmd.ini` sets `Wall=yes` on specific warheads — e.g. lines 12031, 16388 —
so it is opt-in per warhead.)

### 2.3 Combat read site (`decompile_function 0x00489280`, Apply_area_damage) — two roles

Warhead+0x144 (`param_4 + 0x144`) is read in two distinct places:

**(a) Overlay-wall destruction.** When a cell's overlay is a wall type (`OverlayTypeClass+0x2A8 != 0`):
```c
if (overlayType->+0x2A8 != 0 &&
    (warhead->+0x145 /*WallAbsoluteDestroyer*/ != 0 || warhead->+0x144 /*Wall*/ != 0 ||
     (warhead->+0x147 != 0 && overlayType->+0x9C == 6)))
    CellClass__DestroyOverlay();
```

**(b) Bridge-destruction gate (the bridge-relevant role).**
```c
if (((*g_ScenarioClass_Instance & 0x8000) == 0) || (warhead->+0x144 /*Wall*/ == 0))
    goto LAB_0048a2c4;   // SKIP all AoE bridge-collapse sub-blocks
```
i.e. AoE warhead damage collapses high/low/wood/concrete bridges only when the warhead has
`Wall=yes` AND the scenario `DestroyableBridges` SpecialFlag (bit 0xF / 0x8000 of
`ScenarioClass+0x000`) is set. This corroborates [DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md)
exactly. (Bridge Repair Hut C4/demo-truck collapse is NOT gated by either flag — that doc §5.)

---

## 3. Item (3): vtable +0x1B0 — the bridge-relevant slot

The "+0x1B0" slot **is** CheckBridgeTraversal. Verified by reading the slot word live:

- Unit vtable+0x1B0 = `read_memory 0x007F5E20` → `60 9c 4d 00` = **0x004D9C60** (CheckBridgeTraversal).
- Infantry vtable+0x1B0 = `read_memory 0x007EB208` → `60 9c 4d 00` = **0x004D9C60**.
- Foot vtable+0x1B0 = `read_memory 0x007E8E44` → `60 9c 4d 00` = **0x004D9C60**.
- `get_xrefs_to 0x004D9C60` → DATA refs from `0x007E8E44`, `0x007EB208`, `0x007F5E20` (the three
  slots) plus `0x007E2454` (a fourth Foot-derived vtable slot word; no code xref — a sibling vtable).

It is **NOT** CheckBridgeTraversal for Aircraft/Building vtables (their +0x1B0 is DrawIt / an Anim
stub, per the prior doc §1) — consistent: aircraft and buildings do not do bridge-deck traversal.
`Can_Enter_Cell` reaches this slot via `CALL [EAX+0x1B0]` (Unit `0x0073F2EB`, Infantry `0x0051C0E6`),
forwarding `Can_Enter_Cell` arg4 (parent/current cell) as `CheckBridgeTraversal` arg5.

---

## 4. YR-active vs TS-legacy

All three items are **live in retail YR**, not TS-legacy:
- `CheckBridgeTraversal` is on the Foot/Unit/Infantry vtables reached by every ground-unit
  `Can_Enter_Cell` (A* and runtime locomotion) — core pathing, fires constantly.
- `Wall=` is read on every AoE detonation via `Apply_area_damage` (the central AoE dispatcher).
- The bridge-destruction sub-blocks it gates are wrapped by the `DestroyableBridges` SpecialFlag,
  which defaults **on** in skirmish (per [DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md)).
No subterranean/tunnel or off-by-default-flag dead path is involved in any of the three.

---

## 5. Rust handoff

- **Plan P3 (traversal-legality gate):** Implement `CheckBridgeTraversal` as a predicate with the
  binary-shaped signature `(candidate, direction: i32 incl -1, height: &mut i32 incl -1,
  bridge_entered: &mut bool, parent: Option<Cell>)` and the §1.3 decision table verbatim. Critical
  invariants: `parent==None && dir!=-1` reconstructs predecessor via `(dir-4)&7` (NOT "use current
  cell"); `parent==None && dir==-1` uses candidate-only seeding and skips directed checks; directed
  `height==-1` seeds from the parent deck and requires candidate bridgehead `0x200`; only `diff_abs ∈
  {0,1,4}` are legal; `bridge_entered` is set **only** in the ascend case (E4b). Wire it on the
  ground-unit `can_enter_cell` path; keep A* passing an explicit parent + real path height.
- **`+0x144` inventory entry:** Record `WarheadTypeClass +0x144 = Wall (bool, default false)`, parsed
  from INI key `"Wall"`. Rust warhead parsing must read `Wall=` and use it (a) to allow overlay-wall
  destruction and (b) as the per-warhead half of the bridge-AoE gate `(DestroyableBridges &&
  warhead.wall)`. Do not conflate with `WallAbsoluteDestroyer` (+0x145).
- **`+0x1B0` inventory entry:** Record vtable slot `+0x1B0 = CheckBridgeTraversal (0x004D9C60)` for
  Foot/Unit/Infantry; Aircraft/Building override it with non-bridge functions. In Rust this is just the
  bridge-traversal predicate dispatched from ground-unit cell-entry — no vtable plumbing needed, but
  the dispatch must be ground-unit-only (aircraft/buildings skip it).

---

## 6. Sources (all this session unless noted)

- `get_function_by_address 0x004D9C60`, `decompile_function 0x004D9C60`, `disassemble_function 0x004D9C60` — CheckBridgeTraversal identity + decision table.
- `read_memory 0x007F5E20 / 0x007EB208 / 0x007E8E44` = `0x004D9C60`; `get_xrefs_to 0x004D9C60` — +0x1B0 slot occupancy.
- `decompile_function 0x0075D3A0` — warhead parser, +0x144 ReadBool with key `&DAT_0081AC58`.
- `read_memory 0x0081AC58` = `"Wall"`; `read_memory 0x00847E1C` = `"WallAbsoluteDestroyer"`.
- `decompile_function 0x0075CEC0` — constructor (no explicit +0x144 write → default false).
- `decompile_function 0x00469080`→`0x004690B0` WarheadTypeClass::Detonate (dispatches Apply_area_damage); `decompile_function 0x00489280` Apply_area_damage — +0x144 read sites (overlay + bridge gate).
- `get_field_access_context 0x0081AC58` — `"Wall"` also used by `BuildingTypeClass__ReadINI @ 0x0045FE50`.
- INI: `ini/rulesmd.ini` `Wall=yes` lines (12031, 16388, …).
- Prior docs (extended, not redone): [bridges/03-traversal-pathfinding-entry/BRIDGE_CHECK_TRAVERSAL_PARENT_FALLBACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CHECK_TRAVERSAL_PARENT_FALLBACK_GHIDRA_REPORT.md), [BRIDGE_CHECK_TRAVERSAL_AND_CELL_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_CHECK_TRAVERSAL_AND_CELL_OFFSETS_GHIDRA_REPORT.md), [bridges/05-damage-collapse-repair-cabhut/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md).
