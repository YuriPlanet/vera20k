# CellClass CanGrowTiberium / CanSpreadTiberium Predicates - Ghidra Research Report

**Address(es):** `0x00483620` (`CellClass::CanGrowTiberium`), `0x00483690` (`CellClass::CanSpreadTiberium`)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** exact predicate semantics for growth/spread queue seeding and spread-source eligibility at these two helpers, including scenario flags, overlay-to-tiberium mapping, density thresholds, slope and object-list gates, percentage negative/zero behavior, SpecialFlags bit `0x80`, stock YR defaults, current Rust deltas, and do-not-do handoff.
**Non-Scope:** target-cell germination validation in `CellClass::CanPlaceTiberium` except contrast, growth/spread processor batch behavior, timer reload math, save/load reconstruction, and `CellClass::PlaceTiberium` mutation semantics.
**Confidence:** High
**Active in YR:** Yes / Conditional. Both helpers are active through standard YR queue rebuilds. `CanGrowTiberium` is conditional on `[Basic] TiberiumGrowthEnabled` at `ScenarioClass+0x34A6`; `CanSpreadTiberium` is conditional on `SpecialFlags.TiberiumSpreads` bit `0x80` and the all-type spread driver is additionally gated by `ScenarioClass+0x34A6`.

## 0. Working Notes

**Target question:** What exact cells are admitted by `CellClass::CanGrowTiberium @ 0x00483620` and `CellClass::CanSpreadTiberium @ 0x00483690` for queue seeding/source eligibility, and which current Rust predicates must change?

**Non-goals:** Do not re-investigate `CanPlaceTiberium`, heap processor RNG, save/load, terrain object lifecycle, or TIBTRE forced spread except where needed to contrast source-vs-target gates.

**Evidence needed to mark COMPLETE:** decompile and assembly for both primary functions; caller proof; `OverlayToTiberiumIndex` proof; flag parse/default evidence; stock `rulesmd.ini` tiberium percentages; Rust touchpoint scan; final open-question log with no open entries.

**Stop conditions:** Complete after a zero-add pass over the two functions and caller/callee lists; stop if the slice discovers a needed target-validation branch, because target validation belongs to `CanPlaceTiberium` and is explicitly out of scope.

## 1. Overview

`CanGrowTiberium` and `CanSpreadTiberium` are queue membership/source predicates, not full placement validators. They decide whether an existing tiberium overlay cell can be put into the per-type growth or spread queues, and `CanSpreadTiberium` is also used by `AddToSpreadQueue` for source reseeding.

The helpers differ in important boundary cases. Growth admits flat matching tiberium cells with `OverlayData < MaxDensity - 1` and non-negative `GrowthPercentage`; spread admits flat matching source cells with `OverlayData > (tiberium_index / 2)`, non-negative `SpreadPercentage`, no source object-list head at `Cell+0xE4`, and `SpecialFlags.TiberiumSpreads` bit `0x80` set.

## 2. Key Offsets / Fields

| Owner | Offset / global | Meaning in this slice | Evidence | Active in YR |
|---|---:|---|---|---|
| `ScenarioClass` | global `0x00A8B230` | scenario instance pointer | decompile `0x00483620`, `0x00483690` | Yes |
| `ScenarioClass` | `+0x34A6` byte | `[Basic] TiberiumGrowthEnabled`; direct gate for growth helper and both all-type drivers | `0x00483628`, `0x007221B8`, `0x00722C48`, reader `0x00689E90` | Conditional, stock enabled |
| `ScenarioClass/SpecialFlags` | bit `0x80` in first dword | `[SpecialFlags] TiberiumSpreads`; direct gate for `CanSpreadTiberium` | `0x00483699`, reader/writer `0x006B8CA0`, `0x006B8B30` | Conditional, stock enabled |
| `CellClass` | `+0x44` int | overlay type index passed to `OverlayToTiberiumIndex` | `0x00483636`, `0x004836A3` | Yes |
| `CellClass` | `+0x11C` byte | slope index; must be `0` for both helpers | `0x00483650`, `0x004836CF` | Yes |
| `CellClass` | `+0x11E` byte | overlay data / density byte | `0x00483666`, `0x004836BB` | Yes |
| `CellClass` | `+0xE4` pointer | source object-list head; must be null for spread helper only | `0x004836FF..0x00483708` | Yes |
| `TiberiumClass` | `+0x98` int | tiberium type index used by rebuild filtering | rebuild callers `0x007233A0`, `0x007228B0` | Yes |
| `TiberiumClass` | `+0xA0` double | `SpreadPercentage`; helper rejects only negative/unordered | `0x004836E7..0x004836F8` | Yes |
| `TiberiumClass` | `+0xB0` double | `GrowthPercentage`; helper rejects only negative/unordered | `0x00483675..0x00483686` | Yes |
| `TiberiumClass` | `+0xE4` int | MaxDensity; growth requires `OverlayData < MaxDensity - 1` | `0x0048365E..0x0048366F` | Yes |

## 3. Core Logic

### 3.1 `CellClass::CanGrowTiberium @ 0x00483620`

Verified predicate, in exact branch order:

1. Load scenario pointer from `0x00A8B230`; read byte `ScenarioClass+0x34A6`.
2. If `+0x34A6 == 0`, return false.
3. Load `Cell+0x44` and call `CellClass::OverlayToTiberiumIndex @ 0x005FDD20`.
4. If overlay maps to `-1`, return false.
5. Load `g_TiberiumClass_Array[tib_index]`.
6. If `Cell+0x11C != 0`, return false. This is a flat-source gate.
7. Load `TiberiumClass+0xE4`, decrement it, and compare against zero-extended `Cell+0x11E`.
8. If `OverlayData >= MaxDensity - 1`, return false. Stock MaxDensity is `12`, so growth membership admits density/data `0..10` and rejects `11`.
9. Load `TiberiumClass+0xB0 GrowthPercentage` as a double and compare to double constant at `0x007E3810` (`0.0`).
10. If the value is negative, or unordered under the x87 status check, return false; otherwise return true. Normal stock zero passes this helper.

Assembly confirmation:

- `0x00483628`: `MOV CL, byte ptr [EAX + 0x34a6]`; `0x0048362E`: `TEST CL,CL`; `0x00483630`: `JNZ`.
- `0x00483639`: call `0x005FDD20`; `0x0048363E`: compare result to `-1`.
- `0x00483650`: read `Cell+0x11C`; `0x00483656`: `TEST`; nonzero returns false.
- `0x0048365E`: read `TiberiumClass+0xE4`; `0x0048366C`: `DEC ECX`; `0x0048366D`: `CMP EDX,ECX`; `0x0048366F`: `JL` to continue.
- `0x00483675`: `FLD double ptr [EAX + 0xb0]`; `0x0048367B`: `FCOMP double ptr [0x007e3810]`; `0x00483683`: `TEST AH,0x1`; nonzero returns false.

Active in YR: Yes. Direct caller list contains `TiberiumClass::RebuildGrowthQueue @ 0x007233A0`; growth driver also calls `CellClass::GrowTiberium @ 0x00483710`, which duplicates this predicate before actual growth.

### 3.2 `CellClass::CanSpreadTiberium @ 0x00483690`

Verified predicate, in exact branch order:

1. Load scenario pointer from `0x00A8B230`; test bit `0x80` in the first scenario/special-flags dword.
2. If bit `0x80` is clear, return false.
3. Load `Cell+0x44` and call `CellClass::OverlayToTiberiumIndex @ 0x005FDD20`.
4. If overlay maps to `-1`, return false.
5. Compute signed `tib_index / 2`; because `-1` already returned, this is ordinary integer division by two for active non-negative indices.
6. Zero-extend `Cell+0x11E`; if `OverlayData <= tib_index / 2`, return false.
7. If `Cell+0x11C != 0`, return false. This is a flat-source gate.
8. Load `g_TiberiumClass_Array[tib_index]`.
9. Load `TiberiumClass+0xA0 SpreadPercentage` as a double and compare to `0.0`.
10. If the value is negative, or unordered under the x87 status check, return false; normal stock zero passes this helper.
11. Return whether `Cell+0xE4 == 0`. Any source object-list head blocks spread queue/source eligibility.

Assembly confirmation:

- `0x00483699`: `TEST byte ptr [EAX],0x80`; `0x0048369C`: `JNZ`.
- `0x004836A6`: call `0x005FDD20`; `0x004836AD`: compare result to `-1`.
- `0x004836B7..0x004836C4`: `CDQ; SUB EAX,EDX; SAR EAX,0x1` signed divide-by-two shape.
- `0x004836C6`: compare zero-extended `Cell+0x11E` with half-index; `0x004836C8`: `JG` required to continue.
- `0x004836CF`: read `Cell+0x11C`; `0x004836D5`: `TEST`; nonzero returns false.
- `0x004836E7`: `FLD double ptr [EAX + 0xa0]`; `0x004836ED`: `FCOMP double ptr [0x007e3810]`; `0x004836F5`: `TEST AH,0x1`; nonzero returns false.
- `0x004836FF`: read `Cell+0xE4`; `0x00483706`: `TEST EAX,EAX`; `0x00483708`: `SETZ AL`.

Active in YR: Yes/Conditional. Direct callers are `TiberiumClass::RebuildSpreadQueue @ 0x007228B0` and `TiberiumClass::AddToSpreadQueue @ 0x00722AF0`. The all-type spread driver is also gated by `ScenarioClass+0x34A6` before processors run.

### 3.3 Overlay-to-tiberium mapping

`CellClass::OverlayToTiberiumIndex @ 0x005FDD20` first rejects invalid overlay index `-1` or overlay types whose byte `OverlayTypeClass+0x2A9` is false. It then iterates `g_TiberiumClass_Array` and returns `TiberiumClass+0x98` when the overlay id falls into either the flat image range or the sloped image range derived from `TiberiumClass+0xE0` image pointer, image array index `+0x294`, `TiberiumClass+0xE8`, and `TiberiumClass+0xEC`.

If an overlay advertises as tiberium but falls in no tiberium class range, the function logs/registers `"Overlay %s not really tiberium"` and returns `0`. That fallback is unusual, but active predicate use first relies on overlay type data and class image ranges.

Active in YR: Yes. Both helpers call this function through their only callee.

### 3.4 What These Helpers Do Not Check

Neither helper checks target-cell land type, target Buildable, target `AllowTiberium`, target overlay emptiness, bridge/rail flags, live building occupancy, or live `TerrainClass` `SpawnsTiberium`. Those are target germination gates in `CellClass::CanPlaceTiberium @ 0x004838E0`, not source/queue membership gates in this slice.

Growth also does not check `Cell+0xE4`, so an object-list head on an existing ore cell does not block growth queue membership by this helper. Spread does check `Cell+0xE4 == 0` for the source cell.

## 4. INI Keys And Stock Defaults

| Key | Source | Stock YR value | Binary effect in this slice | Active in YR |
|---|---|---:|---|---|
| `[Basic] TiberiumGrowthEnabled` | map INI | standard templates/maps enable it; absent key preserves scenario default | read into `ScenarioClass+0x34A6`; gates `CanGrowTiberium` and both all-type drivers | Conditional |
| `[SpecialFlags] TiberiumSpreads` | map/session special flags | enabled by stock rules/session defaults | bit `0x80`; directly gates `CanSpreadTiberium` | Conditional |
| `[General] TiberiumGrows` | `rulesmd.ini` | `yes` | part of rules/default configuration, but not directly read by these two helpers | Yes as configuration, not direct helper field |
| `[General] TiberiumSpreads` | `rulesmd.ini` | `yes` | feeds scenario/special-flag path; direct helper test is bit `0x80` | Conditional |
| `[General] GrowthRate` | `rulesmd.ini` | `5` | no direct effect on either predicate | No for this slice |
| `[Riparius] GrowthPercentage` | `rulesmd.ini` | `.06` | non-negative, so growth predicate admits eligible cells | Yes |
| `[Riparius] SpreadPercentage` | `rulesmd.ini` | `.06` | non-negative, so spread predicate admits eligible sources if other gates pass | Yes |
| `[Cruentus] GrowthPercentage` | `rulesmd.ini` | `0` | zero passes `CanGrowTiberium`; later processor exits because percentage is not positive | Conditional |
| `[Cruentus] SpreadPercentage` | `rulesmd.ini` | `0` | zero passes `CanSpreadTiberium`; later processor exits because percentage is not positive | Conditional |
| `[Vinifera]` / `[Aboreus]` percentages | `rulesmd.ini` | `.06/.06` | non-negative and positive stock data, though standard map use depends on overlays/assets | Conditional |

Flag evidence:

- `FUN_006B8B30` writes `[SpecialFlags] TiberiumSpreads` from `*param >> 7`.
- `FUN_006B8CA0` reads `[SpecialFlags] TiberiumSpreads` and writes bit 7 with `(read & 1) << 7`.
- `ScenarioClass::Read_INI_Basic @ 0x00689E90` reads `[Basic] TiberiumGrowthEnabled` into `ScenarioClass+0x34A6`.
- `rulesmd.ini` has `[General] TiberiumGrows=yes`, `TiberiumSpreads=yes`; `[Riparius] GrowthPercentage=.06`, `SpreadPercentage=.06`; `[Cruentus] GrowthPercentage=0`, `SpreadPercentage=0`.

## 5. Integration Points

| Integration | Verified behavior | Evidence | Active in YR |
|---|---|---|---|
| Growth queue rebuild | Iterates map cells, filters by current cell tiberium type matching `TiberiumClass+0x98`, then calls `CanGrowTiberium`; accepted entries get priority `0.0` and growth bitmap byte `1`. | `0x007233A0` | Yes |
| Spread queue rebuild | Iterates map cells, filters by current cell tiberium type matching `TiberiumClass+0x98`, then calls `CanSpreadTiberium`; accepted entries get priority `0.0` and spread bitmap byte `1`. | `0x007228B0` | Yes |
| AddToSpreadQueue | Gets cell by coord, calls `CanSpreadTiberium`, then also checks spread bitmap before appending and setting priority jitter. | `0x00722AF0` | Yes |
| Growth driver | Checks `ScenarioClass+0x34A6` before iterating type processors. | `0x00722C40`, assembly `0x00722C48..0x00722C51` | Conditional |
| Spread driver | Checks `ScenarioClass+0x34A6` before iterating type processors, even though `CanSpreadTiberium` separately checks bit `0x80`. | `0x007221B0`, assembly `0x007221B8..0x007221C1` | Conditional |
| Normal `SpreadTiberium(0)` source precheck | Duplicates the `CanSpreadTiberium` source checks inline, then selects a target and calls `CanPlaceTiberium`; forced `SpreadTiberium(1)` bypasses these source checks. | `0x00483780` | Yes/Conditional |

## 6. Current Rust Implementation Status

Rust still uses an RA1-style scan/reservoir model in `src/sim/ore_growth.rs`:

- Growth candidates are any `ResourceType::Ore` with `remaining < MAX_ORE_REMAINING`.
- Spread candidates are `ResourceType::Ore` with `remaining > ORE_BASE_PER_LEVEL * 6`.
- The scan is keyed by `[General] GrowthRate`, not per-`TiberiumClass` queues/timers.
- Gems are excluded by `ResourceType != Ore`, matching stock Cruentus behavior by coincidence for stock data but not by the binary's per-type percentage mechanism.
- `OreGrowthConfig::from_ini` applies `[Basic] TiberiumGrowthEnabled` only to `grows`, while native spread driver also checks `ScenarioClass+0x34A6`.
- `can_germinate` is a target helper that only checks no resource node and optional `PathGrid::is_walkable`; it does not implement binary `CanPlaceTiberium`, but that is outside the two source predicates.

Rust now has some relevant support outside the old scan:

- `src/map/theater.rs` parses `AllowTiberium`.
- `src/map/resolved_terrain.rs` exposes final tile metadata including `allow_tiberium`.
- `src/sim/terrain_spawn.rs` has a separate TIBTRE placement gate and live spawning-terrain index.
- `src/sim/ore_growth.rs` has partial native-shaped growth/spread event queues, but they are not yet the live processor model.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `CanGrowTiberium` entry gate `+0x34A6` | verified | decompile/asm `0x00483620`, `0x00483628..0x00483635` | none |
| `CanGrowTiberium` overlay mapping | verified | `0x00483636..0x00483646`; callee `0x005FDD20` | none |
| `CanGrowTiberium` flat slope gate | verified | `0x00483650..0x0048365D` | none |
| `CanGrowTiberium` max-density-minus-one gate | verified | `0x0048365E..0x00483674` | none |
| `CanGrowTiberium` percentage negative/zero gate | verified | `0x00483675..0x0048368F`; INI defaults | none for normal numeric INI values |
| `CanSpreadTiberium` SpecialFlags bit `0x80` | verified | `0x00483699..0x004836A2`; `0x006B8CA0`; `0x006B8B30` | exact multiplayer session override staging out of scope |
| `CanSpreadTiberium` overlay mapping | verified | `0x004836A3..0x004836B6`; callee `0x005FDD20` | none |
| `CanSpreadTiberium` half-index density gate | verified | `0x004836B7..0x004836CE` | none |
| `CanSpreadTiberium` flat slope gate | verified | `0x004836CF..0x004836DD` | none |
| `CanSpreadTiberium` percentage negative/zero gate | verified | `0x004836DE..0x004836FE`; INI defaults | none for normal numeric INI values |
| `CanSpreadTiberium` source object-list null gate | verified | `0x004836FF..0x0048370C` | none |
| Direct callers | verified | Ghidra callers for `0x00483620`, `0x00483690` | none |
| Target placement gates | touched-for-contrast | sibling report and `0x00483780` | full `CanPlaceTiberium` is out of scope |
| Current Rust predicates | verified-source-scan | `src/sim/ore_growth.rs`, `src/app_init.rs`, `src/rules/ruleset.rs`, `src/map/basic.rs` | future implementation |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-01 - Which mode? -> exhaustive-slice for `CanGrowTiberium` and `CanSpreadTiberium` only.` (evidence: user slot scope)
- `[RESOLVED] OQ-02 - What calls `CanGrowTiberium`? -> only `TiberiumClass::RebuildGrowthQueue @ 0x007233A0` in direct caller list; `GrowTiberium` duplicates similar checks but does not call this helper.` (evidence: Ghidra callers for `0x00483620`; decompile `0x00483710`)
- `[RESOLVED] OQ-03 - What calls `CanSpreadTiberium`? -> `TiberiumClass::AddToSpreadQueue @ 0x00722AF0` and `RebuildSpreadQueue @ 0x007228B0`.` (evidence: Ghidra callers for `0x00483690`)
- `[RESOLVED] OQ-04 - Does growth require scenario flag `+0x34A6`? -> yes, first branch rejects zero.` (evidence: `0x00483628..0x00483635`)
- `[RESOLVED] OQ-05 - Does spread helper require `+0x34A6`? -> no direct helper read; the spread all-type driver checks `+0x34A6`, while helper checks bit `0x80`.` (evidence: `0x00483690`, `0x007221B0`)
- `[RESOLVED] OQ-06 - Which SpecialFlags bit gates spread helper? -> bit `0x80`, `[SpecialFlags] TiberiumSpreads`.` (evidence: `0x00483699`, `0x006B8CA0`, `0x006B8B30`)
- `[RESOLVED] OQ-07 - How is overlay mapped to a tiberium type? -> `OverlayToTiberiumIndex` rejects non-tiberium overlays and matches flat/sloped image ranges across `g_TiberiumClass_Array`.` (evidence: `0x005FDD20`)
- `[RESOLVED] OQ-08 - Does growth check source object occupancy? -> no `Cell+0xE4` read in `0x00483620`.` (evidence: decompile/asm `0x00483620`)
- `[RESOLVED] OQ-09 - Does spread check source object occupancy? -> yes, final return is `Cell+0xE4 == 0`.` (evidence: `0x004836FF..0x00483708`)
- `[RESOLVED] OQ-10 - What density can growth seed? -> `OverlayData < MaxDensity - 1`; stock MaxDensity 12 means data 0..10.` (evidence: `0x0048365E..0x0048366F`)
- `[RESOLVED] OQ-11 - What density can spread seed? -> `OverlayData > tib_index / 2`; type 0 admits data 1..255 by this gate, type 1 admits data 1..255, type 2 admits data 2..255, type 3 admits data 2..255, subject to valid overlay data ranges.` (evidence: `0x004836B7..0x004836C8`)
- `[RESOLVED] OQ-12 - Is the density byte signed? -> loaded with `MOV DL`/`MOV CL` after zeroing the register, so compared as non-negative byte widened to int.` (evidence: `0x00483664..0x0048366D`, `0x004836B9..0x004836C6`)
- `[RESOLVED] OQ-13 - Is percentage zero accepted by the helpers? -> yes; helpers reject negative/unordered through x87 C0 test, not `<= 0`; processors later exit on non-positive percentages.` (evidence: `0x00483675..0x00483686`, `0x004836E7..0x004836F8`; processor reports)
- `[RESOLVED] OQ-14 - Do these helpers check target land/buildable/AllowTiberium? -> no; those are target gates in `CanPlaceTiberium`, not in this source predicate slice.` (evidence: decompile `0x00483620`, `0x00483690`; contrast `0x00483780`)
- `[RESOLVED] OQ-15 - Are stock Riparius percentages active? -> yes, `.06/.06` in `rulesmd.ini`; stock ore is admitted by percentage gates when other gates pass.` (evidence: `ini/rulesmd.ini`)
- `[RESOLVED] OQ-16 - Are stock Cruentus zero percentages a seed blocker? -> no for these helpers; zero passes helper gates, but later processors exit on `<= 0`.` (evidence: `0x00483675`, `0x004836E7`; `rulesmd.ini [Cruentus]`)
- `[RESOLVED] OQ-17 - What is current Rust's biggest predicate mismatch? -> shared RA1 scan model with Ore-only thresholds, `GrowthRate`, and no native per-type source predicates.` (evidence: `src/sim/ore_growth.rs`)
- `[DEFERRED] OQ-18 - Exact `CanPlaceTiberium` target predicate. (category: out-of-scope; reason: user explicitly limited this slot to `CanGrowTiberium`/`CanSpreadTiberium`; next-step-if-pursued: use existing `CanPlaceTiberium` reports or run a target-placement slice.)`
- `[DEFERRED] OQ-19 - Multiplayer/session staging that may override bit `0x80`. (category: requires-different-system-context; reason: this slice verified the helper bit and SpecialFlags parser, not lobby/session transfer; next-step-if-pursued: trace scenario special flags staging and game-mode overrides.)`

Adversarial corner cases answered:

- Missing overlay: rejected by `OverlayToTiberiumIndex == -1`.
- Slope nonzero: rejected by both helpers before percentage checks.
- Full-density stock ore data `11`: rejected by growth, admitted by spread if other gates pass.
- Stock gem zero percentage: helper-admitted if current overlay/type/density gates pass, processor-inactive later.
- Source occupied by any object-list head: blocks spread helper but not growth helper.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Growth source predicate is per current tiberium overlay, scenario `+0x34A6`, flat slope, `OverlayData < MaxDensity - 1`, and non-negative `GrowthPercentage`. | `0x00483620`; assembly `0x00483628..0x00483686`; `rulesmd.ini` | Rust scans only `ResourceType::Ore` and `remaining < MAX`, keyed by `[General] GrowthRate`. | `src/sim/ore_growth.rs`, future per-type queue seed/rebuild API, overlay/resource type model | Seed growth queue from post-Unlimbo overlay cells using binary predicate, not resource-node-only ore scan. | `ore_queue_can_grow_predicate_rejects_slope_and_data_11_but_accepts_data_10` | Do not use one hardcoded ore-only growth predicate for all tiberium types. |
| Spread source predicate is bit `0x80`, valid current tiberium overlay, `OverlayData > tib_index / 2`, flat slope, non-negative `SpreadPercentage`, and `Cell+0xE4 == 0`; all-type spread driver also requires `ScenarioClass+0x34A6`. | `0x00483690`; assembly `0x00483699..0x00483708`; driver `0x007221B0` | Rust spread uses `ResourceType::Ore`, `remaining > 6 levels`, and does not apply `[Basic] TiberiumGrowthEnabled` to spread. | `src/sim/ore_growth.rs`, `src/app_init.rs`, `src/map/basic.rs`, `src/sim/production/production_types.rs` | Seed/process spread only when both driver gate and source predicate pass; source object-list blocks spread membership. | `yr_tiberium_growth_enabled_false_suppresses_spread_driver`; `ore_queue_can_spread_predicate_requires_empty_source_object_list` | Do not treat `TiberiumGrowthEnabled` as growth-only in the native queue model. |
| Zero `GrowthPercentage`/`SpreadPercentage` passes source predicates; processor gates decide whether zero-percent classes do work. | `0x00483675..0x00483686`, `0x004836E7..0x004836F8`; stock `[Cruentus] 0/0`; processor reports | Rust hardcodes gems out of growth/spread by `ResourceType != Ore`, which matches stock effect but not mechanism. | rules tiberium type data, `src/sim/ore_growth.rs`, world hash/snapshot state | Preserve per-type data and allow queue seed membership for zero-percent types while processors exit on `<= 0`. | `cruentus_zero_percentage_can_seed_membership_but_processor_exits` | Do not use percentage `== 0` as a queue rebuild predicate blocker. |

## 10. Negative Facts / Do Not Do

- Do not put land-type Buildable, tile `AllowTiberium`, target overlay emptiness, buildings, or live TIBTRE target rejection into these two source predicates. Those belong to `CanPlaceTiberium`.
- Do not use a shared growth/spread queue candidate predicate. Growth and spread differ by density, flags, source object list, and max-density handling.
- Do not block queue rebuild membership for zero-percent tiberium classes. The helpers reject negative/unordered, not zero.
- Do not treat `Cell+0xE4` as a growth source gate. It is only in `CanSpreadTiberium` among the two helpers.
- Do not treat `[General] GrowthRate` as evidence for these helpers. It is not read by `0x00483620` or `0x00483690`.

## 11. Remaining Uncertainty

- Exact multiplayer/session staging that can force or clear SpecialFlags bit `0x80` after INI parse is out of scope. The helper bit and SpecialFlags parser mapping are verified.
- x87 unordered/NaN comparison behavior is visible from the status-bit test, but INI parsing does not normally create NaN percentages; no runtime NaN case was tested.

## 12. Stale Docs / Follow-up Docs

- [docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md): replace wording saying gems are suppressed by `GrowthPercentage=0` at `CanGrowTiberium` with: "`CanGrowTiberium` rejects negative/unordered `GrowthPercentage`, not zero. Stock Cruentus with `GrowthPercentage=0` can pass this source predicate if overlay/density/slope gates pass; the later growth processor exits on `GrowthPercentage <= 0.0`."
- [docs/research/PLACETIBERIUM_SPREAD_GERMINATION_CONSTRAINTS_AND_OVERLAY_FRAME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PLACETIBERIUM_SPREAD_GERMINATION_CONSTRAINTS_AND_OVERLAY_FRAME_GHIDRA_REPORT.md): keep target validation separate from these helpers; if mentioning `CanSpreadTiberium`, state that it is a source predicate and does not check target land/buildable/AllowTiberium.
- [docs/research/TIBERIUMCLASS_MAP_LOAD_QUEUE_SEEDING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBERIUMCLASS_MAP_LOAD_QUEUE_SEEDING_GHIDRA_REPORT.md): existing wording that zero-percent classes can seed but processors later exit is consistent with this report.

## Sources

- Ghidra decompiled: `0x00483620`, `0x00483690`, `0x005FDD20`, `0x007233A0`, `0x007228B0`, `0x00722AF0`, `0x00483780`, `0x007221B0`, `0x00722C40`, `0x006B8B30`, `0x006B8CA0`, `0x00689E90`.
- Ghidra assembly contexts: `0x00483620`, `0x00483690`, `0x007221B0`, `0x00722C40`.
- INI checked: `ini/rulesmd.ini` `[General]`, `[Tiberiums]`, `[Riparius]`, `[Cruentus]`, `[Vinifera]`, `[Aboreus]`.
- Rust scanned: `src/sim/ore_growth.rs`, `src/app_init.rs`, `src/rules/ruleset.rs`, `src/map/basic.rs`, `src/map/theater.rs`, `src/map/resolved_terrain.rs`, `src/sim/terrain_spawn.rs`.
- Prior reports referenced for contrast: [TIBERIUMCLASS_MAP_LOAD_QUEUE_SEEDING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBERIUMCLASS_MAP_LOAD_QUEUE_SEEDING_GHIDRA_REPORT.md), [PLACETIBERIUM_SPREAD_GERMINATION_CONSTRAINTS_AND_OVERLAY_FRAME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PLACETIBERIUM_SPREAD_GERMINATION_CONSTRAINTS_AND_OVERLAY_FRAME_GHIDRA_REPORT.md), [TIBERIUMCLASS_GROWTH_PROCESSOR_EXACT_QUEUE_PROCESSING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBERIUMCLASS_GROWTH_PROCESSOR_EXACT_QUEUE_PROCESSING_GHIDRA_REPORT.md).
