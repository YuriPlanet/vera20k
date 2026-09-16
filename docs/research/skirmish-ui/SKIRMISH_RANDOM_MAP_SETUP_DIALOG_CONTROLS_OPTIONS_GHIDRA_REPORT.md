# Skirmish Random Map Setup Dialog Controls / Options - Ghidra Research Report

**Address(es):** `0x00595BC0`, `0x00596300`, `0x00595680`, `0x00596C70`, `0x00596E50`, `0x00597260`, `0x005975E0`, `0x005E8590`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** the random-map setup dialog opened from Choose Map command `0x583`: dialog resource/proc path, user-visible setup controls, initial/default seed-object values, control-to-field synchronization, Generate/Randomize/OK/Cancel behavior, enable/disable rules visible to the player, and which setup fields later feed `RandMap.Sed` / preview generation.  
**Non-Scope:** full `RandMap.Sed` serialized byte layout, full random terrain formulas, exact dialog DLU visual geometry, final Choose Map sentinel insertion semantics except where setup result gates them, and launch-time `.SED` map generation internals.  
**Confidence:** High for dialog path, control IDs, field offsets, defaults, result values, button gates, and setup-field consumers. Medium for some button captions where semantics are inferred from helper behavior rather than a resource text dump.  
**Active in YR:** Conditional. The path is live in standard YR offline Skirmish when Choose Map `Create Random Map` command `0x583` is clicked and selected mode allows random maps; the setup dialog only commits when it returns result `1`.

## 0. Working Notes Gate

**Target question:** What exact user-visible controls/options does the random-map setup dialog expose, what defaults and enable/disable rules does it use, and what field state is consumed by later preview / `RandMap.Sed` handoff?

**Non-goals:** Do not re-investigate full file writer layout, full terrain generation, Choose Map list paint, ordinary map preview refresh, or Start/launch setup beyond naming fields consumed after this dialog.

**Evidence needed to mark COMPLETE:** Prove dialog resource/proc path, command entry from `0x005E8590`, init/defaults, control IDs and field writes, Randomize and Generate command behavior, OK/Cancel result values, controls temporarily disabled during generation, post-dialog preview write/cleanup, and current Rust deltas.

**Stop conditions:** Stop after the setup dialog's control/options contract is proven. Defer only resource text/visual geometry and full generator/file-format details owned by sibling slots.

## 1. Overview

Choose Map command `0x583` calls `0x005E8590`, which calls random-map dialog pump `0x00595BC0`. That pump creates dialog resource `0x105` with dialog proc `0x00596300`, stores the modal result in a stack int through `GWLP_USERDATA`, and returns that result to `0x005E8590`. `0x005E8590` aborts unless the result is exactly `1`.

The dialog edits the global `MapSeedClass` object at `0x00ABDFD8`. Combo/spin/edit controls write option fields, Randomize replaces a subset with random choices, Generate runs preview-time generation `0x00598960(1, hwnd)` and `GenerateTerrainPreview`, and OK either accepts an existing preview or generates one before setting result `1`. Cancel sets result `2`.

Active in YR: Conditional. Evidence: `0x005E8590` decompile; assembly context `0x00595BD8..0x00595BE3` sets `EDX=0x596300`, `ECX=0x105`, then calls `0x00622650`; `0x00622650` calls `CreateDialogIndirectParamA`.

## 2. Key State / Offsets

| Item | Meaning | Evidence | Active in YR |
|---|---|---|---|
| dialog resource `0x105` | random-map setup dialog | assembly `0x00595BDD MOV ECX,0x105`; `0x00622650` creates dialog from template | Conditional |
| dialog proc `0x00596300` | random-map setup message handler | assembly `0x00595BD8 MOV EDX,0x596300`; `0x00622650` passes DLGPROC | Conditional |
| stack modal result | result slot pointed to by `GWLP_USERDATA` offset `8` | `0x00595BEE..0x00595C02`; `0x00596300` writes `*puVar5` | Conditional |
| `DAT_00ABDFD8` | active `MapSeedClass` setup/options object | `0x00595BCA`; `0x00596C70`; `0x00598960` | Conditional |
| `DAT_00ABE050` / seed `+0x78` | random-map description/display buffer | constructor `0x00595680`; Randomize/Generate load string id `0xF5E`; save wrapper passes `param_1+0x1E` | Conditional |
| `DAT_00ABE154` | generated preview wrapper | generate command, paint path, `0x00595BC0` shutdown writer | Conditional |
| `DAT_00ABE150` | saved copy of generated-preview seed object | Generate allocates/copies `0x5E` dwords from `0x00ABDFD8` | Conditional |
| `DAT_00ABE2D8` | dirty/options-changed flag used by sync/generate flow | set by `0x00596C70`, Randomize, init; cleared after derived randomization | Conditional |

## 3. Dialog Path And Result Values

### 3.1 Resource/proc path

Active in YR: Conditional on `0x583` click. `0x005E8590` calls `0x00595BC0`. In `0x00595BC0`, the binary initializes RMG settings (`ECX=0x00ABDFD8; CALL 0x005981F0`), sets dialog proc `0x00596300`, sets resource `0x105`, pushes zero extra data, and calls `0x00622650`.

Evidence: decompile `0x005E8590`; assembly context `0x00595BCA..0x00595BE3`; `0x00622650` decompile.

### 3.2 Modal pump and result ownership

Active in YR: Conditional. `0x00595BC0` stores a local int initialized to `0` in dialog user-data offset `8` and pumps until that int becomes nonzero or the outer shell loop exits. On return it destroys the dialog and returns the int.

Evidence: decompile `0x00595BC0`; assembly `0x00595BEE..0x00595C42`.

### 3.3 Result values

Active in YR: Conditional.

- Command `0x5C0` writes result `2` and returns `1`; this is Cancel.
- Command `0x6C5` writes result `1` and returns `1`; this is OK/Create/accept.
- Any non-`1` result makes `0x005E8590` return `-1`, so Choose Map does not commit random-map setup.

Evidence: decompile `0x00596300` command cases; decompile `0x005E8590` result gate.

## 4. Controls / Fields

The setup dialog synchronizes owner-draw/custom combo/spin/edit controls into `MapSeedClass` fields through `0x00596C70`, then clamps through `0x005975E0`.

| Control ID | User-visible role | Field(s) written | Details | Evidence | Active in YR |
|---|---|---|---|---|---|
| `0x405` | map type / landform combo | `+0x3C` | current selection item-data read with `0x147` then `0x150`; change sets dirty flag | `0x00596C81..0x00596CC0`; `0x00596E50` populates from `TXT_MAP_*` table | Conditional |
| `0x407` | theater combo | `+0x38` | item-data is theater index; source table walks theater strings from `0x007E1B78` | `0x00596C70`; `0x00596E50`; generator report | Conditional |
| `0x408` | resources combo | `+0x40` | item-data `0..3` | `0x00596D0A..0x00596D33`; seed loader key `Resources` | Conditional |
| `0x3EA` | time-of-day combo | `+0x48` | item-data `0..3`; edit/change notification disables accept/generate until resync | `0x00596C70`; `0x00596E50`; command notification branch | Conditional |
| `0x406` | size combo | `+0x64` and `+0x68` | one selection writes both width and height options to the same item-data value | `0x00596C70`; `0x005975E0` | Conditional |
| `0x3EB` | player count spin/control | `+0x50` | reads message `0x400`; display sync sends range-like `0x406` with `0x80002`, then value message `0x405` | `0x00596C70`; `0x00596E50`; clamp `2..8` | Conditional |
| `0x3FB` | seed edit/static text | `+0x74` display path verified | display sync formats seed with `FUN_007C8EF4` then sends message `0x4B4`; typed seed commit not drained | `0x00596E50`; constructor/default `+0x74=-1`; init randomizes | Conditional |
| `0x468` | preview child | consumes `DAT_00ABE154` | paint draws generated preview through `DrawStartPositions` unless suppression helper says no | `0x00596300` WM_PAINT; assembly `0x00596ACC -> 0x00640710` | Conditional |
| `0x620` | Generate | runs preview-time generation | disables controls, syncs fields, calls `0x00598960(1, hwnd)`, calls `GenerateTerrainPreview`, re-enables controls, copies seed object | `0x00596300` command `0x620`; preview report | Conditional |
| `0x621` | Randomize | randomizes selected options | syncs, repopulates display, writes random fields, clears preview, disables accept/load-like button, invalidates | `0x00596300` command `0x621`; `0x00597260`; `0x005975E0` | Conditional |
| `0x6C2` | saved-seed file/list action, likely load/select | helper mode `1`; may post Generate | `FUN_005587F0` writes helper state `+4=1`; command case posts `0x620` on success | `0x00596300`; helper decompile | Conditional |
| `0x6C3` | saved-seed file/list action, likely save/name | helper mode `2`; enables `0x6C2/0x6C4` by saved-file availability | `FUN_00558810` writes helper state `+4=2`, stores description ptr | `0x00596300`; helper decompile | Conditional |
| `0x6C4` | saved-seed file/list action, likely delete | helper mode `3`; enables `0x6C2/0x6C4` by saved-file availability | `FUN_00558840` writes helper state `+4=3` | `0x00596300`; helper decompile | Conditional |
| `0x6C5` | OK/Create/accept | result `1` after ensuring preview exists | syncs fields; if no preview, runs preview generation path before result write | `0x00596300` case `0x6C5`; string `RandMap.Map @ 0x0082BB44` | Conditional |
| `0x5C0` | Cancel | result `2` | no setup save/commit in `0x005E8590` | `0x00596300` case `0x5C0`; `0x005E8590` non-1 abort | Conditional |

## 5. Defaults And Clamps

### 5.1 Constructor defaults

Active in YR: Conditional; these are the seed-object defaults before dialog init mutates seed and clamps values. `MapSeedClass__Constructor @ 0x00595680` initializes:

| Field | Default | Meaning from consumers | Evidence |
|---|---:|---|---|
| `+0x38` | `0` | theater index | constructor; generator/loader report |
| `+0x3C` | `1` | map type / landform | constructor |
| `+0x40` | `1` | resources option | constructor |
| `+0x44` | `0` | ruggedness percent | constructor |
| `+0x48` | `1` | time bucket | constructor |
| `+0x4C` | `0` | water amount percent | constructor |
| `+0x50` | `2` | number of players | constructor |
| `+0x54` | `0`, later clamped to `1` | tiberium percent | constructor; `0x005975E0` |
| `+0x58` | `0` | tiberium layout | constructor |
| `+0x5C` | `0` | vegetation percent | constructor |
| `+0x60` | `0` | urban presence percent | constructor |
| `+0x64` | `0` | width option | constructor |
| `+0x68` | `0` | height option | constructor |
| `+0x6C` | `0` | accessibility percent | constructor |
| `+0x70` | `0` | region size percent | constructor |
| `+0x74` | `-1` | seed sentinel; dialog init replaces with `RandomRanged(0,0xFFFF)` | constructor; init branch |
| `+0x78` | localized string id `0xF5E` if string table is available | description/display buffer | constructor `0x00595680`; helper `0x00595710` |

### 5.2 Dialog init behavior

Active in YR: Conditional. On message `0x497`, the proc:

1. sets `_DAT_0082B02C = -1`;
2. enables Generate `0x620` only when `g_IsMapEditor == 0`;
3. if `MapSeed+0x74 == -1`, replaces it with `RandomRanged(0,0xFFFF)` and sets dirty flag;
4. sets `DAT_0082B030 = 1`;
5. calls display sync `0x00596E50`;
6. enables `0x6C2` and `0x6C4` from saved-seed availability helper `0x00559C20`;
7. disables `0x6C5` and `0x6C3`.

Evidence: decompile `0x00596300` `param_2 == 0x497` branch.

### 5.3 Clamp ranges

Active in YR: Conditional. `0x005975E0` clamps:

| Field | Clamp |
|---|---|
| `+0x40`, `+0x48`, `+0x64`, `+0x68` | `0..3` |
| `+0x3C` | `0..4` |
| `+0x44`, `+0x4C`, `+0x58`, `+0x5C`, `+0x60`, `+0x6C`, `+0x70` | `0..100` |
| `+0x50` | `2..8` |
| `+0x54` | `1..100` |
| `+0x74` | `0..0xFFFF` |

Evidence: `0x005975E0` decompile.

## 6. Randomize And Generate

### 6.1 Randomize command `0x621`

Active in YR: Conditional. Randomize first syncs current controls and calls display sync, then writes:

- `+0x38 = (RandomRanged(0,100) > 0x31)`, so this only produces `0` or `1`;
- `+0x3C = RandomRanged(1,4)`;
- `+0x48 = RandomRanged(0,3)`;
- `+0x40 = RandomRanged(0,3)`;
- `+0x64 = +0x68 = RandomRanged(0,3)`;
- derived fields through `0x00597260(+0x3C)`;
- description/display text string id `0xF5E`;
- `+0x74 = RandomRanged(0,0xFFFF)`;
- clamps through `0x005975E0`.

Then it destroys any existing `DAT_00ABE154` preview wrapper, sets it null, disables `0x6C5` and `0x6C3`, and invalidates the dialog.

Evidence: `0x00596300` command `0x621`; helper `0x00597260`; clamp `0x005975E0`; standalone randomizer `0x00597380`.

### 6.2 Derived random fields from map type

Active in YR: Conditional. `0x00597260(seed, map_type)` fills water amount, ruggedness, urban presence, accessibility, region size, tiberium amount (`resources * 0x14`), tiberium layout, vegetation, and seed. Vegetation min/max inputs are individually clamped to `0..100` and if max < min the min is reduced to max before `RandomRanged(min,max)`.

Evidence: `0x00597260` decompile.

### 6.3 Generate command `0x620`

Active in YR: Conditional. Generate syncs controls through `0x00596C70`, clears scenario/map scratch globals and scenario waypoint/count fields, disables controls `0x405`, `0x3EA`, `0x407`, `0x406`, `0x408`, `0x3EB`, `0x621`, `0x620`, `0x6C2`, `0x6C3`, `0x6C4`, `0x6C5`, `0x5C0`, resets description/display text to string id `0xF5E`, calls `0x00598960(1, hwnd)` and `GenerateTerrainPreview`, re-enables the same controls, allocates/copies `0x00ABDFD8` into `DAT_00ABE150`, and posts `WM_PAINT`.

Evidence: `0x00596300` command `0x620`; preview report [GENERATETERRAINPREVIEW_RANDMAP_DIMENSIONS_COLORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/GENERATETERRAINPREVIEW_RANDMAP_DIMENSIONS_COLORS_GHIDRA_REPORT.md).

Important tiny detail: Cancel `0x5C0` is disabled during the synchronous Generate block and re-enabled after preview generation. Rust should not allow state mutation/cancel midway through a native-equivalent generation action unless it deliberately models native's modal blocking.

### 6.4 OK/Create command `0x6C5`

Active in YR: Conditional. OK first syncs current controls. If not in map editor and a generated preview wrapper exists with nonzero inner surface, it accepts immediately. Otherwise it calls `0x00598960(1, hwnd)` to generate. In standard offline Skirmish, acceptance writes result `1`.

Evidence: `0x00596300` command `0x6C5`; `RandMap.Map` string anchor `0x0082BB44`; result write at `LAB_00596A90`.

## 7. What Later Consumers Read

Active in YR: Conditional. The setup dialog leaves `0x00ABDFD8` populated, and accepted `0x005E8590` calls save wrapper `0x00597730("RandMap.Sed")`. Later launch load reads `[RandomMap]` keys corresponding to the same field set.

| Field | Later consumer | Evidence |
|---|---|---|
| `+0x38` theater | `.SED` loader and generator theater init | generator report |
| `+0x3C` map type | terrain/water/island branches, optional tech building branch | `0x00598960` |
| `+0x40` resources | resource/tiberium derived amount and `.SED` key `Resources` | loader/generator reports |
| `+0x44`, `+0x4C`, `+0x58`, `+0x5C`, `+0x60`, `+0x6C`, `+0x70` | terrain density/region/accessibility generation | `0x00597260`; `.SED` loader key list |
| `+0x48` time | lighting/time table | generator report |
| `+0x50` players | generated start count | generator report `0x005A1FB0` |
| `+0x54` tiberium | tiberium generation amount | generator report |
| `+0x64`, `+0x68` width/height | generated map dimensions | generator report `0x00599650` |
| `+0x74` seed | deterministic RMG random table seed | generator report `0x0059897B..0x0059899B` |
| `+0x78` description | saved with seed file and used for sentinel display | `0x00597730`; `0x005E8590` record update path |

## 8. INI Keys

No normal rules/map INI key configures the setup dialog controls directly in this slice. The dialog writes a `MapSeedClass`; save/load later uses `[RandomMap]` keys for persistence:

`Description`, `Width`, `Height`, `NumPlayers`, `Seed`, `MapType`, `Theater`, `Time`, `RegionSize`, `Ruggedness`, `Accessibility`, `WaterAmount`, `Tiberium`, `TiberiumLayout`, `Vegetation`, `UrbanPresence`, `Resources`.

Evidence: sibling generator report verifying loader `0x00597A30`; local research-index and `rg` scan found no direct `rulesmd.ini` setup-dialog key ownership for this UI.

## 9. Current Rust Implementation Status

Current Rust recognizes Choose Map `CreateRandomMap0x583` but does not open/model this native setup dialog.

| Rust area | Status | Evidence |
|---|---|---|
| app command `0x583` | missing: logs only; does not open setup dialog | `src/app.rs:941` |
| choose-map state sentinel helper | partial: can upsert/highlight sentinel if called, but no setup dialog/result gate | `src/ui/skirmish_shell/state/choose_map.rs:144` |
| random sentinel fields | mostly aligned for file/min/max/official after recent work; still no setup digest/source/options object | `src/skirmish_scenarios.rs:107`, `src/skirmish_scenarios.rs:210` |
| `RandMap.img` preview branch | present/partial: runtime random preview filename is recognized; setup dialog generation is still absent | `src/app_skirmish_shell_render/preview.rs:99`, `src/app_skirmish_shell_render/preview.rs:113` |
| setup controls/options model | missing | `rg MapSeed src` found no native seed/options dialog model |
| `.SED` launch generation | missing in this slot's scan; sibling launch report owns details | generator report; current Rust launch/load surfaces |

## 10. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| target question / non-goals / stop conditions | verified | section 0 | none |
| `0x583 -> 0x005E8590 -> 0x00595BC0` liveness | verified | `0x005E8590`; prior Choose Map reports | none for setup dialog |
| dialog resource/proc path | verified | assembly `0x00595BD8..0x00595BE3`; `0x00622650` | exact DLU geometry out-of-scope |
| modal result storage | verified | `0x00595BEE..0x00595C42`; `0x00596300` | none |
| constructor defaults | verified | `0x00595680` | none |
| init message `0x497` | verified | `0x00596300` | none |
| field sync controls | verified | `0x00596C70`; `0x00596E50` | typed seed edit commit not separately drained |
| clamps | verified | `0x005975E0` | none |
| Randomize `0x621` | verified | `0x00596300`; `0x00597260`; `0x00597380` | exact string captions out-of-scope |
| Generate `0x620` | verified | `0x00596300`; `0x00598960`; preview report | terrain formulas out-of-scope |
| OK/Cancel result values | verified | `0x00596300`; `0x005E8590` | none |
| file/list actions `0x6C2/0x6C3/0x6C4` | touched-not-exhausted | helper decompiles `0x005587F0`, `0x00558810`, `0x00558840`, availability `0x00559C20` | exact resource captions and file-dialog UX |
| Rust delta | verified | `rg` / file reads listed above | implementation not performed |

## 11. Open Questions - Final State

- `[RESOLVED] OQ-01 - Is the setup dialog live in YR? -> Yes, conditionally from standard Skirmish Choose Map `0x583` through `0x005E8590`.` (evidence: `0x005E8590`; prior `0x583` reports)
- `[RESOLVED] OQ-02 - Which dialog resource/proc is used? -> resource `0x105`, proc `0x00596300`.` (evidence: `0x00595BD8..0x00595BE3`, `0x00622650`)
- `[RESOLVED] OQ-03 - Where is the modal result stored? -> stack local int exposed via dialog user-data offset `8`.` (evidence: `0x00595BEE..0x00595C02`)
- `[RESOLVED] OQ-04 - What does Cancel return? -> result `2`.` (evidence: `0x00596300` command `0x5C0`)
- `[RESOLVED] OQ-05 - What does OK/Create return? -> result `1` after sync/generation checks.` (evidence: `0x00596300` command `0x6C5`)
- `[RESOLVED] OQ-06 - Does non-1 setup result commit random map? -> No; `0x005E8590` returns `-1`.` (evidence: `0x005E8590`)
- `[RESOLVED] OQ-07 - What are constructor defaults? -> field table in section 5.1; seed starts `-1` and description uses string id `0xF5E`.` (evidence: `0x00595680`)
- `[RESOLVED] OQ-08 - What happens to seed on first init? -> `-1` seed is replaced by `RandomRanged(0,0xFFFF)`.` (evidence: `0x00596300` message `0x497`)
- `[RESOLVED] OQ-09 - Which controls write option fields? -> controls `0x405/0x407/0x408/0x3EA/0x406/0x3EB` write the core field set through `0x00596C70`.` (evidence: `0x00596C70`)
- `[RESOLVED] OQ-10 - What are clamp ranges? -> section 5.3.` (evidence: `0x005975E0`)
- `[RESOLVED] OQ-11 - What does Randomize do? -> writes random theater/map type/resources/time/size/seed plus derived fields, destroys preview, disables accept/load-like button.` (evidence: `0x00596300` `0x621`; `0x00597260`)
- `[RESOLVED] OQ-12 - What does Generate do? -> syncs controls, disables all interactive controls, runs preview generation, re-enables controls, copies seed object, posts paint.` (evidence: `0x00596300` `0x620`)
- `[RESOLVED] OQ-13 - What fields are consumed later? -> the same `MapSeedClass` fields are saved to `RandMap.Sed` and read by `.SED` loader/generator.` (evidence: `0x00597730`; generator report)
- `[RESOLVED] OQ-14 - Is this TS-only legacy? -> No; it is the active YR random-map setup path, conditional on user command and selected-mode availability.` (evidence: `0x005E8590`; dialog path)
- `[RESOLVED] OQ-15 - Does current Rust implement the setup dialog? -> No; app command logs only and no MapSeed setup model exists.` (evidence: `src/app.rs:941`; `rg MapSeed src`)
- `[DEFERRED] OQ-16 - Exact resource DLU positions/captions for every dialog child.` (category: out-of-scope; reason: this slot owns control behavior/options, not pixel layout; next-step-if-pursued: resource dump and visual report)
- `[DEFERRED] OQ-17 - Exact typed seed edit commit path for control `0x3FB`.` (category: bounded-cost-too-high; reason: display sync and generation consumers are proven, but edit notification path needs a narrower UI-control message pass; next-step-if-pursued: trace all `0x3FB` xrefs/messages)
- `[DEFERRED] OQ-18 - Full saved-seed file browser UX for `0x6C2/0x6C3/0x6C4`.` (category: out-of-scope; reason: helpers are touched enough to identify side effects, but not needed for core Create Random Map acceptance; next-step-if-pursued: investigate `0x005587F0/0x00558810/0x00558840` callers and resource text)

## 12. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Proposed test name | Risk / do-not-do |
|---|---|---|---|---|---|---|---|
| `0x583` opens modal setup dialog `0x105` and only commits when that dialog returns exactly `1`; Cancel returns `2` and aborts. | `0x00595BD8..0x00595BE3`; `0x00596300`; `0x005E8590` | missing: `src/app.rs:941` logs only | app modal routing plus new random-map setup state | Add a real setup dialog/state gate before sentinel commit; do not upsert/commit on button press alone. | Click Create Random Map, cancel setup, prior Choose Map selection remains unchanged and no `RandMap.Sed` setup is saved. | `skirmish_random_map_setup_cancel_returns_2_and_preserves_selection` | Do not treat `0x583` as immediate sentinel creation. |
| Setup options live in a `MapSeedClass` field model with constructor defaults, init seed randomization, and native clamp ranges. | `0x00595680`; `0x00596300` `0x497`; `0x005975E0` | missing: no seed/options model | new random-map setup model; eventual `.SED` save/load parser | Represent fields `+0x38..+0x78` with defaults and clamp behavior before Generate/OK. | Fresh setup dialog has players `2`, map type `1`, resources `1`, time `1`, seed randomized into `0..0xFFFF`, tiberium clamped to `1`. | `skirmish_random_map_setup_initializes_native_defaults_and_seed` | Do not invent stock-looking defaults from UI labels or INI. |
| Randomize `0x621` mutates the same setup object, clears preview, disables accept/load-like controls, and uses map-type tables for derived fields. | `0x00596300` `0x621`; `0x00597260`; `0x00597380` | missing | setup dialog controls/options model | Implement Randomize as state mutation plus preview invalidation; acceptance disabled until regeneration. | Generate preview, press Randomize, preview becomes invalid and OK/Create is disabled until Generate/OK regeneration path runs. | `skirmish_random_map_randomize_invalidates_preview_and_disables_accept` | Do not leave stale `RandMap.img`/preview accepted after changing options. |
| Generate `0x620` syncs controls, disables all interactive controls including Cancel during synchronous preview generation, calls `0x00598960(1, hwnd)` plus `GenerateTerrainPreview`, re-enables controls, and stores a copy of the seed object. | `0x00596300` `0x620`; preview report | missing setup flow; preview decode branch exists but generation is absent | setup dialog, random preview generation, app UI blocking | Add a synchronous or native-equivalent generation action that produces preview state and a generated seed snapshot. | Press Generate with changed options; controls are unavailable during generation; after success OK is enabled and saved setup matches generated preview. | `skirmish_random_map_generate_syncs_options_and_copies_seed_snapshot` | Do not make Generate a passive preview reload from normal map data. |
| OK `0x6C5` accepts existing generated preview when present, otherwise forces preview-time generation before returning `1`. | `0x00596300` `0x6C5` | missing | setup dialog accept path | OK must ensure generation before success; accept result then lets `0x005E8590` save `RandMap.Sed` and load `RandMap.img`. | Press OK on a fresh setup without pressing Generate; setup still generates and returns success rather than committing empty preview/options. | `skirmish_random_map_ok_generates_when_preview_missing` | Do not allow successful setup with no generated seed/preview state. |

## 13. Negative Facts / Do Not Do

- Do not implement Create Random Map as immediate selection of a sentinel row. Active in YR: No. Evidence: setup dialog result gate in `0x005E8590`.
- Do not commit setup on Cancel or any non-`1` dialog result. Active in YR: No. Evidence: Cancel writes `2`; `0x005E8590` aborts unless result `1`.
- Do not use process/wall-clock randomness for launch after setup has a seed. Active in YR: No. Evidence: setup clamps/stores `+0x74`; generator seeds from `+0x74` in sibling report.
- Do not keep OK/Create enabled after option changes or Randomize without regenerating. Active in YR: No. Evidence: change notifications and Randomize disable `0x6C5`; Generate/OK re-enable/accept after generation.
- Do not let the player cancel or mutate controls midway through native-equivalent synchronous Generate. Active in YR: No. Evidence: `0x620` disables every listed interactive control, including `0x5C0`, until generation returns.

## 14. Remaining Uncertainty

- Exact DLU geometry and localized captions for dialog `0x105` remain out of scope.
- Exact typed edit commit path for seed control `0x3FB` remains deferred; display/init/generation seed ownership is verified.
- Exact saved-seed file UX for controls `0x6C2/0x6C3/0x6C4` remains deferred; helper mode side effects were touched only to avoid misclassifying core setup controls.

## 15. Stale Docs / Follow-up Docs

Prior report [docs/research/skirmish-ui/SKIRMISH_CREATE_RANDOM_MAP_0X583_SETUP_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CREATE_RANDOM_MAP_0X583_SETUP_PATH_GHIDRA_REPORT.md) has stale current-Rust preview wording in Section 6 after later Rust changes. Replacement wording:

> `preview` - partial: Rust now recognizes `RandMap.Sed` and attempts to read runtime `RandMap.img` for random sentinel previews, but the native setup dialog/generation path that creates that image and seed snapshot is still missing.

No binary-behavior contradiction with prior Create Random Map reports was found; this report refines the setup-dialog control/default contract.

## Sources

- Ghidra read-only decompile / assembly context: `0x00595BC0`, `0x00595BCA..0x00595BE3`, `0x00595BEE..0x00595C42`, `0x00596300`, `0x00595680`, `0x00596C70`, `0x00596E50`, `0x00597260`, `0x00597380`, `0x005975E0`, `0x00597730`, `0x005E8590`, `0x00622650`, `0x005587F0`, `0x00558810`, `0x00558840`, `0x00559C20`, `0x00598960`.
- String anchors: `MapGen.cpp @ 0x0082BA48`, `TXT_RANDOM_MAP_DESCRIPTION @ 0x0082BA2C`, `RandMap.Map @ 0x0082BB44`, `RandMap.img @ 0x00829ABC`, `RandMap.Sed @ 0x0082BC30`.
- Prior docs read: [SKIRMISH_CREATE_RANDOM_MAP_0X583_SETUP_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CREATE_RANDOM_MAP_0X583_SETUP_PATH_GHIDRA_REPORT.md), [GENERATETERRAINPREVIEW_RANDMAP_DIMENSIONS_COLORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/GENERATETERRAINPREVIEW_RANDMAP_DIMENSIONS_COLORS_GHIDRA_REPORT.md), `SKIRMISH_RANDOM_MAP_GENERATOR_00598960_GHIDRA_REPORT.md`.
- Rust scan: `src/app.rs`, `src/ui/skirmish_shell/state/choose_map.rs`, `src/skirmish_scenarios.rs`, `src/app_skirmish_shell_render/preview.rs`.
