## Particle System — Gap-Closing Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass that will close every
> remaining gap in [PARTICLESYSTEMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLESYSTEMCLASS_GHIDRA_REPORT.md). The existing report is
> already GREEN-rated and binary-verified through §9 (Verification Pass) and
> §10 (Exhaustive Detail Pass). This plan touches **only the gaps** — do NOT
> re-cover material in §1–§10 of the existing report.
>
> Execute by running `/re-investigate particle-system-gaps` with this plan
> loaded as context, OR dispatch the function inventory (Section 3) to subagents
> in batches of 5–8.

**Topic:** ParticleSystemClass / ParticleClass — close all open gaps in the existing GHIDRA report.
**Scope Size:** Medium — 28 functions, 12 INI keys (mostly verification of consumer-side parsing), 6 vtables to enumerate.
**Est. Effort:** ~5–7 hours of `/re-investigate` work.
**Prior Research:** see Section 2.
**Expected Output:** an addendum / appendix appended to
[docs/research/PARTICLESYSTEMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLESYSTEMCLASS_GHIDRA_REPORT.md)
under a new `## 11. Gap-Closing Pass (2026-05-XX)` section. Do NOT rewrite the
existing 1–10 — append.
**Next Pipeline Step:** `/brainstorm particle-system-rust-architecture` once the
report is 100% complete (no implementation yet — design first).

---

## 1. Goal

When this investigation finishes, the report must answer:

1. What is the exact byte layout of the `DynamicVectorClass<ColorStruct>` block at
   `ParticleTypeClass+0x2B8..+0x2D0`, including which field holds the data
   pointer, count, and capacity? What does the formula
   `ParticleType + 0x2BC + index*3` from §8.3 actually resolve to in memory?
2. How does ParticleSystemClass / ParticleClass serialize to / deserialize from
   the .SAV stream (Load + Save)?
3. What are the exact byte offsets and parser sites in TechnoTypeClass /
   BuildingTypeClass for every consumer-side particle key
   (`DamageParticleSystems`, `DestroyParticleSystems`,
   `RefinerySmokeParticleSystem`, `NaturalParticleSystem`,
   `GapGeneratorParticleSystem`, `BarrelParticle`)?
4. How does `Image=` on a `[Particles]` section turn into a loaded SHP, and
   does ParticleTypeClass differ from AnimTypeClass on this path?
5. What every slot in the ParticleSystemClass vtable (0x7EFB9C) and ParticleClass
   vtable (0x7EF954) actually points to — for both the primary and the three
   secondary (IPersistStream et al.) vtables?
6. What is the active-in-YR status of every `unused` key flagged in the INI scan
   (`DestroyParticleSystems`, `NaturalParticleSystem`, `GapGeneratorParticleSystem`,
   `ChronoSparkle2`) — is the binary parser hot or dead?
7. For the five secondary spawn sites (VoxelAnim, TriggerAction, FUN_00459900,
   FUN_004C2A60, FUN_00684C30), what particle system is spawned, when, and with
   what parameters?
8. Which RulesClass field at `+0x1020` is used by mind-control beam, chrono-warp,
   and electric bolts — what INI key does it map to?

## 2. Prior Research Inventory

| Report | Scope | Confidence | Used For |
|--------|-------|------------|----------|
| [PARTICLESYSTEMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLESYSTEMCLASS_GHIDRA_REPORT.md) | Particle system, full §1–§10 | GREEN (audited 2026-05-04) | Baseline — append to it; do NOT redo |
| `TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md` | TTC ReadINI, base size 0xDF8, ~332 ReadINI calls | HIGH | Skip TTC overall structure — only extract the 6 particle-related read sites |
| [TECHNOTYPECLASS_BASE_ADDENDUM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOTYPECLASS_BASE_ADDENDUM.md) | TTC corrections | MEDIUM | Cross-check before publishing TTC offsets |
| [BUILDINGCLASS_SAVE_LOAD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_SAVE_LOAD_GHIDRA_REPORT.md) | BuildingClass Load/Save (0x453E20 / 0x454190), OLE Structured Storage, IPersistStream contract | HIGH | Use as template for the PSC/ParticleClass Save/Load decompile — same pattern |
| [ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md) | AbstractClass Load (0x410380), Save (0x410320), 4 COM vtables | MEDIUM | Reference for the secondary-vtable layout |
| [OBJECTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_GHIDRA_REPORT.md) | ObjectClass vtable at 0x7EF060 (122 entries) | HIGH | Cross-check — particle vtables override slots from this base |
| [ANIM_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_GHIDRA_REPORT.md) | AnimTypeClass, Image= storage at +0x1F8, ResolveImageForTheater (0x5F9070) | MEDIUM | Confirms the Image=/SHP path is shared with ParticleType |
| [ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md) | LightSourceClass entry at FUN_005FF250, combat light spawner FUN_0048A620 | LOW | Already cross-referenced by §10.3 — no new dive needed |
| [VOXELANIMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXELANIMCLASS_GHIDRA_REPORT.md) | VoxelAnimClass ctor 0x7493B0, AI 0x749F30 | HIGH | Skip the class — only extract the particle-spawn tail call |
| [RULESCLASS_COLORADD_TABLE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_COLORADD_TABLE.md), [HOUSE_CREATION_COLOR_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOUSE_CREATION_COLOR_SYSTEM.md) | Player/scheme color systems | LOW | Not relevant to ColorList runtime layout — skip |

**Conflicts between reports:** none flagged in the scoping scan.

**Ranges already covered (DO NOT redo):**
- ParticleSystemTypeClass / ParticleTypeClass struct layouts (§2.1–§2.2)
- All 5 system-AI functions, all 5 particle-AI functions, all movement functions
- Wind tables, BehavesLike enums, default values
- Spawn-site parameter formats for TechnoClass::AI_Update / ReceiveDamage / Fire_At, BuildingClass::UpdateGapGenerator, CaptureManagerClass::Update, WarpAttachClass::UpdateAttack
- Draw_It pipeline including translucency flags, fast-forward skip, fog check
- Wind direction tables (gas + smoke variants)
- ColorList color-formula at runtime (the 0.05 random jitter, accumulator > 1.0 advance)
- Smoke NextParticle two-child spawn behaviour

## 3. Function Inventory

| #  | Phase | Address      | Current Name                                          | Scope Reason                                                                 | Depth Target | TS-Legacy Risk |
|----|-------|--------------|-------------------------------------------------------|------------------------------------------------------------------------------|--------------|----------------|
| 1  | 1     | `0x00476B20` | `FUN_00476B20`                                         | ColorList parser — strtok loop reading R,G,B triplets into a Color vector. The vector header it returns is what gets copied to `ParticleTypeClass+0x2C8..+0x2D0`. Pin down: (a) what fields live at offsets +0x10/+0x14/+0x18 of the returned vector (data ptr / count / capacity ordering), (b) entry size (3 vs 4 bytes per RGB), (c) how `param_1[0x2BC]` from §8.3 ties in. | FULL         | Low |
| 2  | 1     | `0x00478440` | `FUN_00478440`                                         | DynamicVectorClass<Color>::Clear. Pairs with #1 to confirm vector layout.                                                                                       | MEDIUM       | Low |
| 3  | 1     | `0x004788E0` | `FUN_004788E0`                                         | DynamicVectorClass<Color>::CopyFrom — does the actual byte copy from parser local to the type. Confirms entry stride.                                            | MEDIUM       | Low |
| 4  | 1     | `0x00524EC0` | `FUN_00524EC0`                                         | Identity passthrough used in INI parse macro. Light verify only — confirm it's a no-op.                                                                          | LIGHT        | Low |
| 5  | 1     | `0x007E4E58` | (vtable, 4 dwords)                                    | DynamicVectorClass<ColorStruct> vtable — read all 4 slots and label their bodies (dtor, alloc, resize, clear).                                                  | MEDIUM       | Low |
| 6  | 1     | `0x00477AC0` | `FUN_00477AC0`                                         | Vtable+0x00 — scalar dtor for the Color vector. Confirms the +0x4 buffer / +0xD owned-flag fields.                                                              | LIGHT        | Low |
| 7  | 1     | `0x004784F0` | `FUN_004784F0`                                         | Vtable+0x08 — Resize/SetCapacity. Confirms growth strategy and entry stride.                                                                                    | LIGHT        | Low |
| 8  | 1     | `0x00477900` | `FUN_00477900`                                         | Vtable+0x0C — Reset/Clear. Closes the vector ABI.                                                                                                               | LIGHT        | Low |
| 9  | 1     | `0x00712170` | `TechnoTypeClass::ReadINI`                            | Extract the **6 particle-related read sites** ONLY: `DamageParticleSystems`, `DestroyParticleSystems`, `RefinerySmokeParticleSystem`, `NaturalParticleSystem`, `BarrelParticle` (if present here), `DamageSmokeOffset`/`DestroySmokeOffset`/`RefinerySmokeOffsetOne..Four`. Document every byte offset, default, and vector layout. **Do NOT redo the rest of TTC.**                                       | MEDIUM       | Medium — `NaturalParticleSystem` and `DestroyParticleSystems` have ZERO occurrences in standard YR INI; verify the parser still runs and the consumer code is reachable in YR |
| 10 | 1     | `0x006F32D0` | `BuildingTypeClass::ReadINI`                          | Find `GapGeneratorParticleSystem` reader (BuildingClass::UpdateGapGenerator reads `BuildingType+0x764`, but TTC's slot 0x764 is `NaturalParticleSystem` — likely a building-only override at the same offset). Also extract any other building-only particle key not in TTC. | MEDIUM       | Medium — confirm `GapGeneratorParticleSystem` is not pure TS legacy (gap generators exist in YR, but the smoke transition may not fire) |
| 11 | 1     | `0x005F92D0` | `ObjectTypeClass::ReadINI`                            | Document the `Image=` read site (writes to byte 0x1F8, 25-byte string), the virtual call at vtable+0x2C that selects the loader, and the `Voxel=false` branch that calls #12.                                                                                                  | FULL         | Low |
| 12 | 1     | `0x005F9070` | `FUN_005F9070`                                         | The actual SHP-load helper invoked from #11. Confirms how `Image=` resolves to a SHP file (filename construction, theater handling, MIX lookup).                                                                                                                                | FULL         | Low |
| 13 | 1     | `0x005B40B0` | `LoadFileFromMIX` (suspected)                         | Final MIX-archive lookup called by #12 and AlphaImage path. LIGHT — just confirm it's the MIX reader.                                                                                                                                                                          | LIGHT        | Low |
| 14 | 2     | `0x0062FF20` | `ParticleSystemClass::Load`                           | Already verified to live at primary-vtable +0x14. Decompile to extract: which fields are restored, which are remapped via SwizzleManager (`FUN_006CF240`), what fixups happen for the particle vector at +0xC0.                                                                | FULL         | Low |
| 15 | 2     | (TBD)        | `ParticleSystemClass::Save` (in IPersistStream secondary vtable) | Locate via the secondary vtable at PSC+0x04. Then decompile. Mirror of #14 — what gets serialized and in what order.                                                                                                                                                            | FULL         | Low |
| 16 | 2     | (TBD)        | `ParticleClass::Load`                                | Locate via ParticleClass secondary IPersistStream vtable. Decompile.                                                                                                                                                                                                            | FULL         | Low |
| 17 | 2     | (TBD)        | `ParticleClass::Save`                                | Same as #16 but Save side.                                                                                                                                                                                                                                                      | FULL         | Low |
| 18 | 2     | `0x007EFB9C` | ParticleSystemClass primary vtable (32 dwords already read; need wider read for slots beyond +0x80) | Enumerate **all** slots and label each. Wider read needed (>=128 dwords) to cover Mark for deletion (+0xF8), SetCoords (+0x1B4), GetImageFrame (+0x1D0), GetAnimFrame (+0x1E8). | MEDIUM       | Low |
| 19 | 2     | `0x007EF954` | ParticleClass primary vtable (same — wider read)    | Enumerate all slots. Same wider-read requirement as #18.                                                                                                                                                                                                                        | MEDIUM       | Low |
| 20 | 2     | (TBD)        | PSC + ParticleClass secondary vtables (3 each, lookup via `this+4`/`this+8`/`this+12`) | Three IPersistStream-style vtables per class. Enumerate slots and identify well-known COM methods (QueryInterface / AddRef / Release / Load / Save / GetClassID / GetSizeMax). Each vtable: ~7–8 slots.                                                                          | MEDIUM       | Low |
| 21 | 3     | `0x007493B0` | `VoxelAnimClass::Constructor` (tail only)             | Already-known particle spawn at the end. LIGHT — extract the exact spawn parameters (`VAType+0x2FC` field name, coords source, target/owner args).                                                                                                                              | LIGHT        | Low |
| 22 | 3     | `0x006DD8B0` | `TriggerAction::Execute`                              | Identify which trigger-action subtype index spawns particle systems. The function is a giant switch — find ONLY the case that calls `ParticleSystemClass::Constructor`. Document parameters. | LIGHT        | Medium — TriggerAction enums include TS-era actions; confirm the particle subtype is callable from a YR map |
| 23 | 3     | `0x00459900` | `FUN_00459900` (Refinery dump particle spawner)       | Per Agent D: spawns up to 4 RefinerySmoke systems at offsets 0x7CC..0x7F8. Document trigger condition, spawn parameters, and confirm "fires once per dump cycle". | MEDIUM       | Low (Refineries are core YR) |
| 24 | 3     | `0x004C2A60` | `EBolt::Init` (suspected) / `FUN_004C2A60`           | Electric bolt initialiser — spawns PSC from `RulesClass+0x1020`. Document: what bolt visuals trigger this (Tesla, Prism, IonStorm), and reconfirm `RulesClass+0x1020` (see #28).                                                                                                | MEDIUM       | Medium — IonStorm is YR-active; Tesla/Prism active too. But "BoltExplosion" / similar strings may be TS-era. |
| 25 | 3     | `0x00684C30` | `FUN_00684C30` (Scenario_Start tail)                  | Map-init creates the global `DAT_00A8ED78` PSC at fixed coord (0xA80,0xA80) using `g_RulesClass + arr[idx]*4`. Document which array, which index, and what type the global PSC ends up being. Confirm whether this is reachable in standard YR skirmish (vs only campaign).      | MEDIUM       | HIGH — this is exactly the kind of TS-era weather/storm system that may be dormant in YR. Verify it actually fires in skirmish, not just in campaign. |
| 26 | 3     | `0x00454DB0` | `BuildingClass::UpdateGapGenerator_Tick`              | Already documented in §8.6.4 of the report — but ONLY light-confirm that the TTC offset 0x764 alias for `GapGeneratorParticleSystem` resolves correctly given #10.                                                                                                              | LIGHT        | Low (covered) |
| 27 | 3     | `g_RulesClass + 0x1020` | RulesClass field — INI key identification     | Find the ReadString call in `RulesClass::ReadINI` that writes to byte 0x1020. Identify the INI key name. Candidates: `DefaultSparkSystem`, `BoltSystem`, `IonStormSystem`, or one of the `[General]` keys already enumerated. | LIGHT        | Low |
| 28 | 3     | (TBD)        | `RulesClass::ReadINI` BarrelParticle / GapGenerator key extraction | If #9 doesn't surface `BarrelParticle`, try `RulesClass::ReadINI` (string search for the key). The INI scan shows it lives in `[General]`, so its parser likely lives there.                                                                                                     | LIGHT        | Low |

**Phase boundaries:**
- **Phase 1 checkpoint** (after #1–#13): If ColorList layout doesn't match §8.3's formula or the Image= path differs from AnimTypeClass, STOP and re-scope. These are the entries Rust struct definitions will be built on.
- **Phase 2 checkpoint** (after #14–#20): Save/Load complete + vtables fully labeled. Required for snapshot work to ever touch particles. If unable to locate Save mirrors, document and defer rather than guess.
- **Phase 3 checkpoint** (after #21–#28): Full coverage. Anything still flagged as UNVERIFIABLE goes into a final "Known Limits" subsection — not pretending we resolved it.

## 4. Detail Checklist

The executor must extract and document each item below. Items with known leads from the scoping scan are pre-populated.

**Magic numbers / constants to decode:**
- ColorList entry stride (3 bytes vs 4 bytes — doc currently has both implications)
- Default growth rate of the Color vector
- The "0.05" jitter in spark/railgun color animation — already documented; cross-check with #1
- `RulesClass+0x1020` value default
- The fixed coord (0xA80, 0xA80) in #25 — what does that resolve to in cell space?

**Bit flags / masks:**
- `OneFrameLight` interaction with the LightSize > 0 guard (already in §10.3)
- The vtable+0x2C "image-loader kind code" returned by ObjectTypeClass — document which kinds exist (5 = Voxel, others = ?)

**Struct offsets to extract:**
- TTC byte offsets: 0x764, 0x768, 0x76C, 0x770, 0x774, 0x778, 0x77C, 0x788, 0x78C, 0x790, 0x7A4, 0x7A8, 0x7AC, 0x7B0, 0x7B4, 0x7B8, 0x7BC, 0x7C0, 0x7C4, 0x7C8, 0x7CC..0x7F8
- BuildingTypeClass byte offsets used for gap-generator particle (likely 0x764-aliased + offsets 0x768..0x770)
- ParticleTypeClass byte offsets +0x2B8..+0x2D0 (the contested ColorList sub-layout)
- ObjectTypeClass +0x1F8 (Image= filename, 25-byte char buffer)
- ObjectTypeClass +0x213 (AlphaImage)
- ObjectTypeClass +0xAC (resolved AlphaImage SHP pointer)

**Clamps / rounding / off-by-ones:**
- Color vector capacity-grow heuristic in #7
- Save/Load swizzle pointer remapping ordering in #14

**Edge cases to test:**
- Empty ColorList= (zero entries) — what does the parser do?
- Saving a PSC mid-AI-tick (is the spawn timer field swizzled correctly?)
- A TTC with zero `DamageParticleSystems` and zero `DestroyParticleSystems` — confirm parser doesn't crash
- A `[BuildingType]` with `GapGeneratorParticleSystem=NONE` or omitted — does the binary fall back, error, or ignore?

**Timing / ordering:**
- Where Save/Load fits in the .SAV pipeline (referenced from BUILDINGCLASS_SAVE_LOAD)
- When `FUN_00684C30` runs relative to scenario init (before/after rules parse, before/after first AI tick)
- When `FUN_00459900` fires relative to refinery dock animation phases

**TS-legacy flags:** see Section 7 — consolidated.

**Vtable dispatches:**
- vtable+0x2C in ObjectTypeClass (image-loader kind)
- All un-named slots in PSC and ParticleClass vtables (Sections 18–20)
- Whether ParticleClass::Move_Dispatch (0x62D5E0) is invoked through a vtable slot or only directly

## 5. INI Keys in Scope

| Key | Section | Default / sample | Suspected Purpose | Currently parsed in Rust? |
|-----|---------|------------------|-------------------|----------------------------|
| `DamageParticleSystems` | `[BuildingType]` / `[UnitType]` | `SparkSys,SmallGreySSys` | List of PSCs spawned when below ConditionYellow. Per §8.6.1: AI_Update filters Spark, ReceiveDamage filters Smoke. | NO |
| `DestroyParticleSystems` | (TTC parser exists; **0 occurrences in YR INI**) | — | List spawned on death. May be active in YR for non-rules-defined cases. Verify YR-active status during research. | NO |
| `RefinerySmokeParticleSystem` | `[BuildingType]` | `SmallGreySSys` | Smoke during ore dump (used by FUN_00459900). | NO |
| `NaturalParticleSystem` | (TTC parser; **0 occurrences in YR INI**) | — | Listed in §7.5 as conditional. Confirm whether the parser-allocated slot is read by any active code. | NO |
| `GapGeneratorParticleSystem` | `[BuildingType]` (TTC alias at 0x764, **0 INI occurrences**) | — | Smoke on gap-generator state-3→state-0 transition (see §8.6.4). Verify it's actually active in YR despite no INI overrides. | NO |
| `BarrelParticle` | `[General]` | `SmallGreySSys` | Barrel-fire smoke. | NO |
| `DefaultTestParticleSystem` | `[General]` | `TestSmokeSys` | Generic test/debug system. | NO |
| `DefaultRepairParticleSystem` | `[General]` | `WeldingSys` | Engineer repair sparks. | NO |
| `ChronoSparkle1` | `[General]` (also `[AudioVisual]`) | `CHRONOSK` (animation, not a PSC) | Chrono effect — note this is an Anim, not a PSC, despite report mentions. | NO |
| `ChronoSparkle2` | (parser may exist; **0 INI occurrences**) | — | Verify whether the binary parses it; if it only exists for trigger-action use, document. | NO |
| `WindDirection` | `[General]` | `1` | Already documented; cross-check parser site for completeness. | NO |
| `Image=` | `[Particles]` (per particle type) | e.g. `WCCLOUD1` | Resolved to SHP via ObjectTypeClass::ReadINI path. Confirm no override. | Partial (Anim system has the resolver; particles don't reuse it yet) |
| `Report=` | `[Particles]` | (sample TBD from INI scan #9) | Sound on particle event. Out-of-scope **for now** — flagged but not researched in this pass. | NO |
| `SpawnDelay`, `RandomRate` | `[Particles]` | — | Surfaced by INI scan; not in current report. Flag for §11 mention if active. | NO |

## 6. Caller & Integration Map

| Caller | Calls Into | When Invoked | Decompile? |
|--------|------------|--------------|------------|
| `VoxelAnimClass::Constructor` (0x7493B0) | PSC ctor (0x62DC50) | Once per voxel-anim spawn, if `VAType+0x2FC` set | LIGHT (#21) |
| `TriggerAction::Execute` (0x6DD8B0) | PSC ctor | Map-trigger fires the particle subtype | LIGHT (#22) |
| `FUN_00459900` (refinery dump) | PSC ctor (up to 4 systems) | Each refinery dump cycle | MEDIUM (#23) |
| `FUN_004C2A60` / `EBolt::Init` | PSC ctor | Electric bolt visual init (Tesla / Prism / IonStorm) | MEDIUM (#24) |
| `FUN_00684C30` (scenario init) | PSC ctor | Once at scenario start (creates global PSC) | MEDIUM (#25) |
| `BuildingClass::UpdateGapGenerator_Tick` (0x454DB0) | PSC ctor | Gap-generator state 3→0 | LIGHT (#26 — already covered) |

**Rust integration anchors (informational):**
- `src/rules/weapon_type.rs:211–236` — already parses `AttachedParticleSystem`, `UseFireParticles`, `UseSparkParticles` (refs only, no resolution)
- `src/sim/animation.rs` + `src/rules/art_data.rs` — Image=/SHP binding pattern that ParticleType will share
- `src/sim/components.rs` `DamageFireOverlay` — placeholder building-damage smoke; will be replaced by real particle systems
- No `particle_type.rs`, `particle_system.rs`, or ColorList parser exists yet (confirmed)

**Callers explicitly NOT in scope (already covered by §1–§10):**
- `TechnoClass::AI_Update` (0x6F9E50)
- `TechnoClass::ReceiveDamage` (0x701900)
- `TechnoClass::Fire_At` (0x6FDD50)
- `UnitClass::AI` (0x7360C0) — refinery smoke caller; defers to FUN_00459900 which IS in scope (#23)
- `CaptureManagerClass::Update` (0x471A50)
- `WarpAttachClass::UpdateAttack` (0x629FD0)
- `Apply_area_damage` (0x489280)

## 7. TS-Legacy Risk Register

Every "active in YR" claim the executor produces must check this list:

| Item | Risk | Action |
|------|------|--------|
| `DestroyParticleSystems` | 0 INI occurrences in current YR. Parser still in TTC. | Trace callers in TTC-consumer code; confirm if ANY YR-shipped code actually triggers spawn. If not, mark `Active in YR: No (parser exists, no live caller)`. |
| `NaturalParticleSystem` | 0 INI occurrences. Already flagged in §7.5 of report as "conditional". | Same as above. Trace callers of TTC+0x764 with non-building reads. |
| `GapGeneratorParticleSystem` | 0 INI occurrences. But gap generators DO exist in YR. | Confirm `BuildingClass::UpdateGapGenerator_Tick` actually runs in YR (not just defined). The smoke spawn requires non-default smoke offset (see §8.6.4) — verify a stock YR `[GAGAP]` building meets that condition. |
| `ChronoSparkle2` | 0 INI occurrences. | Verify whether `RulesClass::ReadINI` even tries to read it. If yes, where does it write? If no, drop from doc. |
| Scenario-start global PSC (#25) | High risk — TS-era weather/storm patterns often live in scenario init. | Confirm: is `DAT_00A8ED78` actually consumed by anything during a normal YR skirmish? Or is it only campaign-script-driven? Trace its xrefs. |
| `TriggerAction` particle subtype (#22) | Triggers are mission-driven; many TS-era subtypes exist that no YR map uses. | Search YR mission `.map`/`.mmx` files OR conclude "exists but unused in standard skirmish — campaign-only". |
| `BarrelParticle` | Low risk — `[General]` key exists, refinery barrels are a thing. | Just verify the parser writes it and the consumer (likely VoxelAnim debris) reads it. |
| Light source persistence in spark systems (FUN_0062E280 vs AI_Spark) | Already cross-checked in scoping; no risk. | Re-confirm the OneFrameLight gate is the only difference. |

## 8. Current Rust Implementation Surface

(From Agent C scan — informational only.)

- **`src/rules/weapon_type.rs`** (409 lines) — parses `AttachedParticleSystem=`, `UseFireParticles=`, `UseSparkParticles=` as references only (no resolution).
- **`src/rules/art_data.rs`** (803 lines) — central Image= resolver (used by units/buildings/anims).
- **`src/sim/animation.rs`** — Anim type parser; demonstrates the SHP-binding pattern ParticleType will reuse.
- **`src/assets/shp_file.rs`** (338 lines) — SHP decoder.
- **`src/sim/components.rs`** — `DamageFireOverlay`, `DamagingFire` are hardcoded building-damage smoke placeholders, NOT a particle system. They'll be retired when real PSC support lands.
- **No `particle_type.rs`, no `particle_system.rs`, no ColorList parser, no TechnoType particle keys parsed** — confirmed absent.

## 9. Deferred Open Questions

These are explicitly **deferred** and should be raised in the executed report's
"Known Limits" subsection rather than guessed at:

1. **`Report=` key in `[Particles]`** — sound binding for particle events. Visible in the INI but the audio integration path is out of scope here. Defer to a follow-up `/plan-investigation particle-audio-integration` if needed.
2. **`SpawnDelay` / `RandomRate` in `[Particles]`** — surfaced in INI but not in the existing report. Worth noting whether they're parsed or dead in this gap pass; deeper integration is a follow-up.
3. **LightSourceClass internals** (FUN_005FF250 / 5FF2D0 / 5FF850) — used by spark-with-light systems. The existing report identifies the entry points but not the runtime layout. **Out of scope** for this plan; flag as a sibling investigation.
4. **`.SAV` format outer envelope** — OLE Structured Storage details. The BUILDINGCLASS_SAVE_LOAD report covers the contract; we only need to confirm PSC/ParticleClass conform, not redo the format.
5. **Voxel particle types** (BehavesLike==Voxel? if such exists) — current INI shows none. Confirm via the BehavesLike enum string-table reads (already done in §9.1/§9.2).
6. **Whether `RulesClass+0x1020` is the same field used by mind control AND chrono warp AND electric bolts** — current report claims it is. This plan's #27 confirms it.

## 10. Execution Strategy

**Recommendation: Batched subagents (Phase 1 first, then 2, then 3).**

Reason: 28 functions is too much for one focused session, but the phase boundaries map cleanly to subagent batches.

- **Phase 1 batch A** (ColorList): #1–#8 → one subagent. Returns the vector ABI + struct sub-layout.
- **Phase 1 batch B** (TechnoType + Building parsers + Image= path): #9, #10, #11, #12, #13 → one subagent. Returns 6 byte-offset rows + the SHP-binding flow.
- **Phase 1 checkpoint** — pause, write skeleton findings, decide whether ColorList/Image= claims need re-scope.
- **Phase 2 batch C** (Save/Load): #14–#17 → one subagent. Returns 4 decompiled functions (or "Save not findable" with evidence).
- **Phase 2 batch D** (Vtables): #18, #19, #20 → one subagent. Wider memory reads + slot labels.
- **Phase 3 batch E** (Spawn callers): #21–#28 → one subagent (LIGHT depth, can fit many functions).

Synthesize all 5 batches into a single `## 11. Gap-Closing Pass` section appended to the existing GHIDRA report. Do NOT rewrite §1–§10.

**Alternative**: single-session `/re-investigate` with the same phasing if subagent dispatch isn't desired. ETA ~5–7 hours.

## 11. Success Criteria

The executed research must:

1. Resolve every Section 1 question with HIGH confidence (binary citation per claim).
2. Decompile every function in Section 3 OR explicitly justify omission (e.g., "ParticleClass::Save not located in any vtable; documented as STILL-OPEN").
3. State "Active in YR: Yes / No / Conditional" for every key in the TS-Legacy Risk Register (Section 7).
4. Add a `## 11. Gap-Closing Pass` section to [PARTICLESYSTEMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLESYSTEMCLASS_GHIDRA_REPORT.md) — appended, not replacing §1–§10.
5. Update the existing Section 7 Open Questions with explicit `RESOLVED in §11.X` annotations or `STILL OPEN — see §11 Known Limits`.
6. Bump the report's overall confidence header to reflect the additional verification.
7. Make NO Rust changes. The output is a doc patch only.

## Sources

- **Ghidra addresses sampled (Agent D):** 0x00524EC0, 0x00476B20, 0x004788E0, 0x00478440, 0x007E4E58 (vtable), 0x00477AC0, 0x004784F0, 0x00477900, 0x0062FF20, 0x007EFB9C (vtable, partial), 0x007EF954 (vtable, partial), 0x00712170, 0x006F32D0, 0x005F92D0, 0x005F9070, 0x005B40B0, 0x007493B0, 0x006DD8B0, 0x00459900, 0x004C2A60, 0x00684C30, 0x00454DB0, 0x0062E280, 0x005F7090.
- **Docs surveyed:** PARTICLESYSTEMCLASS, TECHNOTYPECLASS_BASE (+ ADDENDUM), BUILDINGCLASS_SAVE_LOAD, ABSTRACTCLASS, OBJECTCLASS, ANIM_CLASS, ANIMCLASS_SPAWN_PATHS, VOXELANIMCLASS, ALPHA_SHAPE_CLASS_LIFECYCLE, RULESCLASS_COLORADD_TABLE, HOUSE_CREATION_COLOR_SYSTEM, COUNTRY_SIDE_TYPE_CLASSES, READINI_FIELD_MAPS, TECHNOCLASS_VTABLE_COMPLETE, BUILDINGCLASS_VTABLE_COMPLETE, FOOTCLASS_VTABLE_COMPLETE, ANIM_CLASS_DEEP_DIVE, PRISM_CASCADE_TRIGGER.
- **INI files checked:** ini/rulesmd.ini, ini/rules.ini, ini/artmd.ini, ini/art.ini.
- **Rust files surveyed:** src/rules/weapon_type.rs, src/rules/art_data.rs, src/sim/animation.rs, src/sim/components.rs, src/assets/shp_file.rs.
- **Related plans:** none on the same topic (verified).
