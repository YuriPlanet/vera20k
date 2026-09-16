---
title: Cloak FX System — Investigation Plan
status: awaiting approval
---

# Cloak FX System — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass for the cloak rendering
> path. Execute by running `/re-investigate cloak FX system` with this plan loaded
> as context. Phase 1 is **verification + gap-fill** against the existing
> HIGH-confidence reports; Phase 2 unblocks the unknowns that prevent the shader
> `apply_fx()` bit-0 branch from being filled in; Phase 3 is integration + edges.
> Pause and summarize after each phase.

**Topic:** Cloak FX rendering path in gamemd.exe — the observable per-pixel
output produced during cloaking, fully-cloaked, uncloaking, fully-uncloaked,
Mirage-tree-disguise, and allied-shimmer-pulse states; including the inputs
needed to populate `SpriteInstance.fx_flags` bit 0 and `fx_params[0]`
(currently a no-op stub in [sprite_voxel_shader.wgsl:96-109](src/render/sprite_voxel_shader.wgsl#L96-L109)).

**Scope Size:** Medium — ~20 functions, 8 INI keys, 2 phases of unknowns plus a
shader-bridge synthesis step.

**Est. Effort:** ~6-9 hours of `/re-investigate` work
(~15-30 min × 3 FULL functions, ~5-10 min × 10 MEDIUM, ~2-5 min × 7 LIGHT, plus
1-2h on the per-pixel-alpha-bridge synthesis).

**Prior Research:**

- [CLOAKING_VISUAL_PIPELINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_VISUAL_PIPELINE.md) (1257 lines, HIGH) — dedicated visual pipeline,
  state→blitter mapping, **per-pixel blend formulas** for shimmer/50%/25%
  blitters, allied-shimmer pulse cycle, VXL draw entry. **THE primary
  reference.**
- [CLOAKING_STEALTH_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_STEALTH_SYSTEM_GHIDRA_REPORT.md) (902 lines, HIGH) — consolidating
  master report; covers state machine, triggers, decloak triggers, detection,
  gap generator, disguise, visual rendering, struct fields, INI keys.
- [CLOAKING_INTERACTIONS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_INTERACTIONS_REPORT.md) (356 lines, HIGH) — transport / chronoshift
  / mind control / disguise interactions with cloak.
- [DISGUISE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISGUISE_SYSTEM_GHIDRA_REPORT.md) (1117 lines, HIGH, with MEDIUM detection
  consumer) — full Mirage tree-disguise mechanics, Spy disguise, GetDisplayType
  /GetDisplayOwner rendering hooks.
- [SENSOR_CLOAK_DETECTION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SENSOR_CLOAK_DETECTION.md) (319 lines, HIGH) — cloak vs sensors detection.
- [BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md) (632 lines, HIGH) — building
  cloak generators (mostly TS-legacy in YR retail).
- [SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md) (434 lines) — spy effects on
  buildings (post-disguise; not the disguise itself).

**Conflicts / discrepancies between reports:** None found between the cloak
reports themselves. **One discrepancy surfaced during scoping** (see Section 9
Open Question #1): the address `0x006FB740` is cited in `CLOAKING_STEALTH_*`
and `CLOAKING_VISUAL_*` as `CloakingTick`, but the body at that address as
read in the scoping pass dispatched via vtable on TechnoClass+0x220..+0x238
and did not present as a self-contained cloak state machine. **Verifying this
address is Step 1 of Phase 1** — if wrong, every downstream finding that
references it must be re-anchored.

**Expected Output:** A new research document at
`docs/research/CLOAK_FX_SHADER_BRIDGE_GHIDRA_REPORT.md`
that:
1. Verifies (or corrects) the prior reports' load-bearing addresses, formulas,
   and field offsets, and notes verification-confirmed claims explicitly.
2. Closes the 6 specific open questions in Section 9.
3. Produces the **shader-bridge recipe** in Section 2 of the Goal — a complete
   per-state mapping from runtime cloak state to (fx_flags bit 0, fx_params[0],
   sprite-key override).
4. Notes per-finding "Active in YR: Yes / No / Conditional" per CLAUDE.md.

**Next Pipeline Step:** `/brainstorm cloak FX Rust integration` (sim component
shape + render-side population + Mirage sprite-swap mechanism), then
`/write-plan` for Phase 2 of the voxel-gpu-remap-fx work.

---

## 1. Goal

When this investigation finishes, the resulting research document must answer:

1. **The shader-bridge recipe.** For every runtime `(CloakState, CloakProgress,
   IsPlayerControlled, ViewerHouseRelation)` tuple, what `(fx_flags bit 0,
   fx_params[0])` does the gamemd-equivalent fragment shader need, and when
   does the unit need a different sprite key entirely (Mirage tree)?
2. **The per-stage pixel blend formula.** Is the cloak-fade transition
   reproducible as a flat per-instance alpha multiply (current shader stub),
   or does parity require reproducing gamemd's dithered intensity-table
   pattern? If dither, what is the table and the sampling pattern?
3. **The allied shimmer pulse cycle.** What is the exact tick-driven phase
   function `phase(g_CurrentFrameCounter, +0x1DC) → (shimmer | 50%-blend |
   opaque)` for player-controlled cloaked units, and is the phase tied to the
   game tick (deterministic) or wall-clock (render-time)?
4. **The Mirage tree-disguise hand-off.** Under what exact conditions does a
   Mirage Tank render as an OverlayType tree sprite vs render as the actual
   VXL with cloak-FX applied? Is this a render-time sprite swap or a
   sim-state swap? Does the cloak-fade animation play under the disguised
   sprite, or only on the un-disguised one?
5. **The `+0x1DC` aliasing question.** Is `TechnoClass+0x1DC` the same field
   read by `ModifyCloakDrawFlags` as the allied-shimmer phase base AND
   written by `UnitClass::TurretAI` as the Mirage disguise frame counter? If
   so, what is the dual-use contract — does Mirage cloak-shimmer therefore
   reset every time the disguise is picked, by design?
6. **Address verification.** Are the addresses cited in [CLOAKING_VISUAL_PIPELINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_VISUAL_PIPELINE.md)
   (specifically the `CloakingTick` address `0x006FB740`) correct?

The shader-bridge recipe (item 1) is the **primary deliverable**. Items 2-6
exist to make item 1 trustworthy.

---

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| [CLOAKING_VISUAL_PIPELINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_VISUAL_PIPELINE.md) | State→visual_state→blitter→pixel blend; allied shimmer; VXL draw entry; complete timeline | HIGH | Intensity LUT layout for shimmer blitter; a-buffer semantics; whether VXL state-4 0x200A/0x200C bits produce a distinct blend ratio downstream or just brightness modulation; `CloakingTick` address may be wrong |
| [CLOAKING_STEALTH_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_STEALTH_SYSTEM_GHIDRA_REPORT.md) | Master overview; state machine; triggers; decloak triggers; detection; gap gen; disguise rendering | HIGH (MEDIUM on disguise rendering only) | Repeats `0x006FB740` as `CloakingTick` — same verification risk |
| [CLOAKING_INTERACTIONS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_INTERACTIONS_REPORT.md) | Transport, chronoshift, mind-control, disguise cross-effects | HIGH | None FX-specific |
| [DISGUISE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISGUISE_SYSTEM_GHIDRA_REPORT.md) | Mirage tree-disguise mechanics; tree-pick site; damage-breaks; rendering observer (GetDisplayType / GetDisplayOwner) | HIGH | Whether the cloak-fade animation runs on the disguised (tree) sprite or only the un-disguised one; relative tick-order of TurretAI vs CloakingTick |
| [SENSOR_CLOAK_DETECTION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SENSOR_CLOAK_DETECTION.md) | Sensor sight, sensor arrays, psychic detection, disguise detection | HIGH | None FX-specific (detection affects visibility, not pixel blend) |
| [BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md) | Building cloak generators | HIGH | TS-legacy in YR retail — likely out of scope for Phase 2 |
| [SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md) | Spy post-infiltration effects | HIGH | Out of scope (not rendering) |

**Net coverage:** ~80% of cloak FX rendering is already documented at HIGH
confidence. The remaining 20% is the load-bearing items in Section 9.

---

## 3. Function Inventory

Numbered list of every function the `/re-investigate` pass must touch, grouped
by execution phase. Phase 1 produces a usable skeleton; Phase 2 fills depth;
Phase 3 closes context + edge cases.

| # | Phase | Address | Current Name | Scope Reason | Depth Target | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|--------------|----------------|
| 1 | 1 | `0x006FB740` | `CloakingTick` (per prior doc) | **VERIFY address.** Scoping pass found body did not match a cloak state machine. If wrong, find the true CloakingTick address. | FULL | Low |
| 2 | 1 | `0x00703860` | `TechnoClass_GetVisualState` | Re-verify the CloakProgress → visual_state 0..5 formula and the `Invisible=` (TechnoTypeClass+0xC9A) early-exit branch | FULL | Low — `Invisible=` is YR-valid; verify no SpecialFlags gate inside |
| 3 | 1 | `0x00706640` | `TechnoClass__Draw` (VXL path) | Verify flag-encoding switch (`0x2000` base + `0x200A`/`0x200C` for state-4); follow flag propagation through `param_11` | MEDIUM | Low |
| 4 | 1 | `0x00705E00` | `TechnoClass_DrawSHP` | Verify SHP flag-encoding switch; baseline for VXL comparison | LIGHT — already well-covered, just confirm | Low |
| 5 | 1 | `0x0070ED80` | `ModifyCloakDrawFlags` (vtable+0x43C) | Verify allied-shimmer phase function exactly: `phase = (g_CurrentFrameCounter - this+0x1DC + 0x40) & 0xFF` and the 4 phase bands. Locate WRITERS of `+0x1DC`, `+0x1EC`, `+0x1F4` (Agent D could not via byte-pattern). | FULL | Low |
| 6 | 1 | `0x00490B90` | `Blitter_selector` | Locate the static intensity-table base that gets stored into `BlitterInfo+8` and the remap-palette base at `BlitterInfo+4`. These are referenced by the shimmer/50%/25% blitters but the LUT itself is per-instance, sourced from this dispatcher. | MEDIUM | Low |
| 7 | 1 | `0x00494330` | `Shimmer_blitter` (75/25) | Re-confirm formula `intensity = clamp((param * 261) >> 11, 0, 254); alpha = intensity_table[intensity * 512 + a_buffer_pixel]; src = remap_palette[(alpha \| pixel_value) * 2]; *dest = (src >> 2 & mask) * 3 + (*dest >> 2 & mask)`. Extract the meaning of `param`, `a_buffer_pixel`, and where the dither pattern comes from. | FULL | Low |
| 8 | 1 | `0x00497CF0` | `ZBuf_50pct_blend` | Verify `*dest = (src >> 1 & mask) + (*dest >> 1 & mask)` — flat 50/50 | LIGHT | Low |
| 9 | 1 | `0x00494080` | `ZBuf_25pct_blend` | Verify `*dest = (src >> 2 & mask) + (*dest >> 2 & mask) * 3` — 25/75 | LIGHT | Low |
| 10 | 2 | `0x007468C0` | `UnitClass::TurretAI` | Mirage tree-pick site. Verify the field writes at `+0x1D8` (disguise-active), `+0x1DC` (frame counter — **same offset as cloak phase base?**), tree TypeClass at `+0x518`. Determine whether `+0x1DC` is shared with the cloak phase base or is a parsing artifact. | FULL | Low — disguise is YR-active on MGTK and SPY |
| 11 | 2 | _TBD_ | Mirage display selector | `DISGUISE_SYSTEM` doc names `GetDisplayType` (verify address) and `GetDisplayOwner` — find the call sites in `TechnoClass__Draw` / `TechnoClass__DrawSHP` that decide "render as tree" vs "render as VXL+FX". Verify whether the cloak-fade flag bits propagate through GetDisplayType or are bypassed for tree sprites. | MEDIUM | Low |
| 12 | 2 | `0x00703770` | `TechnoClass::StartCloaking` (vtable+0x460) | Verify the +0x1DC / +0x1EC / +0x1F4 field initialization sequence — likely this is where the writers live that Agent D's byte-pattern search missed. | MEDIUM | Low |
| 13 | 2 | `0x007036C0` | `TechnoClass::StartUncloaking` (vtable+0x45C) | Same — verify field-reset writers | LIGHT | Low |
| 14 | 2 | `0x004D3780` | `TechnoClass::DoCloak` | Auto-cloak transition entry — check if it touches +0x1DC | LIGHT | Low |
| 15 | 2 | `0x006F4EB0` | `TechnoClass::DoUncloak` | Forced uncloak entry — check if it touches +0x1DC/+0x1EC | LIGHT | Low |
| 16 | 2 | `0x006691E0` (entry) | `RulesClass::ReadAudioVisual` | Locate `CloakSound` parse at xref `0x0066A6FA`. Confirm field offset on RulesClass (Agent D reported `+0x6A0`, requires `param_1 type` check per CLAUDE.md decompilation pitfall). | LIGHT | Low |
| 17 | 2 | _TBD_ (function containing `0x0066F146`) | `RulesClass::ReadGeneral` (or similar) | Locate `CloakingStages` parse at xref `0x0066F146`. Confirm field offset (Agent D reported `[0x628]` indexed which translates to **byte offset 0x628 if `param_1` is `int`** OR **byte offset 0x1898 if `param_1` is `int*`** — must verify per CLAUDE.md `param_1` pitfall). | LIGHT | Low |
| 18 | 3 | _TBD_ | `VXL_CacheBlit` and/or `FUN_006C89E0` | Trace where bits `0x200A`/`0x200C` (VXL state-4 brightness variant) are decoded downstream of `TechnoClass__Draw`. Determine whether this resolves to a distinct blitter or just brightness modulation on the lit rasterizer. **Cross-check against [VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md) §2 — the live YR dispatch entries (4/5/6/7) don't enumerate a "brightness variant".** Possible TS-legacy dead path; verify before implementing. | MEDIUM | **HIGH — confirm the brightness variant is reachable from a normal YR voxel cloak frame** |
| 19 | 3 | `0x006FC0B0` | `TechnoClass::GetFireError` (DecloakToFire path) | Confirms the decloak-on-fire trigger; cross-reference into per-weapon `DecloakToFire=` INI key (already parsed in Rust per Agent C) | LIGHT | Low |
| 20 | 3 | _TBD_ | `TechnoClass::AI` (caller of #1) and `UnitClass::AI` (caller of #10) | Determine per-tick invocation order: does CloakingTick run before or after UnitClass::TurretAI? This decides whether Mirage tree-pick and cloak-fade can ever conflict on +0x1DC mid-tick. | LIGHT | Low |
| 21 | 3 | _TBD_ | `TechnoTypeClass::ReadINI` near `Cloakable` / `CloakingSpeed` / `CloakStop` | Confirm field offsets for the 3 per-type cloak INI keys; verify `param_1` type for the decompilation pitfall. Spot-check default values (CLOAKABLE is default=no). | LIGHT | Low — keys are YR-active on SUB/DLPH/SQD |

**Total: 21 inventory items** (1 unresolved address pair counts as one slot
each in #11, #17, #18, #20). Solidly in the 8-30 normal band.

**Phase boundaries:**

- **Phase 1 checkpoint (after #1-9):** must produce verified addresses + the
  3 blitter pixel formulas + the visual_state mapping + the allied-shimmer
  phase function. If at this point the `CloakingTick` address (#1) is wrong
  and the +0x1DC writer chain (#5) is still unresolved, **revise this plan
  before starting Phase 2**.
- **Phase 2 checkpoint (after #10-17):** must produce the Mirage tree-disguise
  vs cloak-fade hand-off contract and confirm whether +0x1DC is shared
  between disguise and cloak. INI key offsets confirmed with param_1 typing
  noted.
- **Phase 3 (after #18-21):** edge cases — VXL brightness variant disposition,
  decloak-on-fire trigger, tick-order, per-type INI verification.

---

## 4. Detail Checklist

Per CLAUDE.md, parity is on observable output. Extract every detail that
determines a player-visible pixel, a tick-deterministic state change, or a
sprite-swap decision.

### Magic numbers and constants

- **`0x40` phase offset** in `ModifyCloakDrawFlags`: `phase = (frame_counter - this+0x1DC + 0x40) & 0xFF`. Confirm 0x40 is hard-coded; not from INI.
- **Phase bands** in allied shimmer: `< 0x40`, `0x40..=0x43`, `< 0x4C`, `0x4C..=0x4F`, `< 0x70`, `0x70..=0x73`, `< 0x7C`, `0x7C..=0x7F`. Confirm each boundary exactly from the disassembly.
- **`261 >> 11` shimmer intensity scaling**: `intensity = clamp((param * 261) >> 11, 0, 254)`. Where does `param` originate (function argument? blitter state?) and what range does it cover?
- **256-multiplier for visual-state computation**: `visual = (int)((double)CloakProgress / (double)CloakingStages * 256.0)`. Confirm it's a `double` (not float) and the rounding behavior of `(int)` cast (truncate-toward-zero).
- **Visual-state thresholds** `0x40`, `0x80`, `0xC0`, `0xFF` for cloaking animation (already documented; verify).
- **`CloakingStages` default 9** ([General] line 323 of rulesmd.ini, per Agent B). Confirm default value in the RulesClass ReadInt call (often the third arg).
- **`CloakingSpeed` per-type**: 1 for SUB/DLPH, 5 for SQD. Default value on TechnoTypeClass — find in the ReadInt call.

### Bit flags and masks

- **`fx_flags` bit 0 mapping**: the shader is given a `u32` flag. Produce the
  complete `runtime_state → fx_flags_bit_0_set?` truth table.
- **VXL draw flags** `0x2000` (base), `0x2002` (shimmer/z-read), `0x2004`
  (50% blend), `0x200A`, `0x200C`, `0x800` (remap), `0x4000` (mirror), `0x04`
  (warping), `0x06` (combined). Confirm each in `TechnoClass__Draw`.
- **SHP draw flags** `0x02`, `0x04`, `0x06`, `0x08`, `0x20`, `0x800`. Same.
- **`Blitter_selector`'s `flags & 6` dispatch**. Reproduce the four-row dispatch
  table from [CLOAKING_VISUAL_PIPELINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_VISUAL_PIPELINE.md) §"Blitter Selection" exactly,
  including the `+0x3000` and `+0x08` variants.
- **CloakState enum**: 0=Uncloaked, 1=Cloaking, 2=Cloaked, 3=Uncloaking. Verify
  no 4th state exists.

### State machine states

- Confirm the 4 CloakState values and the transition graph: `0 → 1` (auto-cloak
  eligible OR forced); `1 → 2` (CloakProgress reaches CloakingStages); `2 → 3`
  (decloak trigger fired); `3 → 0` (CloakProgress reaches 0). Confirm no
  `2 → 1` or `0 → 3` shortcut transitions.
- Visual state 0..5 mapping (5 = "skip draw entirely"). Confirm state-5 means
  no draw at all, not "draw with alpha 0" — this affects whether the shader
  uses `discard` or alpha=0 (sort order matters).
- Allied shimmer states: opaque / shimmer / 50%-blend, cycled by phase
  bands. Confirm there is no 4th band.

### INI keys to verify

| Key | Section | Default | Per-instance Field | Verify? |
|-----|---------|---------|--------------------|---------|
| `CloakingStages` | [General] | 9 | RulesClass + offset TBD | YES (#17) |
| `CloakSound` | [AudioVisual] | NavalUnitEmerge | RulesClass + offset TBD (Agent D suggested +0x6A0) | YES (#16) |
| `DefaultMirageDisguises` | [General] | TREE01..TREE04 | RulesClass + offset TBD | LIGHT — already in Agent B; verify offset via `/re-investigate` if needed |
| `Cloakable` | per-type | no | TechnoTypeClass+0xCD0 (already verified) | LIGHT (#21) |
| `CloakingSpeed` | per-type | TBD | TechnoTypeClass+0x310 (already verified) | LIGHT (#21) |
| `CloakStop` | per-type | TBD | TechnoTypeClass+0xC93 (already in CLOAKING_VISUAL_*) | LIGHT — confirm default |
| `Invisible` | per-type | no | TechnoTypeClass+0xC9A | LIGHT — Agent D confirmed reader site in GetVisualState |
| `DisguiseWhenStill` | per-type | no | TechnoTypeClass+0xD31 (already verified) | LIGHT |
| `PermaDisguise` | per-type | no | TechnoTypeClass+0xD30 | LIGHT |
| `CanDisguise` | per-type | no | TechnoTypeClass+0xD2F | LIGHT |
| `DecloakToFire` | per-weapon | yes (?) | WeaponTypeClass + offset (Rust parses; verify) | LIGHT (#19) |

### Struct offsets to extract

For each, note `param_1` type (`int` vs `int *`) per CLAUDE.md decompilation
pitfall. Pre-existing offsets from the reports — to be VERIFIED, not
re-derived:

**TechnoClass (instance):**
- `+0x220` CloakState (DWORD)
- `+0x224` CloakProgress (DWORD)
- `+0x228` CloakDirty (BYTE)
- `+0x22C..+0x237` CloakStepTimer (CDTimer)
- `+0x238` CloakingSpeed (DWORD)
- `+0x23C` CloakStepDelta (DWORD)
- `+0x240..+0x24B` ReCloakDelayTimer (CDTimer)
- `+0x269` CloakShroudActive (BYTE)
- `+0x26C` CloakShroudRadius (DWORD)
- `+0x270` IsWarpingIn (BYTE)
- `+0x271` IsWarpingOut (BYTE)
- **`+0x1DC` allied-shimmer phase base / Mirage disguise frame counter** —
  **VERIFY ALIASING** (Section 9 OQ#5)
- `+0x1EC` shimmer-cycle CDTimer start
- `+0x1F4` shimmer-cycle CDTimer duration
- `+0x1D8` disguise-active flag (Mirage)
- `+0x518` disguised TypeClass pointer (Mirage tree TypeClass)
- `+0x3D2` HasStealthAbility (BYTE)
- `+0x3D5` (unknown, checked alongside HasStealthAbility)
- `+0x41a` likely DisguiseRemoved? (mentioned in Agent D's notes on GetVisualState)

**TechnoTypeClass:**
- `+0xCD0` Cloakable
- `+0xCD2` CloakShroudRadius (?)
- `+0xC93` CloakStop
- `+0xC9A` Invisible
- `+0x310` CloakingSpeed
- `+0x2A2` VeteranAbilities[CLOAK]
- `+0x2B4` EliteAbilities[CLOAK]
- `+0x5F0` SensorsSight
- `+0xD2F` CanDisguise
- `+0xD30` PermaDisguise
- `+0xD31` DisguiseWhenStill

**RulesClass:**
- CloakingStages offset — `+0x628 or +0x1898` (Agent D ambiguous, see #17)
- CloakSound offset — `+0x6A0` (Agent D, verify)
- DefaultMirageDisguises offset — TBD

**Globals:**
- `g_CurrentFrameCounter` at `0x00A8ED84` (Agent D confirmed)
- `g_ABuffer` at `0x0087E8A4` (Agent D confirmed)

### Clamps, rounding, off-by-ones

- **CloakProgress = 0 with CloakState != 0**: the prior doc shows this returns
  visual state 0 (opaque) even mid-state. Confirm this edge case in
  `GetVisualState` and decide whether the shader treats this as
  "not-yet-cloaking" or "fully-opaque-mid-transition".
- **`visual >= 0xFF`** boundary: returns 5 (skip), but `0xFF` is the next-to-
  max byte. Verify whether `visual == 0xFF` returns 4 or 5 — affects the very
  last frame of cloaking animation.
- **`(int)((double)Progress / Stages * 256.0)` cast rounding**: truncate vs
  round-to-nearest. With Stages=9 and Progress=8, `8/9*256 = 227.55` → cast
  to 227 (truncate). Verify.
- **Phase wrap-around**: `(g_CurrentFrameCounter - this+0x1DC + 0x40) & 0xFF`
  — this is a 256-frame cycle. Confirm no longer-period cycle.

### Edge cases to test

- **Cloaked Mirage Tank fires its weapon**: does `DecloakToFire` apply? Does
  the disguise also break? Cross-reference `GetFireError` (#19) with
  [DISGUISE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISGUISE_SYSTEM_GHIDRA_REPORT.md) damage-breaks-disguise rule.
- **Mirage on water cell**: the user's brief says "tree disguise only on
  non-water cells". Verify the cell-type gate inside `TurretAI`.
- **Mirage adjacent to enemy sensor**: cloak state can be 0 (visible) while
  disguise is still active. Verify GetDisplayType returns tree-type even when
  cloak is broken.
- **Spy walking with PermaDisguise**: disguise persists through movement;
  cloak-fade does NOT apply (Spy isn't `Cloakable=yes`). Confirm GetVisualState
  returns 0 for Spy.
- **Veteran/Elite-promoted cloak**: a unit with `VeteranAbilities=CLOAK` only
  gains cloak after promotion. Confirm `HasStealthAbility (+0x3D2)` is the
  runtime gate and that prior reports' init-chain claim is correct.
- **Discovered-by-current-player**: the GetVisualState branch returns 3
  (semi-transparent) for the local player viewing their own cloaked unit even
  when fully cloaked. Confirm this is YR behavior (gamemd allies see allied
  cloaked units; enemies see nothing).
- **Spectator / replay mode**: GetVisualState has dev-mode and game-mode
  short-circuits. Confirm these are not reachable in a normal YR skirmish.
- **CloakingSpeed=0**: would cause divide-by-zero or instant-cloak.
  Investigate the clamp.

### Timing / ordering

- **`advance_tick` insertion point** for the future Rust Cloak component:
  must run AFTER damage/combat (decloak triggers fire there) and BEFORE
  vision/sensors (cloak state affects visibility-to-enemies). Confirm by
  reading the prior reports' "Decloak Triggers" section vs the existing Rust
  tick order in `CLAUDE.md`.
- **Tick order of `CloakingTick` vs `UnitClass::TurretAI`**: relevant to
  #20. If TurretAI runs first and writes +0x1DC, then CloakingTick reads
  it — the Mirage disguise frame counter aliases the cloak phase base.
  If the reverse, they're effectively independent.
- **Allied shimmer phase tick source**: `g_CurrentFrameCounter` is the game
  tick. Confirm the shimmer cycle is per-tick deterministic, not
  wall-clock — affects whether the shader needs a `current_frame` uniform or
  can rely on CPU-pre-computed alpha per frame.

### TS-legacy flags

- **`SpecialFlags` checks anywhere in the cloak path**: scan inside #2
  (GetVisualState), #3 (TechnoClass__Draw), #1 (CloakingTick) for
  `SpecialFlags & 0x????`. None expected (cloak is YR-active) but flag any
  that appear.
- **Building cloak generators**: [BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md)
  marks `CloakGenerator=yes` and `Cloakable=yes` as TS-legacy on stock-YR
  buildings — Agent B confirmed no INI key `CloakGenerator=` appears in
  retail rulesmd.ini (only `GapGenerator=` on GAGAP). **Building cloak FX
  is OUT OF SCOPE for Phase 2.**
- **VXL state-4 brightness variant** (`0x200A` / `0x200C` bits): listed as
  HIGH TS-legacy risk in #18. The live-YR voxel dispatch table per
  [VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md) §2.1 only enumerates 4 reachable
  slots (4/5/6/7 = lit, no/mirror, OBB-half), none of which are described as
  "brightness variants". The 0x200A/0x200C path may be a dormant SHP-only
  flag that VXL inherits but never resolves to a distinct visual.
- **`Invisible=yes` (TechnoTypeClass+0xC9A)**: per Agent B not used on any
  retail YR type. Code is reachable but condition never fires in a stock
  game. Document as conditional/dormant.

### Vtable dispatches to resolve

- **vtable+0x460 `StartCloaking`** — #12
- **vtable+0x45C `StartUncloaking`** — #13
- **vtable+0x43C `ModifyCloakDrawFlags`** — #5 (allied shimmer)
- **vtable+0x2A4 `ShouldUncloak`** — already covered; LIGHT confirm only
- **vtable+0x2A0 `CanAutoCloak`** — already covered; LIGHT confirm only
- **vtable+0x288 `IsCloakable`** (FootClass override) — already covered;
  LIGHT confirm only
- **vtable+0x464 unnamed (Get_Visual_Transparency_Scale?)** — Agent D found
  this is called inside `TechnoClass__Draw` for VXL; verify its purpose
  during #18.

---

## 5. INI Keys in Scope

| Key | Section | Default | Suspected Purpose | Parsed in Rust? |
|-----|---------|---------|-------------------|------------------|
| `CloakingStages` | [General] | 9 | Number of fade steps from opaque to invisible | NO |
| `CloakSound` | [AudioVisual] | NavalUnitEmerge | Sound at cloak/decloak | NO |
| `DefaultMirageDisguises` | [General] | TREE01..TREE04 | Pool of OverlayTypes Mirage Tank picks from | NO |
| `Cloakable` | per-type | no | Unit can cloak | NO |
| `CloakingSpeed` | per-type | TBD | Frames between fade steps | NO |
| `CloakStop` | per-type | TBD | Movement breaks cloak | NO |
| `Invisible` | per-type | no | Type is fully hidden to enemies always | NO |
| `CanDisguise` | per-type | no | Type can hold a disguise | YES (object_type.rs) |
| `PermaDisguise` | per-type | no | Disguise never breaks from damage | NO |
| `DisguiseWhenStill` | per-type | no | Type auto-disguises when stationary | NO |
| `DetectDisguise` | per-type | no | Type reveals nearby disguised units | NO |
| `DetectDisguiseRange` | per-type | TBD | Disguise-detection radius (cells) | NO |
| `DecloakToFire` | per-weapon | yes (?) | Weapon forces decloak before firing | YES (weapon_type.rs) |
| `RadarInvisible` | per-type | no | Hidden from radar/minimap | YES (object_type.rs) |
| `Sensors` | per-type | no | Type acts as a sensor source | NO |
| `SensorsSight` | per-type | 0 | Sensor radius in cells | NO |

**Summary:** Of 16 cloak-FX-adjacent keys, only 4 are parsed in Rust today.
**13 keys × associated TechnoTypeClass / RulesClass field offsets must be
added to the Rust parser as part of the Phase 2 implementation** — but
that's `/write-plan`'s job; this investigation produces the offsets.

---

## 6. Caller & Integration Map

### gamemd.exe callers

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|------------|--------------|----------------------------|
| `TechnoClass::AI` (TBD) | #1 `CloakingTick` | Every game tick for every alive TechnoClass | LIGHT — just confirm reachable from a normal YR unit-update flow |
| `UnitClass::AI` (TBD) | #10 `UnitClass::TurretAI` | Every tick for every UnitClass with a turret | LIGHT — same |
| `TechnoClass__Render` (already documented) | #3 `TechnoClass__Draw` (VXL) and #4 `TechnoClass_DrawSHP` | Every frame for visible units | NO — already covered; cite the prior report |
| `Blitter_selector` `0x00490B90` | #7 / #8 / #9 (blitters) | Inside every draw call that has a non-opaque flag | MEDIUM — for #6, to locate the per-instance LUT base setup |
| `TechnoClass::GetFireError` `0x006FC0B0` | #15 `DoUncloak` (via DecloakToFire path) | Pre-fire check, every weapon trigger | LIGHT (#19) |
| `WarheadTypeClass::Detonate` | `DoUncloak` (damage decloak path; already documented) | Per-hit | NO — already in CLOAKING_INTERACTIONS |
| `ObjectClass::Limbo` / `Unlimbo` | Transport enter/exit cloak handling | Per transport interaction; already covered | NO |

### Callers NOT in scope

- All BuildingClass cloak-generator callers (gap generator code) — TS-legacy
  per [BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md).
- Spy-infiltration callers — covered in [SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md),
  but spy-infiltration is post-spy-infiltrates-building, not cloak FX.
- Chronosphere warp-in/out (`IsWarpingIn` / `IsWarpingOut` at +0x270/+0x271) —
  these compose with cloak draw flags but are the subject of Phase 5 (Warp
  FX) per the parent design doc, not Phase 2.

### Rust integration map

Where the Rust render layer will read cloak state and populate FX uniforms:

- **`src/sim/components.rs`** — new `Cloak` component with fields `state:
  CloakState`, `progress: u8`, `step_timer: i32` (game-tick CDTimer),
  `recloak_delay: i32`, `shimmer_phase_base: u32` (per-instance frame counter
  origin). Hashes into `World::state_hash`.
- **`src/sim/world.rs` `advance_tick`** — new tick stage between
  damage-application and vision-update: `tick_cloak(world)` that advances
  CloakProgress, evaluates triggers, fires CloakSound EVA cue at state
  transitions.
- **`src/rules/object_type.rs`** — add fields for `cloakable: bool`,
  `cloaking_speed: i32`, `cloak_stop: bool`, `invisible: bool`,
  `disguise_when_still: bool`, `perma_disguise: bool`, `detect_disguise: bool`,
  `detect_disguise_range: i32`, `sensors: bool`, `sensors_sight: i32`.
- **`src/rules/ruleset.rs`** — add `cloaking_stages: i32` (default 9),
  `cloak_sound: SoundId`, `default_mirage_disguises: Vec<OverlayTypeId>`.
- **`src/app_instances/units.rs`** — `build_unit_instances` reads
  `entity.cloak.as_ref().map(|c| (c.fx_flags_bit_0(), c.fx_params_0()))`
  and populates `SpriteInstance.fx_flags` and `fx_params[0]`. For Mirage
  with active disguise + non-water cell + state == Cloaked, ALSO override
  `UnitSpriteKey` to render the tree OverlayType instead of the VXL — likely
  via a `display_type_override` similar to the existing harvester
  unload-class mechanism at [units.rs:91-97](src/app_instances/units.rs#L91-L97).
- **Sound integration:** new sound trigger when `CloakState` transitions
  0→1 or 2→3.

---

## 7. TS-Legacy Risk Register

Per CLAUDE.md "Tiberian Sun legacy code in gamemd.exe — READ THIS CAREFULLY",
the executor must verify every code path is live in standard YR before
implementing.

- **`SpecialFlags`-gated cloak paths**: NONE EXPECTED — cloak is YR-active on
  SUB/DLPH/SQD. Scan for `SpecialFlags & 0x????` reads in #1, #2, #3, #5 and
  flag any hits.
- **`Invisible=yes` (TechnoTypeClass+0xC9A) consumer in GetVisualState (#2)**:
  Reachable from any cloaked unit's draw, but Agent B confirmed NO retail YR
  type sets `Invisible=yes` in the INI. **Code is live; data does not exist
  in stock YR.** Document as "conditional — unused in retail data".
- **Building cloak generators (`CloakGenerator=`, `Cloakable=` on
  BuildingTypeClass)**: TS-legacy, not on any retail YR building. Out of
  scope.
- **VXL state-4 brightness variant flags `0x200A`/`0x200C` (#18)**: HIGHEST
  TS-legacy risk in this investigation. The live YR voxel rasterizer table
  per [VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md) §2.1 enumerates only 4
  reachable slots (4/5/6/7), none described as "brightness variants". The
  `0x200A`/`0x200C` path may be dormant. **The investigation must
  determine** whether these bits resolve to a visually distinct downstream
  effect in a normal YR cloaked-VXL draw, or whether they're shipped-but-
  unreachable code. If the latter, the shader can ignore brightness
  variation for VXL state 4 and just use the same alpha as state 3.
- **Allied vs enemy view discrimination in GetVisualState (#2)**: the
  `IsAlliedWithPlayer` / `IsDiscoveredByCurrentPlayer` branches presume
  player-perspective rendering. In replay/spectator mode the dev-mode
  short-circuit fires; verify these aren't reachable in stock YR
  singleplayer/skirmish (per CLAUDE.md, fog of war is off in YR by default —
  visibility computation differs).
- **Tibsun-era field `+0x1DC` aliasing (Section 9 OQ#5)**: if disguise and
  cloak both write `+0x1DC`, this might be a TS-era field whose dual-use
  was an unintended consequence of code merging. Even if it produces a
  player-visible cloak-phase-reset every time Mirage picks a new tree,
  parity may require preserving the alias. The investigation must
  characterize the observable result, then we decide whether to mirror it
  in Rust or implement clean separate fields with the same output.

---

## 8. Current Rust Implementation Surface

### Already present (Phase 1 of voxel-gpu-remap-fx)

- **`src/render/batch.rs`** SpriteInstance fields `fx_flags`, `fx_params: [f32;4]`,
  `ic_tint: [f32;4]`, `house_color_idx: u32` — all wired to GPU vertex
  attributes at locations 7-10.
- **[src/render/sprite_voxel_shader.wgsl:96-109](src/render/sprite_voxel_shader.wgsl#L96-L109)**
  `apply_fx()` with bit-0 stub `if ((flags & 1u) != 0u) { c.a = c.a * params.x; }`.
- **`src/rules/object_type.rs:284-287`**: `radar_invisible` (RadarInvisible)
  parsed. `can_disguise` (CanDisguise) parsed. `gap_generator` (GapGenerator)
  parsed-but-placeholder.
- **`src/rules/weapon_type.rs`**: `decloak_to_fire` (DecloakToFire),
  `disguise_fake_blink_time`, `disguise_fire_only` parsed.
- **`src/rules/warhead_type.rs`**: `makes_disguise` (MakesDisguise) parsed.
- **`src/rules/ruleset.rs`**: `attack_cursor_on_disguise` (AttackCursorOnDisguise)
  parsed.
- **`src/app_instances/units.rs:91-97`**: existing `display_type_override`
  mechanism on GameEntity (used by miner unload-class rendering). The
  Mirage tree-disguise sprite swap can reuse this pattern.
- **`src/app_instances/units.rs:238-249`**: SpriteInstance push site leaves
  `fx_flags`, `fx_params`, `ic_tint` at default-zero via `..Default::default()`.

### Brand new for Phase 2 (out of scope for this investigation, but listed
for context)

- `Cloak` component on `GameEntity` (sim-side state machine)
- `tick_cloak()` system in `World::advance_tick`
- 10+ new INI parse entries in `object_type.rs` (Cloakable, CloakingSpeed,
  CloakStop, Invisible, DisguiseWhenStill, PermaDisguise, DetectDisguise,
  DetectDisguiseRange, Sensors, SensorsSight)
- 3 new INI parse entries in `ruleset.rs` (CloakingStages, CloakSound,
  DefaultMirageDisguises)
- FX-uniform population at the SpriteInstance push sites in `units.rs`
- Mirage tree-disguise sprite-key override via `display_type_override`
- Allied-shimmer phase computation (CPU-side per frame using game-tick
  counter, OR shader-side using a `current_frame` uniform — design decision
  for /brainstorm phase)

### Components / files NOT in scope

- `src/sim/components.rs` is a single-file 705-line module; no
  `src/sim/components/` subdirectory. The Cloak component will be added
  inline OR the file split first — `/brainstorm` will decide.

---

## 9. Deferred Open Questions

Surfaced during scoping; cannot be resolved without `/re-investigate`-grade
work. These are the load-bearing unknowns; the investigation **must close
each of these** or explicitly document them as still-unresolved.

### OQ#1 — Is the `CloakingTick` address `0x006FB740` correct?

**Source of doubt:** Agent D's scoping decompile of `0x006FB740` read fields
`+0x220`, `+0x224`, `+0x228`, `+0x22c`, `+0x238` and dispatched via vtable —
which matches *parts* of cloak state — but did not present as the integrated
per-tick state machine the prior docs describe. **Two possibilities:**
(a) the address is correct and Agent D misread the body in a quick scan, or
(b) the address actually belongs to a related vtable method (e.g.,
`AI_per_tick` or `Update_CloakState`) and the true single-function
"CloakingTick" lives elsewhere. **First-priority task in Phase 1.**

### OQ#2 — Where are `TechnoClass+0x1DC`, `+0x1EC`, `+0x1F4` written?

**Source of doubt:** Agent D's byte-pattern search for writers (`c7 86 ec 01
xx xx`, `89 86 ec 01 xx xx`, etc.) returned no matches. The fields are clearly
read by `ModifyCloakDrawFlags (0x0070ED80)` but the writers were not located
via pattern search. Most likely they're written via different addressing
modes (e.g., `[esi+edx*4+...]` or `[ebx+...]` with ESI/EBX-relative base).
**Probable site:** inside `StartCloaking (0x00703770)` (#12) and possibly
`StartUncloaking (0x007036C0)` (#13). Use Ghidra's references-window /
xref-to-data on the timer fields directly.

### OQ#3 — What is the `intensity_table` for the shimmer blitter?

**Source of doubt:** Per [CLOAKING_VISUAL_PIPELINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CLOAKING_VISUAL_PIPELINE.md) line 755, the shimmer
blitter reads `alpha = intensity_table[intensity * 512 + a_buffer_pixel]`.
The `intensity * 512` stride suggests a 512-entry-per-intensity-level table.
Agent D found that the LUT base is stored at `BlitterInfo+8` (per-instance);
the setup happens inside `Blitter_selector (0x00490B90)`. **Locate the static
table and extract its layout** to determine whether shimmer is a dither
pattern (parity-required) or just a smooth alpha curve (then flat alpha
suffices in our shader).

### OQ#4 — VXL state-4 flags `0x200A` / `0x200C`: distinct visual or dormant?

**Source of doubt:** `TechnoClass__Draw` for VXL units sets these bits for
visual_state 4 (the only place differing from SHP), and `param_11` propagates
them downstream. But the live YR rasterizer dispatch table per
[VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md) doesn't enumerate a "brightness
variant" — only 4 reachable slots (4/5/6/7), all of which are described as
"lit, with/without mirror, OBB-half". **The bits may be dead** (shipping-but-
unreachable) or they may modulate the lit rasterizer's intensity. If dead,
state 4 collapses to the same visual as state 3 for VXL units, and our
shader doesn't need a separate state-4 branch.

### OQ#5 — Is `TechnoClass+0x1DC` shared between cloak phase and Mirage disguise frame?

**Source of doubt:** Agent D found `UnitClass::TurretAI` writes
`param_1[0x77] = g_CurrentFrameCounter`. With `param_1` typed `int *`, that's
byte offset `0x77 × 4 = 0x1DC` — the same offset `ModifyCloakDrawFlags`
reads as the allied-shimmer phase base. **Two possibilities:**
(a) the offset truly is shared, and every Mirage disguise pick resets the
cloak-shimmer phase (player-observable cosmetic detail), or
(b) the offset interpretation differs (e.g., `param_1` is `int` in one of
the functions, not `int *`, making the actual offsets `0x77` vs `0x1DC`
distinct).  **Verify by reading the function prototype on both sides** and
applying the CLAUDE.md decompilation pitfall guidance. If shared, this is a
parity item to preserve.

### OQ#6 — Does the Mirage tree-disguise sprite path participate in cloak FX?

**Source of doubt:** [DISGUISE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISGUISE_SYSTEM_GHIDRA_REPORT.md) documents that
`GetDisplayType` returns the tree's TypeClass pointer for a fully-disguised
Mirage. But does the cloak draw flag path (`TechnoClass__Draw` at #3) still
fire on this tree-sprite output, with the cloak-fade alpha multiply
applied to the tree pixel? OR does `GetDisplayType` short-circuit to render
the tree opaque, ignoring cloak state entirely? **The hand-off mechanism
matters for our Rust integration:** if tree always renders opaque, we just
sprite-swap on full-cloak. If tree gets cloak FX applied, we need to apply
shader FX to whatever sprite our `display_type_override` resolves to.

### OQ#7 — Does the allied shimmer cycle use game-tick or wall-clock?

**Source of doubt:** `ModifyCloakDrawFlags` reads `g_CurrentFrameCounter`
(global at `0x00A8ED84`, Agent D confirmed). If this is the game tick (sim
deterministic), the shader can compute phase from a uniform passed
per-frame. If it's wall-clock (render frame counter), the phase is
non-deterministic and the lockstep contract is unaffected (render-only).
**Verify `g_CurrentFrameCounter` is the game-tick counter, not a render-frame
counter.** Per CLAUDE.md, sim must remain deterministic.

---

## 10. Execution Strategy

**Recommended: Multi-phase single `/re-investigate` session.**

The plan splits into 3 phases with explicit checkpoints. Run as one
`/re-investigate cloak FX system` invocation, pausing for summary between
each phase:

- **Phase 1 (≈3-4h):** Verify the load-bearing addresses + the 3 blitter
  pixel formulas + the visual-state mapping + the allied-shimmer phase
  function. Inventory items #1-9. Checkpoint: address-correctness confirmed
  or corrected before Phase 2 begins.
- **Phase 2 (≈2-3h):** Resolve the 6 open questions in Section 9 except OQ#1
  (resolved in Phase 1). Inventory items #10-17. Checkpoint: Mirage
  tree-disguise hand-off contract written; +0x1DC aliasing resolved; INI
  offsets confirmed with `param_1` typing noted.
- **Phase 3 (≈1-2h):** Edge cases + VXL brightness variant disposition +
  decloak-on-fire + tick-order. Inventory items #18-21.

**Batched-subagent variant (alternative for time-boxed run):**

If `/re-investigate` is going to be split across sessions: dispatch Phase 1
items #1-9 as a single batch (these are tightly coupled around the
state→pixel pipeline); Phase 2 items #10-15 as a second batch (Mirage + state
transitions); Phase 2 items #16-17 + Phase 3 items #18-21 as a third batch
(INI offsets + edges).

**NOT recommended:** Parallelizing all 21 items to subagents. The CloakingTick
address-correctness in #1 gates the interpretation of half the inventory; a
fan-out without that anchor produces a report keyed to a possibly-wrong
address.

---

## 11. Success Criteria

The executed research document must:

- Answer every one of the 6 questions in Section 1 with explicit cite (Ghidra
  address or "verified-from-binary at 0x..."). Items left unresolved must be
  explicitly marked as such — not glossed over.
- Include every function from Section 3 in the inventory with confidence
  level, or explicitly justify omission (e.g., "#21 not needed — TechnoTypeClass
  offsets verified during cross-reference of #2").
- Resolve every Section 9 open question (OQ#1-7), or, for the unresolved ones,
  document the residual unknown precisely enough that a follow-up
  `/plan-investigation` can re-scope.
- State "Active in YR: Yes / No / Conditional" for every finding per CLAUDE.md.
- Cite Ghidra addresses for every HIGH-confidence claim. Inferred / MEDIUM
  claims explicitly marked.
- Produce a **shader-bridge table** in the report's section 2 of the form:

  ```
  | runtime state | viewer relation | fx_flags bit 0 | fx_params[0] | sprite override |
  | CloakState=0 | (any) | 0 | 1.0 | none |
  | CloakState=1, prog 1-2 | enemy | 0 | 1.0 | none ("appear opaque") |
  | CloakState=1, prog 3-6 | enemy | 1 | 0.50 | none |
  | CloakState=1, prog 7   | enemy | 1 | 0.25 | none |
  | CloakState=1, prog 8   | enemy | 1 | 0.10 | none |
  | CloakState=2 | enemy | 1 | 0.0 (or "discard")  | none |
  | CloakState=2 | allied | 1 | [computed from shimmer cycle] | none |
  | CloakState=2 | self, Mirage on non-water | 1 | [shimmer cycle] | tree from DefaultMirageDisguises |
  ... etc
  ```

  This table is what makes the report executable — without it, the report is
  reference material; with it, the report is a Rust implementation recipe.

- Note explicitly which formulas in the prior reports were **re-verified**
  vs which were trusted from the prior doc. Per CLAUDE.md "are you sure?"
  rule, the load-bearing items (visual-state computation, the 3 blitter
  pixel formulas, the allied-shimmer phase function) must each be
  re-derived from the binary on this pass.

- Note any newly-found TS-legacy paths in the cloak rendering pipeline beyond
  those already cataloged in [BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md).

---

## Sources

- **Ghidra addresses pre-sampled during scoping:**
  `0x00703860` (GetVisualState),
  `0x00706640` (TechnoClass__Draw VXL),
  `0x00705E00` (TechnoClass_DrawSHP),
  `0x006FB740` (CloakingTick — address-verification flagged as OQ#1),
  `0x00490B90` (Blitter_selector),
  `0x00494330` (Shimmer 75/25 blitter),
  `0x00497CF0` (50/50 blitter),
  `0x00494080` (25/75 blitter),
  `0x0070ED80` (ModifyCloakDrawFlags / allied shimmer),
  `0x007468C0` (UnitClass::TurretAI / Mirage tree-pick),
  `0x00703770` (StartCloaking),
  `0x007036C0` (StartUncloaking),
  `0x004D3780` (DoCloak),
  `0x006F4EB0` (DoUncloak),
  `0x006FC0B0` (TechnoClass::GetFireError),
  `0x006691E0` (RulesClass::ReadAudioVisual entry),
  `0x0066A6FA` (CloakSound xref),
  `0x0066F146` (CloakingStages xref).
  Global symbols: `0x00A8ED84` (g_CurrentFrameCounter), `0x0087E8A4` (g_ABuffer).

- **Docs searched:**
  - `docs/research/` — 7 cloak-related reports
    surveyed (CLOAKING_VISUAL_PIPELINE.md, CLOAKING_STEALTH_SYSTEM_GHIDRA_REPORT.md,
    CLOAKING_INTERACTIONS_REPORT.md, DISGUISE_SYSTEM_GHIDRA_REPORT.md,
    SENSOR_CLOAK_DETECTION.md, BUILDINGCLASS_CLOAK_SENSOR_GHIDRA_REPORT.md,
    SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md).
  - [VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md) and
    `VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md` for VXL-pipeline cross-reference
    (re: state-4 brightness flag).

- **INI files checked:**
  - `ini/rulesmd.ini` — cloak/disguise/sensor keys; tabulated in Section 5.
  - `ini/rules.ini` — base RA2 fallback; same key list.
  - `ini/artmd.ini` — no cloak-specific keys.

- **Related plans:**
  - `docs/plans/2026-05-10-voxel-gpu-remap-fx-design.md` (parent design,
    Phase 2 = Cloak FX is what this investigation unblocks).
  - `docs/plans/2026-05-10-voxel-gpu-remap-fx-plan.md` (Phase 0+1
    implementation plan, already executed at commits 9930229..6854fc9).
