# Refinery Storage Flow — Investigation Plan

> **For Claude:** Execute via `/re-investigate refinery storage flow` with this plan loaded as context, OR dispatch the function inventory to subagents in 2-3 batches grouped by phase. Phase 1 checkpoint is mandatory before starting Phase 2.

**Topic:** Per-refinery ore storage in gamemd.exe — the full data flow from a harvester depositing a bale into a refinery, through the building's local `StorageClass` accumulator, into the owner's credit pool, *and* the tier-display visual indicator, AI-difficulty credit bonus (`AIVirtualPurifiers`), and Ore Purifier bonus (`PurifierBonus`) that share the same plumbing.

**Scope Size:** Medium — 18 functions, 5 INI keys, 2 RulesClass fields, 5 BuildingClass/HouseClass offsets.

**Est. Effort:** ~4-5 hours of `/re-investigate` work (~15-30 min per FULL function × 5 = 75-150 min; ~5-10 min per MEDIUM × 8 = 40-80 min; ~2-5 min per LIGHT × 5 = 10-25 min, plus synthesis).

**Prior Research:** 21 documents found (see Section 2). Critical gap: `DepositOreFromStorage @ 0x522D50` is **referenced but never decompiled** in any prior report. The whole storage→credits chain is incomplete.

**Expected Output:** research document at
`docs/research/REFINERY_STORAGE_FLOW_GHIDRA_REPORT.md`

**Next Pipeline Step:** `/brainstorm refinery storage model` once this report exists. The brainstorm decides whether to add per-building storage 1:1 with gamemd, or use a cleaner Rust internal design that produces the same observable outputs (credits awarded, tier displayed, purifier bonus applied, AI economy bonus applied).

---

## 1. Goal

After this investigation we must be able to answer, with binary evidence:

1. **Where does a bale go?** Harvester pops a bale during dock-unload — does the bale-value enter (a) the refinery's local `StorageClass` first, then transfer to owner credits via `DepositOreFromStorage`, or (b) go straight to owner credits with the refinery's storage as a parallel cosmetic counter, or (c) split (some to building, some to player)?
2. **When does building storage drain to owner credits?** Every tick? On a timer? On overflow? Per-bale? On-demand? What is `DepositOreFromStorage`'s call cadence?
3. **What's the exact `PurifierBonus` formula and where is it applied?** The gap-scan suggests it's *count-based* (number of purifiers × bonus × ore) not *boolean* — verify against `DepositOreFromStorage`'s decompilation.
4. **How is `AIVirtualPurifiers` applied?** Indexed by what? `HouseClass+0x184` per Agent D, but is that `DifficultyIndex`, `ColorIndex`, `HouseIndex`, or something else? When does the AI bonus add (every bale, or only when AI owns a refinery)?
5. **Storage tier thresholds for the visual:** I already verified `tier = floor(4 × stored / Storage)` in `UpdateAnimation` phase F. Confirm that's the only consumer of building-local storage for the visual, and that the formula is symmetric for `GAREFN`/`NAREFN`/`YAREFN`.
6. **Storage cap behavior:** What happens when `stored == Storage`? Does deposit silently overflow into credits, get clamped, or block the harvester?
7. **Slave Miner deposit path:** The Yuri Slave Manager calls `DepositOreFromStorage` too (`SlaveManagerClass::AI_Update @ 0x6AFBD2`). Does it use the same code path with different parameters, or a sibling function?

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| [ORE_VALUE_CREDIT_DEPOSIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/ORE_VALUE_CREDIT_DEPOSIT_GHIDRA_REPORT.md) | StorageClass struct (4 floats per tib type), AddAmount/GetTotalAmount signatures @ 0x6C9xxx | HIGH | per-bale credit formula not traced; harvester→deposit flow missing |
| `HARVESTER_DOCK_UNLOAD.md` | Dock pad coords, CanDock conditions, dock queue mechanics | MED | `DepositOreFromStorage` not analysed; credit-award path incomplete |
| [HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) | Full unload lifecycle (link→approach→unload loop→undock); MissionRepairAndProduce @ 0x44B780 | HIGH | smoke particle spawning, per-bale visual VFX, AI difficulty multiplier |
| [BUILDING_DOCKING_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_DOCKING_SYSTEM_GHIDRA_REPORT.md) | BuildingTypeClass docking fields (DockingOffset, NumberOfDocks, DockUnload, Refinery flags), CanDock @ 0x457CE0 | HIGH | `Storage=` field NOT mapped; per-building stored ore state |
| [BUILDINGCLASS_UPDATE_ANIMATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_UPDATE_ANIMATION_GHIDRA_REPORT.md) | UpdateAnimation @ 0x4509D0 phase F (Refinery tier-display), slots 3–6 (ActiveAnim..ActiveAnimFour = GAREFNL1..L4) | HIGH | tier formula re-verified in last conversation: `tier = (stored*4)/Storage` |
| `REFINERY_DOCK_ANIM_SLOTS_GHIDRA_REPORT.md` | SetAnimSlotImage on slots 7/10/8 during dock; 4-point smoke burst (vtable+0x468) | HIGH | slot 10 gate condition; first-bale timing — both now fixed in our impl |
| [BUILDING_ANIM_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_ANIM_STATE_MACHINE.md) | 21-slot table structure | HIGH | tier-switching rules covered separately by UpdateAnimation report |
| [AI_DIFFICULTY_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AI_DIFFICULTY_SYSTEM.md) | DifficultyClass struct (9 doubles + 3 bools @ 0x66D270), Firepower/Cost/BuildTime multipliers | HIGH | **`AIVirtualPurifiers` and `PurifierBonus` formula NOT present** in this doc |
| [MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md) | Harvester refinery dock-enter FSM | HIGH | no storage/tier/purifier logic; deferred (prerequisite: this plan) |
| `FACTORY_CREDIT_SYSTEM_GHIDRA_REPORT.md` | HouseClass credits counter increment (unrelated path) | LOW | tangential |
| [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) | RulesClass struct; `AIVirtualPurifiers` at `RulesClass+0x1B7C` (3 ints "4,2,0") | HIGH | **`PurifierBonus` formula NOT decompiled** |
| [MINER_DOCK_GAPS_RESEARCH.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MINER_DOCK_GAPS_RESEARCH.md) | Focused gap audit on miner dock | HIGH | cross-refs storage gaps but defers |
| `docs/plans/2026-05-12-miner-multi-bale-extraction-{design,plan}.md` | Multi-bale harvest + first-bale dock fix (just shipped) | HIGH | follow-ups: tier display, AI bonus, purifier formula — this plan |
| `docs/gap-scans/2026-05-12-gap-scan-miner-deep.md` | Disparity audit (11 findings) | HIGH | findings #7 (tier), #8 (AIVirtualPurifiers), #16 (PurifierBonus) — this plan covers all three |

**Conflicts between reports:**

- `REFINERY_DOCK_ANIM_SLOTS.md` says "ActiveAnim slots 3–6 are always-on loop from moment building placed." [BUILDINGCLASS_UPDATE_ANIMATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_UPDATE_ANIMATION_GHIDRA_REPORT.md) phase F says conditional create/clear based on storage tier. The latter is correct (I re-decompiled phase F last conversation and confirmed the tier-gated `ClearAnimSlot` + `CreateAnimForSlot` pattern). Mark `REFINERY_DOCK_ANIM_SLOTS_GHIDRA_REPORT.md` for a verify pass after this investigation.

**Correction to gap-scan miner-deep (worth recording in the report):**

- Gap-scan finding #5 (per-bale SpecialAnim not played) and #6 (per-bale smoke particles not emitted) are **already implemented** at [src/app_building_anim.rs:341](src/app_building_anim.rs#L341) (`consume_bale_events()`). The function triggers SpecialAnim slot 10 + spawns up to 4 `RefinerySmokeOffsetN` particles per `BaleDepositEvent`. The gap-scan missed this. Findings #5/#6 should be re-marked DONE in any follow-up scan.

## 3. Function Inventory

Group ordering: **Phase 1 must produce a usable skeleton** of the bale→storage→credits chain on its own. Phase 2 fills in formulas and edge cases. Phase 3 confirms integration and TS-legacy status.

| # | Phase | Address | Current Name | Scope Reason | Depth Target | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|--------------|----------------|
| 1 | 1 | `0x00522D50` | `BuildingClass__DepositOreFromStorage` | **The single function this entire investigation orbits.** Drains refinery storage → owner credits; applies PurifierBonus; indexes AIVirtualPurifiers. Never decompiled in any prior report. | FULL — every branch, every field offset, every constant | Low (Refinery= is live in YR) |
| 2 | 1 | `0x006C9690` | `StorageClass__AddAmount` | Per-bale write primitive. Called by Harvest_Ore_Tick + Mission_Deploy_Building. | MEDIUM — args, return, overflow behavior | Low |
| 3 | 1 | `0x006C96B0` | `StorageClass__RemoveAmount` | Drain primitive used by `DepositOreFromStorage`. | MEDIUM — args, return, underflow behavior | Low |
| 4 | 1 | `0x006C9650` | `StorageClass__GetTotalAmount` | Read primitive (used 5× in UpdateAnimation phase F + by Deposit). | MEDIUM — return type, slot summation | Low |
| 5 | 1 | `0x004F9610` | `HouseClass__Add_Tiberium_Credits` | Final credit-pool write. Writes BOTH `HouseClass+0x54E8` and `+0x30C` — verify these are the same field or distinct (cached vs authoritative). | FULL — every branch, both offsets, side effects | Low |
| 6 | 2 | `0x0073D630` | `UnitClass__Mission_Deploy_Building` | State 3 of harvester-deploy contains the per-bale `Add_Tiberium_Credits` call (xrefs at `0x73E4A9, 0x73E4C9`). The per-bale gate `HarvesterDumpRate × 900 ≤ field_0x3E` lives here. **Scope ONLY state 3** — the rest of the FSM is already covered. | FULL on state 3 path | Low |
| 7 | 2 | `0x004F9700` | `HouseClass__Add_Tiberium_To_Storage` | Alternative path that goes building→storage instead of straight to credits. Loops AddAmount until `Storage` cap (per Rules+0x17D0). When is THIS called vs Deposit? | FULL — call sites, loop bounds, overflow | Medium — verify whether ANY YR-normal flow hits this, or only legacy/Tiberium ramp |
| 8 | 2 | `0x004509D0` | `BuildingClass__UpdateAnimation` phase F | Already decompiled last conversation: `tier = (stored * 4) / Storage`, slots 3/4/5/6 = ActiveAnim/Two/Three/Four, ConditionYellow picks damaged variant. Include for completeness. | LIGHT — already verified, just cite | Low |
| 9 | 2 | `0x0050BF60` | `HouseClass__RecalcBonuses` | Recomputes OrePurifier accumulator at `HouseClass+0x5398` (case 3) by multiplying `BuildingTypeClass+0x16D8` across owned buildings. **This is half the PurifierBonus formula.** | FULL — every case, every offset | Low |
| 10 | 2 | `0x0050BEB0` | `HouseClass__GetAccumulatedBonus` | Reads `HouseClass+0x5398` for case 3 (OrePurifier). Used by DepositOreFromStorage. **Other half of PurifierBonus.** | LIGHT — confirm case 3 path + return type | Low |
| 11 | 2 | `0x0066FC6A` | `RulesClass__ReadGeneral` (PurifierBonus site) | Reads `PurifierBonus=` into `RulesClass+0xF3C` (float). Just confirm field type, default, units. | LIGHT — one ReadFloat call | Low |
| 12 | 2 | `0x0067055F` | `RulesClass__ReadGeneral` (AIVirtualPurifiers site) | Reads `AIVirtualPurifiers=4,2,0` into `RulesClass+0x1324` (or `+0x1B7C` per prior report — **conflicting offsets, resolve**). 3-element int array indexed by what? | LIGHT — confirm array layout + index field | Low |
| 13 | 2 | `0x006C9820` | `StorageClass__FindFirstNonEmptySlot` | Iteration helper used by DepositOreFromStorage. Confirms ore vs gem ordering. | LIGHT — return semantics | Low |
| 14 | 3 | `0x0043FB20` | `BuildingClass__Update` | Vtable-dispatched per-frame tick. Confirms UpdateAnimation invocation cadence. | LIGHT — already known, just cite | Low |
| 15 | 3 | (xref-only) | `BuildingClass__Unlimbo` (caller of RecalcBonuses) | RecalcBonuses fires on building placement. Confirm. | LIGHT | Low |
| 16 | 3 | (xref-only) | `BuildingClass__OnSold` (caller of RecalcBonuses) | RecalcBonuses fires on building destruction/sell. Confirm. | LIGHT | Low |
| 17 | 3 | `0x006AFBD2` | `SlaveManagerClass__AI_Update` | The OTHER caller of DepositOreFromStorage. Yuri Slave Miner deposit path. | MEDIUM — confirm whether it uses identical parameters or a Yuri-specific override | Low (Yuri faction is YR-exclusive) but **verify SlaveManager isn't TS leftover bound only to legacy Worker class** — Yuri's slaves are NOT TS workers |
| 18 | 3 | `0x00522E70` | `FUN_00522E70` (likely Slave Harvest_Ore step) | Calls AddAmount + GetTotalAmount; reads `param_1[0x1B0]+0x800` (storage cap on attached refinery). The slave miner's per-bale write site. | MEDIUM — confirm method + caller chain | Same as #17 |

## 4. Detail Checklist

Categories to extract during execution:

**Formulas (high priority):**
- `PurifierBonus` per-bale formula. Conjecture from Agent D: `bonus = storageFacilities × Rules+0xF3C × oreAmount` where `storageFacilities = HouseClass+0x538C + (IsAI ? AIVirtualPurifiers[difficulty] : 0)`. Confirm or correct.
- `AIVirtualPurifiers` index source. Conjecture: `HouseClass+0x184` is the `DifficultyIndex` field. Confirm by reading the field's writer.
- Tier formula `(stored × 4) / Storage` already verified. Document the truncation behavior at exact-25%/50%/75% boundaries (does 25% → tier 0 or tier 1?).
- Storage cap behavior on overflow — does AddAmount clamp, wrap, or silently increment past cap?

**Magic numbers / constants to decode:**
- `BuildingClass+0x538C` — Agent D called this "per-instance Storage / refinery slot bonus" but the name is ambiguous. Is this `storage_facility_count`? `total_storage_value`? Resolve.
- `BuildingClass+0x6F0` — cached tier (last-emitted slot 3/4/5/6 selector). Already verified.
- `BuildingClass+0x184` — Agent D calls this the AIVirtualPurifiers index. Confirm.
- `RulesClass+0xF3C` (PurifierBonus float), `+0x1324` (AIVirtualPurifiers int[3]) — also confirm `+0x1B7C` referenced by prior report; resolve which is right.
- `RulesClass+0x17D0` — global storage cap (Add_Tiberium_To_Storage's loop bound). What INI key sets this?
- `HouseClass+0x54E8` and `+0x30C` — both written by `Add_Tiberium_Credits`. One is probably the live counter, the other a cached/display value. Resolve.

**State machine states:**
- `Mission_Deploy_Building` already known (5 states). Scope only state 3 (the per-bale dump loop) plus the entry condition.

**INI keys to verify:**
- `Storage=` — verify it reads to `BuildingTypeClass+0x800` (Agent D evidence).
- `PurifierBonus=.25` ([General], YR default) — verify `RulesClass+0xF3C` float.
- `AIVirtualPurifiers=4,2,0` ([General]) — verify 3-int array layout + index.
- `Refinery=yes` — verify it sets `BuildingTypeClass+0x16BB`.
- `OrePurifier=yes` — verify it sets `BuildingTypeClass+0x16D8` (the per-building bonus contribution used by RecalcBonuses).

**Struct offsets to extract:**

| Struct | Offset | Inferred field | Verify? |
|--------|--------|---------------|---------|
| `BuildingClass` | `+0x538C` | per-instance storage-facility count or value | YES |
| `BuildingClass` | `+0x184` | HouseClass owner index or DifficultyIndex of owner | YES |
| `BuildingClass` | `+0x6F0` | cached active tier slot (0/3/4/5/6) | already known |
| `BuildingTypeClass` | `+0x800` | `Storage=` capacity | YES |
| `BuildingTypeClass` | `+0x16BB` | `Refinery=yes` flag | YES (confirms phase F gate) |
| `BuildingTypeClass` | `+0x16D8` | `OrePurifier` per-building bonus contribution | YES |
| `HouseClass` | `+0x30C` | credits (live or cached?) | YES |
| `HouseClass` | `+0x54E8` | credits (live or cached?) | YES |
| `HouseClass` | `+0x5398` | accumulated OrePurifier bonus (RecalcBonuses output) | YES |
| `HouseClass` | `+0x184` | possibly DifficultyIndex; possibly something else | **YES — critical** |
| `HouseClass` | `+0x1EC` | `IsHuman` flag (gates AIVirtualPurifiers add) | YES |
| `RulesClass` | `+0xF3C` | PurifierBonus float | YES |
| `RulesClass` | `+0x1324` | AIVirtualPurifiers int[3] | YES (resolve vs `+0x1B7C`) |
| `RulesClass` | `+0x17D0` | global storage cap | YES |

**Edge cases to test:**
- Bale arrives at a full refinery (`stored == Storage`). What happens?
- Refinery destroyed mid-unload — does `DepositOreFromStorage` handle null gracefully?
- AI player with no Ore Purifier — does `AIVirtualPurifiers` still apply, or does it require `storageFacilities > 0`?
- Multiple refineries owned by same player — is `Storage=` per-building or summed?
- `Refinery=no` building with `Storage=N` set — does it still display tier? (Probably not, but confirm phase F gate is `Refinery=`-strict.)
- Yuri's `YAREFN` lacks `Refinery=yes` (per Agent B). What renders the tier on YAREFN, if anything?

**Timing/ordering:**
- Where does `DepositOreFromStorage` fire in the per-tick order vs `UpdateAnimation`? Sim-side, do we need per-bale or per-frame deposit?
- Is `RecalcBonuses` lazy (on building add/remove only) or per-frame? Agent D says 3 callers including Unlimbo + OnSold — confirm no per-tick caller exists.

**TS-legacy flags:** *(see Section 7)*

**Vtable dispatches:**
- `BuildingClass::Update` itself is vtable slot at `0x007E3F18`. No other vtable dispatches expected in scope.

## 5. INI Keys in Scope

| Key | Section | YR Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|------------|-------------------|----------------------------|
| `Storage=` | `[GAREFN]`/`[NAREFN]`/`[YAREFN]` | 200 | Per-refinery storage capacity (max ore the building can hold) | YES — read into `obj.storage` |
| `Refinery=` | per-building | yes (GAREFN/NAREFN), absent (YAREFN) | Flags refinery for tier display + DepositOreFromStorage. **YAREFN lacks this — investigate** | YES — `obj.refinery` |
| `OrePurifier=` | `[GAOREP]`/`[NAOREP]` | yes | Flags building as Ore Purifier — contributes to RecalcBonuses accumulator | YES — `obj.ore_purifier` |
| `PurifierBonus=` | `[General]` | .25 | Per-purifier bonus multiplier (per bale or per total deposit — investigate) | YES — `rules.general.purifier_bonus_pct` (currently treated as bool-pct, may need refactor) |
| `AIVirtualPurifiers=` | `[General]` | 4,2,0 | Hard/Med/Easy AI gets N "virtual" purifiers worth of bonus | **NO — absent in Rust** |
| `RefinerySmokeOffset{One,Two,Three,Four}=` | per-building | varies | Per-bale smoke particle origin | YES — emitted by `consume_bale_events` |
| `RefinerySmokeParticleSystem=` | per-building | SmallGreySSys | Particle type for smoke | YES — used by `consume_bale_events` |
| `SpecialAnim*=` on refineries | per-building | varies | Per-bale flash anim (slot 10) | YES — triggered by `consume_bale_events` |

## 6. Caller & Integration Map

**Callers in gamemd (Phase 3 confirmation targets):**

| Caller | Calls Into | When Invoked | Should Executor Decompile? |
|--------|------------|--------------|----------------------------|
| `UnitClass::Mission_Deploy_Building @ 0x73D630` (state 3) | `Add_Tiberium_Credits @ 0x4F9610` x2 | Per-bale during harvester unload | YES — state 3 only (#6 in inventory) |
| `BuildingClass::Update @ 0x43FB20` | `UpdateAnimation @ 0x4509D0` | Every frame | LIGHT — confirm |
| `SlaveManagerClass::AI_Update @ 0x6AFBD2` | `DepositOreFromStorage @ 0x522D50` | Slave miner deposit (when?) | MEDIUM (#17) |
| `BuildingClass::Unlimbo` | `RecalcBonuses @ 0x50BF60` | Building placement | LIGHT (#15) |
| `BuildingClass::OnSold` | `RecalcBonuses @ 0x50BF60` | Building destruction | LIGHT (#16) |
| `FUN_004FBC58` (unknown) | `RecalcBonuses` | Unknown trigger | LIGHT — confirm context, decompile only if non-trivial |
| `UnitClass::Harvest_Ore_Tick` | `StorageClass::AddAmount` x2 | Per harvest call (already known) | NO — out of scope |

**Where this hooks into Rust today:**

- Sim bale-deposit: [src/sim/miner/miner_dock_sequence.rs:381](src/sim/miner/miner_dock_sequence.rs#L381) — straight to owner credits, no building intermediate.
- Sim PurifierBonus: [src/sim/miner/miner_dock_sequence.rs:386-393](src/sim/miner/miner_dock_sequence.rs#L386-L393) — boolean check via `player_has_purifier()` × pct, NOT count-based as gamemd seems to do.
- Render tier (BROKEN): [src/app_instances/shp.rs:508](src/app_instances/shp.rs#L508) — iterates `art_entry.building_anims`, no tier filter; all slots draw.
- Render per-bale anim/smoke (WORKS): [src/app_building_anim.rs:341](src/app_building_anim.rs#L341) — `consume_bale_events()` already drives SpecialAnim + RefinerySmokeOffsetN.
- Sim AI difficulty: [src/sim/game_options.rs:49-50](src/sim/game_options.rs#L49-L50) — `ai_difficulty: i32` field exists, but **no gameplay code reads it**.

**What other Rust systems will consume the output:**

- Render layer needs to read per-refinery `stored_amount` and `storage_cap` to compute tier on every frame.
- Production/credits system reads owner credits; would change from "bale-direct-to-credits" to "bale-to-building-storage, deposit-tick-to-credits".
- AI economy code (currently nothing) would need to apply the difficulty bonus on each deposit.

**Callers we will NOT investigate (justified):**

- `UnitClass::Harvest_Ore_Tick` AddAmount calls — already verified in the multi-bale work just shipped. The map cell → harvester cargo path is solved.
- `BuildingClass::OnConstructionComplete` (uses GetTotalAmount per Agent D) — likely initialises storage on building placement; out of scope unless it does something exotic.
- `HouseClass::Spend_Money @ 0x4F9790` — credit deduction; out of scope (we don't need to model spending here).
- `BuildingClass::Sell` (also calls Add_Tiberium_Credits) — refund logic; out of scope.
- `FUN_00684C30` (also calls Add_Tiberium_Credits) — unknown context; out of scope unless Phase 1 surfaces a reason.

## 7. TS-Legacy Risk Register

Consolidated list to cross-check during execution. Each item names the flag/path and the verification target:

- **`Type+0x16BB` (`Refinery=`)** — LOW risk. Set on `GAREFN`/`NAREFN` in YR rulesmd. Phase F gate. No defaults-off concern. Confirm anyway that `YAREFN` doesn't have it (Agent B says it doesn't), and document what tier-display behavior YAREFN gets.

- **`HouseClass+0x1EC` (`IsHuman` flag)** — LOW risk. Gates the AIVirtualPurifiers add per Agent D's note "if (!IsHumanPlayer(owner) && g_GameMode != 0)". `g_GameMode != 0` likely means "not in main menu" — confirm this isn't a TS-era campaign gate that defaults differently in YR skirmish.

- **`g_GameMode`** — MEDIUM risk. Used in the AIVirtualPurifiers gate. Could be a TS-era enum where YR skirmish is value N. Find the writer and confirm YR skirmish hits the AI-bonus-active branch.

- **`SlaveManagerClass::AI_Update @ 0x6AFBD2`** — LOW risk. Yuri Slave Miner is YR-exclusive (not TS legacy). But confirm the slave-deposit path isn't gated behind a `Yuri Country=yes` rules flag that defaults off, and that it actually fires during normal skirmish (not just AI players).

- **`FUN_00522E70` (Slave Harvest_Ore step)** — LOW risk. Same as #17.

- **`HouseClass::Add_Tiberium_To_Storage @ 0x4F9700`** — MEDIUM risk. The name says "Tiberium" (TS-era). Could be the TS Worker-class deposit path that's dormant in YR. Trace its callers; if no YR-skirmish caller exists, mark as TS leftover and remove from scope.

- **`RulesClass+0x17D0`** (global storage cap) — LOW risk. Used by Add_Tiberium_To_Storage. If that function is TS-only (above), this offset is also out of scope.

- **`OrePurifier` in TS** — note: in Tiberian Sun, OrePurifier was indeed a building type with the same name. Verify the offset `BuildingTypeClass+0x16D8` reader is gated by an `OrePurifier=yes` flag that's actually live in YR (which it is — `GAOREP`/`NAOREP` use it).

## 8. Current Rust Implementation Surface

**What exists today (mostly correct):**

- [src/sim/miner/](src/sim/miner/) — entire miner FSM (harvest, dock, unload, depart). Just fixed multi-bale + first-bale-timing this session.
- [src/sim/miner/miner_dock_sequence.rs:381](src/sim/miner/miner_dock_sequence.rs#L381) — bale → owner credits. Will need restructuring to bale → building → owner credits.
- [src/sim/miner/miner_dock_sequence.rs:386-393](src/sim/miner/miner_dock_sequence.rs#L386-L393) — boolean PurifierBonus application. Likely needs count-based refactor pending Phase 1 findings on `DepositOreFromStorage`.
- [src/app_building_anim.rs:341](src/app_building_anim.rs#L341) — `consume_bale_events()` per-bale SpecialAnim + smoke. Already complete.

**What is missing (the work this investigation enables):**

- **No per-refinery storage state.** No `BuildingStorage` component on `GameEntity`. Searched: `BuildingStorage`, `building_storage`, `stored_ore`, `stored_credits` → 0 hits.
- **No tier-driven refinery render.** `src/app_instances/shp.rs:508` loops all building_anims; needs a tier filter that reads from per-building storage state.
- **No AIVirtualPurifiers parsing or use.** `ai_difficulty` field exists in `GameOptions` but is dead (0 readers).
- **No PurifierBonus count-based formula.** Current impl assumes boolean has-purifier; gamemd appears count-based.

## 9. Deferred Open Questions

These are explicitly NOT resolved by the scoping pass and must be answered during execution:

1. **Conflicting AIVirtualPurifiers offset:** Agent D says `RulesClass+0x1324`; prior [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) says `+0x1B7C`. Resolve by reading the writer at `RulesClass::ReadGeneral` site `0x67055F`.
2. **YAREFN tier display:** YAREFN lacks `Refinery=yes` per Agent B's INI scan. What renders its storage indicator in gamemd, if anything? (Or do Yuri refineries simply not have one?)
3. **Storage cap clamp vs overflow:** what does `AddAmount` do when `stored + amount > cap`?
4. **`storageFacilities` semantics:** the Agent D snippet says `int storageFacilities = *(owner + 0x538C); if (!IsHuman && g_GameMode != 0) storageFacilities += AIVirtualPurifiers[difficulty];`. So `storageFacilities` is a COUNT, not an amount. What counts as "1 storage facility"? Every owned refinery? Every owned Ore Purifier? Both? Confirm.
5. **`HouseClass+0x54E8` vs `+0x30C` credit fields:** which is the authoritative live counter, which (if either) is a cached display value? Both written by `Add_Tiberium_Credits`.
6. **`Add_Tiberium_To_Storage` aliveness in YR:** trace callers; if no live caller in normal YR skirmish, drop from scope as TS-only.
7. **`FUN_00522E70` real name:** if it's the slave-miner harvest step, name it. If it's something else, document and re-decide scope.
8. **Per-tick vs per-bale deposit drain:** is `DepositOreFromStorage` called once per bale (during state 3 of Mission_Deploy_Building), or on a separate per-tick cadence (e.g., a timer)? Decompiling the function and its caller resolves this.

## 10. Execution Strategy

**Recommend: Batched subagents, three groups.** ~18 functions is too many for a single-session `/re-investigate` to do justice to with FULL depth on the load-bearing ones, but small enough that 3 parallel batches can complete in one sitting.

**Batch A (Phase 1, 5 functions):** Dispatch one agent to FULL-decompile `DepositOreFromStorage` (#1) — that's the spine. Dispatch a second to MEDIUM-decompile StorageClass primitives (#2/#3/#4) and `Add_Tiberium_Credits` (#5). **Checkpoint after Batch A:** synthesise a skeleton of the bale→storage→credits chain. If the skeleton conflicts with Section 1 questions, revise this plan before continuing.

**Batch B (Phase 2, 8 functions):** Dispatch agents in parallel for Mission_Deploy_Building state 3 (#6), Add_Tiberium_To_Storage with TS-aliveness check (#7), the two RulesClass readers (#11/#12), RecalcBonuses + GetAccumulatedBonus (#9/#10), StorageClass slot helper (#13). UpdateAnimation phase F (#8) is LIGHT-cite-only — fold into the synthesis.

**Batch C (Phase 3, 5 functions):** Single agent confirms BuildingClass::Update vtable cadence (#14), Unlimbo + OnSold callers (#15/#16), SlaveManager + slave step (#17/#18). Output is a one-page integration map.

**Synthesis pass:** combine batches into `REFINERY_STORAGE_FLOW_GHIDRA_REPORT.md`. Must explicitly answer every question in Section 1 and Section 9.

## 11. Success Criteria

The executed research document must:

- Answer every numbered question in Section 1
- Include every function from Section 3 (or explicitly justify omission)
- Resolve every deferred question from Section 9 — or re-document them as unresolved with reasoning
- State "Active in YR: Yes/No/Conditional" for every function (default Yes only if traced from a live skirmish caller chain)
- Cite Ghidra addresses for every HIGH-confidence claim
- Include three confidence axes per major finding (content / identity / binding) per [feedback_research_confidence_axes.md]
- Verify every vtable-override claim via live `read_memory` (no Ghidra-label-only trust) per [feedback_vtable_binding_verification.md]
- Note the gap-scan correction: findings #5 and #6 (per-bale SpecialAnim + smoke) are already implemented at [src/app_building_anim.rs:341](src/app_building_anim.rs#L341)

## Sources

- **Ghidra MCP** — gamemd.exe loaded at instance 8089. Functions inventoried: 0x522D50, 0x6C9650, 0x6C9690, 0x6C96B0, 0x6C9820, 0x4F9610, 0x4F9700, 0x4509D0, 0x50BF60, 0x50BEB0, 0x66FC6A, 0x67055F, 0x43FB20, 0x73D630, 0x6AFBD2, 0x522E70. Vtable at `0x007E3F18` (BuildingClass::Update slot).
- **Docs searched (both locations):** see Section 2 table — 21 standalone reports + 5 in-repo gap-scans/plans.
- **INI files checked:** `ini/rulesmd.ini`, `ini/rules.ini`, `ini/artmd.ini`, `ini/art.ini` — see Section 5 table.
- **Related plans:**
  - [docs/plans/2026-05-12-miner-multi-bale-extraction-design.md](docs/plans/2026-05-12-miner-multi-bale-extraction-design.md)
  - [docs/plans/2026-05-12-miner-multi-bale-extraction-plan.md](docs/plans/2026-05-12-miner-multi-bale-extraction-plan.md)
  - [docs/gap-scans/2026-05-12-gap-scan-miner-deep.md](docs/gap-scans/2026-05-12-gap-scan-miner-deep.md) — findings #7, #8, #16
