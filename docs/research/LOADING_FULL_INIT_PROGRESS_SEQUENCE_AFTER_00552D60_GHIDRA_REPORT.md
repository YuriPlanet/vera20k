# Loading Full_Init Progress Sequence After 0x00552D60 - Ghidra Report

**Address(es):** `ScenarioClass__Full_Init @ 0x00686B20`, first renderer call `0x00687588`, first progress call `0x00687594`, nested owners `Init_Theater @ 0x005349C0`, `ScenarioClass__Read_INI_Basic @ 0x00689E90`, `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70`, `ScenarioClass__Post_Map_Init @ 0x00686890`.
**Investigation Mode:** exhaustive-slice.
**Claimed Scope:** standard offline Skirmish `ScenarioClass__Full_Init` progress-callback sequence from the verified first renderer `0x00552D60` through later load phases, including nested helpers called synchronously by `Full_Init`.
**Non-Scope:** first-renderer LS art composition, `PROGBARM.SHP` pixel geometry, campaign LS text/briefing, outer wrapper milestones outside `Full_Init` except the now-bounded terminal selected-map `100` handoff, and native pixel/dwell equivalence.
**Confidence:** High for direct/nested call order, constants, the dynamic-loop formula, monotonic state advance, and the synchronous non-dialog redraw mechanism; Medium for proving that standard selected-map runtime reaches that non-dialog branch because the prior zero-argument initialization claim does not establish the lifecycle value of `ProgressClass+0x64`.
**Active in YR:** Yes for standard offline Skirmish (`g_GameMode == 5`) along successful selected-map load; conditional rows state their gates.

## Target Question

After `ScenarioClass__Full_Init` calls the first standard Skirmish loading renderer at `0x00687588` and then `FUN_0069AE90(3)` at `0x00687594`, what later visible progress callbacks occur, in what order, with what owner phase and argument source/value?

## Non-Goals

- Do not re-decode `0x00552D60` LS background, marker, or text composition.
- Do not re-decode `FUN_0069AE90`/`ProgressClass` draw geometry beyond using its monotonic visibility gate.
- Do not trace `FUN_00598960`, the rest of `Read_Scenario`, or shell Start success except for the bounded terminal selected-map `100` callback at `0x00684B2B` and where prior reports establish that standard Skirmish reaches `Full_Init`.
- Do not mutate Ghidra or edit Rust/INI/in-repo docs.

## Evidence Needed To Mark COMPLETE

- Decompile `ScenarioClass__Full_Init` and identify all `FUN_0069AE90` callsites after `0x00687594`.
- For every helper called by `Full_Init` that has a `FUN_0069AE90` xref, decompile it and place its callbacks in parent order.
- For every load-bearing row, provide assembly context showing caller address and argument source.
- Apply the verified `FUN_0069AE90` monotonic gate to distinguish visible advances from duplicate/lower invocations.
- State standard Skirmish path liveness and conditions.

## Stop Conditions

- Stop once `Full_Init` returns or reaches its failure exit; do not expand into outer startup wrappers.
- Stop when all `FUN_0069AE90` xrefs inside `Full_Init` and its progress-calling direct helpers are placed in the ledger.
- Stop before progress-bar drawing details; use prior `ProgressClass` reports for repaint semantics.

## Progress Visibility Rule Used

`FUN_0069AE90` first halves requested values for random-map loads (`ScenarioClass+0x34BD != 0`), then reads current lane 0 percent through `FUN_00643E90(0)`, multiplies by `100.0`, and calls `FUN_00643C50(0, requested, -1, -1)` only when the current percent is strictly less than the requested milestone. Equal or lower milestones do not advance state. On an advancing value, `0x00643C50` synchronously sends `WM_PAINT` when `ProgressClass+0x64` is non-null; otherwise it calls `0x00643AE0`, whose tail calls `0x004F4780` with `CL=1`, `EDX=DAT_0088730C`, and a null rectangle. `0x004F4780` brackets a surface copy with display-chain callbacks. This proves a synchronous display blit mechanism, not an explicit Present/Flip call. Whether standard selected-map runtime selects the null-`+0x64` branch remains **UNVERIFIED** because `0x00642A60(..., 0)` does not write `+0x64`. Evidence: `decompile_function 0x0069AE90`, `decompile_function 0x00643C50`, `decompile_function 0x00643AE0`, `disassemble_bytes 0x00643C2F..0x00643C50`, `decompile_function 0x004F4780`, `decompile_function 0x00642A60`.

For ordinary selected-map Skirmish, the random-map halving branch is inactive.

## Ordered Milestone Ledger

Rows marked `visible: yes` are progress callbacks that can repaint after applying the monotonic gate. Rows marked `visible: no` are invoked but suppressed by the native callback because the current milestone is already higher or equal.

| Order | Visible? | Owner phase | Caller address | Argument source/value | Active in standard Skirmish? | Evidence |
|---:|---|---|---|---|---|---|
| 1 | yes | first renderer handoff | `0x00687594` in `Full_Init` | immediate `PUSH 0x3` / decimal `3` after `CALL 0x00552D60` | Yes | asm `0x00687588..0x00687594`; first-renderer report |
| 2 | yes | theater init entry | `0x00534A63` in `Init_Theater`, parent call `0x0068765B` | immediate `PUSH 0x8` / `8` | Yes | decompile `0x005349C0`; asm `0x00534A5B..0x00534A63`; parent asm `0x0068763E..0x00687667` |
| 3 | no | theater archive reload | `0x00534B65` in `Init_Theater` | immediate `PUSH 0x6` / `6` | Conditional on theater-cache mismatch; suppressed after visible `8` | decompile `0x005349C0`; asm `0x00534B5E..0x00534B65`; `0x0069AE90` |
| 4 | yes | theater archive reload | `0x00534BE9` in `Init_Theater` | immediate `PUSH 0xC` / `12` | Conditional on theater-cache mismatch | decompile `0x005349C0`; asm `0x00534BE2..0x00534BE9` |
| 5 | yes, on increases | theater palette/remap loop | `0x00534D9A` in `Init_Theater` | register `EDI`; computed `min(i / (DAT_00B054E0 / 13) + 0x0C, 0x19)` / decimal `13..25`, called only when local previous value changes | Conditional on theater-cache mismatch and `DAT_00B054E0 > 0` | decompile loop in `0x005349C0`; asm `0x00534D84..0x00534D9A` |
| 6 | conditional | theater finish | `0x00534DC5` in `Init_Theater` | immediate `PUSH 0x19` / `25` | Visible only if loop did not already advance to `25`; otherwise duplicate-suppressed | decompile `0x005349C0`; asm `0x00534DB4..0x00534DC5` |
| 7 | yes | after `Init_Theater` returns | `0x00687667` in `Full_Init` | immediate `PUSH 0x1E` / `30` | Yes | decompile `0x00686B20`; asm `0x0068765B..0x00687667` |
| 8 | yes | command bar rules load | `0x0068769B` in `Full_Init` | immediate `PUSH 0x1F` / `31` after `RulesClass__ReadCommandBar` | Yes | asm `0x00687683..0x0068769B` |
| 9 | yes | rules CD/file setup | `0x006876B8` in `Full_Init` | immediate `PUSH 0x23` / `35` after `CDFileClass__Constructor(DAT_00887048)` | Yes | asm `0x006876A0..0x006876B8` |
| 10 | yes | variable names + rules process | `0x0068775B` in `Full_Init` | immediate `PUSH 0x2D` / `45` after `RulesClass__Process(param_1)` | Yes | asm `0x0068773F..0x0068775B` |
| 11 | yes | side mix init | `0x00687847` in `Full_Init` | immediate `PUSH 0x32` / `50` after successful `InitSideMixFiles` | Yes when side mix init succeeds; failure exits | asm `0x00687833..0x00687847` |
| 12 | yes | `[Basic]` / lighting read | `0x0068ACA0` in `ScenarioClass__Read_INI_Basic` | immediate `PUSH 0x37` / `55` near end of lighting-key pass | Yes if `Read_INI_Basic` succeeds | decompile `0x00689E90`; asm `0x0068AC93..0x0068ACA0` |
| 13 | yes | player/house setup inside `Read_INI_Basic` | `0x0068AD34` in `ScenarioClass__Read_INI_Basic` | immediate `PUSH 0x3A` / `58` after setting `g_PlayerPtr` flags and `+0x56F4` | Yes if `Read_INI_Basic` succeeds | decompile `0x00689E90`; asm `0x0068AD04..0x0068AD34` |
| 14 | yes | end of `Read_INI_Basic` | `0x0068AD53` in `ScenarioClass__Read_INI_Basic` | immediate `PUSH 0x3C` / `60`, after optional map-editor call | Yes if `Read_INI_Basic` succeeds | decompile `0x00689E90`; asm `0x0068AD3E..0x0068AD53` |
| 15 | no | direct post-`Read_INI_Basic` callback | `0x00687863` in `Full_Init` | immediate `PUSH 0x3A` / `58` after `Read_INI_Basic` returns true | Invoked, but non-visible because `Read_INI_Basic` already advanced to `60` | asm `0x0068784C..0x00687863`; monotonic gate `0x0069AE90` |
| 16 | no | pre-map-section type/script/team setup | `0x006879F4` in `Full_Init` | immediate `PUSH 0x3C` / `60` after constructors/recompute | Invoked, but duplicate-suppressed because current is already `60` | asm `0x006879D5..0x006879F4`; monotonic gate `0x0069AE90` |
| 17 | yes | map/theater section start | `0x004AD011` in `Read_Map_Section_And_IsoMapPacks`, parent call `0x006879FF` | immediate `PUSH 0x3F` / `63` after setting scenario theater and map vtable `+0x18` | Yes | decompile `0x004ACE70`; asm `0x004ACFF6..0x004AD011`; parent asm `0x006879F9..0x00687A04` |
| 18 | yes | theater tileset / surface setup | `0x004AD0AF` in `Read_Map_Section_And_IsoMapPacks` | immediate `PUSH 0x41` / `65` after tile-set load path and surface constructors | Yes | decompile `0x004ACE70`; asm `0x004AD087..0x004AD0AF` |
| 19 | yes | cell tags pass | `0x004AD339` in `Read_Map_Section_And_IsoMapPacks` | immediate `PUSH 0x43` / `67` after CellTags loop | Yes | decompile `0x004ACE70`; asm `0x004AD31D..0x004AD339` |
| 20 | yes | IsoMapPack decode | `0x004AD716` in `Read_Map_Section_And_IsoMapPacks` | immediate `PUSH 0x44` / `68` after IsoMapPack 1..5 decode helpers | Yes | decompile `0x004ACE70`; asm `0x004AD6F8..0x004AD716` |
| 21 | yes | post-IsoMapPack helper | `0x004AD74F` in `Read_Map_Section_And_IsoMapPacks` | immediate `PUSH 0x45` / `69` after `FUN_00546DA0` | Yes | decompile `0x004ACE70`; asm `0x004AD743..0x004AD74F` |
| 22 | yes | after map/overlay prelude | `0x00687A28` in `Full_Init` | immediate `PUSH 0x46` / `70` after `FUN_007283C0`, `FUN_00465CC0`, `FUN_004F42F0(2)` | Yes | asm `0x00687A09..0x00687A28` |
| 23 | yes | terrain/tiberium init | `0x00687A96` in `Full_Init` | immediate `PUSH 0x48` / `72` after terrain read and tiberium growth/spread queue init | Yes | asm `0x00687A74..0x00687A96` |
| 24 | yes | units section | `0x00687AB8` in `Full_Init` | immediate `PUSH 0x4A` / `74` after radar rebuild and `ScenarioClass__Read_Units_Section` | Yes | asm `0x00687A9B..0x00687AB8` |
| 25 | yes | infantry/unknown object pass | `0x00687ADC` in `Full_Init` | immediate `PUSH 0x4C` / `76` after `FUN_0041B110` and `FUN_0051FB00` | Yes | asm `0x00687ABD..0x00687ADC` |
| 26 | yes | buildings read | `0x00687AFB` in `Full_Init` | immediate `PUSH 0x4E` / `78` after `BuildingClass__ReadFromINI`, with `DAT_00829AE4` set `0` before and `1` after | Yes | asm `0x00687AE1..0x00687AFB` |
| 27 | yes | optional random-map rules refresh boundary | `0x00687B82` in `Full_Init` | immediate `PUSH 0x52` / `82`; prior `TMCJ4F.INI` branch only when `g_GameMode == 5 && DAT_00A8ED91 != 0` | Milestone yes; extra TMCJ4F work conditional | decompile `0x00686B20`; asm `0x00687B18..0x00687B82` |
| 28 | yes | cell attributes init | `0x00687BA3` in `Full_Init` | immediate `PUSH 0x56` / `86` after `MapClass__InitCellAttributes(0)` stores `_DAT_0087F91C` | Yes | asm `0x00687B8C..0x00687BA3` |
| 29 | yes | beacon art init | `0x00687BBE` in `Full_Init` | immediate `PUSH 0x5A` / `90` after `Init_BeaconArt` | Yes | asm `0x00687BA8..0x00687BBE` |
| 30 | yes | post-map-init inner callback | `0x00686952` in `ScenarioClass__Post_Map_Init` | immediate `PUSH 0x5D` / `93` after random-unit/session-start handling | Conditional parent call; normal Skirmish path uses it | decompile `0x00686890`; asm `0x00686931..0x00686952`; parent asm `0x00687BC8..0x00687C07` |
| 31 | yes | after post-map-init and tactical cleanup | `0x00687C07` in `Full_Init` | immediate `PUSH 0x60` / `96` after `FUN_006CF230(&DAT_00B0C110)` | Conditional with parent post-map-init block, normal Skirmish path | asm `0x00687BF1..0x00687C07` |
| 32 | yes | final pre-object/render refresh | `0x00687C4C` in `Full_Init` | immediate `PUSH 0x62` / `98` after temporary map-editor-mode toggle and `FUN_00452D40` | Yes if load reaches final success path | asm `0x00687C2B..0x00687C4C` |

The successful selected-map load then reaches an outer `ScenarioClass__Read_Scenario` callback at `0x00684B2B`. Assembly computes raw `100` when `ScenarioClass+0x34BD == 0` and raw `200` otherwise; `FUN_0069AE90` halves the random-map value, so both selected and random-map paths target effective `100`. This terminal callback is outside the 32-row `Full_Init` ledger and does not change its ordering. Evidence: `get_function_by_address 0x00684B2B`; `disassemble_bytes 0x00684AF0..0x00684B50`; `decompile_function 0x00684620`; `decompile_function 0x0069AE90`.

## Core Logic Notes

- `Full_Init` direct xrefs after `0x00687594` are decimal `30,31,35,45,50,58,60,70,72,74,76,78,82,86,90,96,98`.
- The visible sequence is not just those direct constants. `Init_Theater`, `ScenarioClass__Read_INI_Basic`, `Read_Map_Section_And_IsoMapPacks`, and `ScenarioClass__Post_Map_Init` add nested callbacks on the same parent load path.
- The direct `Full_Init` `58` at `0x00687863` is not visible in the successful standard path because `ScenarioClass__Read_INI_Basic` already called `60` before returning.
- The direct `Full_Init` `60` at `0x006879F4` is also not visible because it duplicates the helper's final `60`.
- `Init_Theater` raw call order includes `8` then conditional `6`; the global callback suppresses `6` after `8`.
- `Read_Map_Section_And_IsoMapPacks` contributes the dense map-section sequence `63,65,67,68,69` before control returns to `Full_Init` and the direct `70`.

## INI Keys

No new INI key controls the progress milestone values themselves. Relevant reads only determine load phases:

| Key / source | Role in this slice | Evidence |
|---|---|---|
| `[Basic] Theater` | read by `Full_Init`, passed to `Init_Theater`; theater-change cache controls whether the conditional archive/palette milestones run | `Full_Init 0x0068763E..0x0068765B`; `Init_Theater 0x005349C0` |
| `[Basic]` / `[Header]` / `[Lighting]` | read inside `ScenarioClass__Read_INI_Basic`; progress rows `55/58/60` bracket late portions of that parser | `0x00689E90`; asm `0x0068ACA0`, `0x0068AD34`, `0x0068AD53` |
| `TMCJ4F.INI` | optional Skirmish random-map rules refresh before direct milestone `82` | `Full_Init 0x00687B18..0x00687B82` |

## Integration Points

`ScenarioClass__Read_Scenario` configures the standard non-campaign progress surface and first-renderer state, then startup reaches `ScenarioClass__Full_Init`. Inside `Full_Init`, standard Skirmish takes the non-campaign setup branch, calls the first renderer at `0x00687588`, applies milestone `3`, constructs tactical/theater state, and then advances progress through direct and nested load phases until final success.

`ScenarioClass__Read_Scenario` calls `0x00642A60` with final argument zero at `0x00684706`, but that branch does **not** clear `ProgressClass+0x64`: it writes `+0x50`, `+0x7C`, and other setup state, then returns without touching `+0x64`. The null-`+0x64` lifecycle premise is therefore **UNVERIFIED**. Mechanically, any advancing callback with `+0x64 == 0` redraws synchronously through `FUN_00643C50 -> FUN_00643AE0 -> FUN_004F4780`; the latter performs a surface-copy handoff with display-chain bracketing, not an explicit Present/Flip operation. Evidence: `decompile_function 0x00684620`; `disassemble_bytes 0x006846E0..0x00684715`; `decompile_function 0x00642A60`; `decompile_function 0x00643C50`; `disassemble_bytes 0x00643C2F..0x00643C50`; `decompile_function 0x004F4780`.

## Current Rust Implementation Status

| Surface | Current behavior | Delta |
|---|---|---|
| `src/app_loading.rs:780..790` | Native selected-map loading advances to effective `3` before the first displayed native frame. | Source-complete for the verified first displayed state; exact native 3% pixel width/dwell remains unchecked. |
| `src/app_loading.rs:308..315`, `src/app_loading.rs:519..585`, `src/app_loading.rs:677..704` / `src/app_init.rs:497..529` | Captures two runtime color schemes per parsed `[Colors]` entry, applies the first/same/changed-theater cache gate, and reconstructs the verified changed-only dynamic `13..25` sequence from the pre-load count. | Source-complete for the verified mechanism; Rust emits the reconstructed sequence after its monolithic theater load, not during each native per-scheme rebuild. |
| `src/app_init.rs:428..488`, `src/app_init.rs:638..740`, `src/app_init.rs:797..800`, `src/app_init.rs:1253..1261` | Emits the verified selected-map phase ledger, including `55`, `90`, and `93`, at current Rust load boundaries. | Source-complete for milestone routing; phase work is Rust-native and does not claim exact native dwell timing. |
| `src/app_loading.rs:1017..1150` | Every advancing native callback re-renders and presents the current loading frame synchronously through wgpu. | Player-visible synchronous handoff is implemented; this is not a claim that wgpu `output.present()` is mechanism-identical to native `WM_PAINT`/surface-copy plumbing. |
| `src/app_loading.rs:600..613` | Successful selected-map completion advances and synchronously presents terminal raw/effective `100` before `LoadingPump::Finished`. | Source-complete for the verified terminal state; no native pixel/dwell/Present parity claim. |

## Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `ScenarioClass__Full_Init` direct progress calls after `3` | verified | decompile `0x00686B20`; xrefs `0x00687594..0x00687C4C`; assembly contexts | none for constants/order |
| `Init_Theater` nested progress calls | verified | decompile `0x005349C0`; xrefs `0x00534A63`, `0x00534B65`, `0x00534BE9`, `0x00534D9A`, `0x00534DC5`; bounded count follow-up [LOADING_THEATER_DYNAMIC_13_25_PROGRESS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_THEATER_DYNAMIC_13_25_PROGRESS_GHIDRA_REPORT.md) | native pixel/dwell capture only if later exactification requires it |
| `ScenarioClass__Read_INI_Basic` nested progress calls | verified | decompile `0x00689E90`; xrefs `0x0068ACA0`, `0x0068AD34`, `0x0068AD53` | none for constants/order |
| `Read_Map_Section_And_IsoMapPacks` nested progress calls | verified | decompile `0x004ACE70`; xrefs `0x004AD011`, `0x004AD0AF`, `0x004AD339`, `0x004AD716`, `0x004AD74F` | none for constants/order |
| `ScenarioClass__Post_Map_Init` nested progress call | verified | decompile `0x00686890`; xref `0x00686952`; parent condition in `Full_Init` | exact local skip flag provenance, not needed for normal Skirmish ledger |
| monotonic state advance / duplicate suppression | verified | decompile `0x0069AE90`; prior ProgressClass report | none |
| synchronous non-dialog redraw mechanism | verified | decompile `0x00643C50`, `0x00643AE0`, `0x004F4780`; asm `0x00643C38..0x00643C42` | standard selected-map `ProgressClass+0x64` lifecycle remains UNVERIFIED |
| outer terminal callback | verified | `Read_Scenario` asm `0x00684B0F..0x00684B2B`; decompile `0x00684620`, `0x0069AE90` | broader outer-wrapper callback inventory remains deferred |

## Open Questions - Final State

- `[RESOLVED] OQ-01 - Does `Full_Init` call any progress callback after first renderer milestone `3`? -> Yes; direct calls plus nested helper calls form the sequence in Section 6.` (evidence: `get_function_xrefs 0x0069AE90`; decompile `0x00686B20`)
- `[RESOLVED] OQ-02 - Are helper-owned callbacks inside `Full_Init` part of the player-visible sequence? -> Yes, because helpers are called synchronously from `Full_Init` between direct milestones and invoke the same direct-draw progress callback.` (evidence: parent callsites `0x0068765B`, `0x00687853`, `0x006879FF`, `0x00687BEC`)
- `[RESOLVED] OQ-03 - Are the direct `58` and `60` `Full_Init` calls visible? -> No on the successful standard path; `Read_INI_Basic` has already advanced to `60`.` (evidence: `0x0068AD53`; `0x00687863`; `0x006879F4`; `0x0069AE90`)
- `[RESOLVED] OQ-04 - Does `Init_Theater` include a lower milestone after a higher one? -> Yes, it calls `8` then conditionally `6`; `6` is suppressed by `FUN_0069AE90`.` (evidence: `0x00534A63`, `0x00534B65`, `0x0069AE90`)
- `[RESOLVED] OQ-05 - What is the dynamic theater-loop argument source? -> Register `EDI`, computed from loop index divided by `DAT_00B054E0 / 13`, plus `12`, capped to `25`, and called only when changed from the previous local value.` (evidence: decompile `0x005349C0`; asm `0x00534D84..0x00534D9A`)
- `[RESOLVED] OQ-06 - Does normal selected-map Skirmish use random-map halving? -> No; `FUN_0069AE90` halves only when `ScenarioClass+0x34BD != 0`.` (evidence: decompile `0x0069AE90`; random map branch not standard selected-map path)
- `[RESOLVED] OQ-07 - Is `Post_Map_Init` inside this sequence? -> Yes conditionally before direct `96`; it emits `93` then returns to `Full_Init`.` (evidence: parent asm `0x00687BD8..0x00687C07`; child asm `0x0068694B..0x00686952`)
- `[RESOLVED] OQ-08 - What dynamic `Init_Theater` values occur for the stock pre-load scheme registry? -> Stock `rulesmd.ini` has 21 `[Colors]` keys; `0x0066D3A0` constructs two runtime scheme entries per key, so `N=42`, quotient `3`, and the `0x005349C0` changed-only loop emits `13..25` inclusive. See `[LOADING_THEATER_DYNAMIC_13_25_PROGRESS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_THEATER_DYNAMIC_13_25_PROGRESS_GHIDRA_REPORT.md)`.` (evidence: `decompile_function 0x0066D3A0`; read-only count of stock `rulesmd.ini [Colors]`; decompile/assembly of `0x005349C0`)
- `[PARTIALLY RESOLVED] OQ-09 - Do outer wrapper callbacks after `Full_Init` add a terminal visible milestone before first playable frame? -> Yes: successful `Read_Scenario` computes selected-map raw `100` (random-map raw `200`, halved to `100`) and calls `FUN_0069AE90` at `0x00684B2B`. A broader wrapper inventory remains outside this report.` (evidence: `get_function_by_address 0x00684B2B`; `disassemble_bytes 0x00684AF0..0x00684B50`; `decompile_function 0x00684620`; `decompile_function 0x0069AE90`)

## Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Full native loading progress after `3` uses a fixed ordered ledger with nested helper milestones, not a smooth or arbitrary bar. | Section 6; `0x00687594`, `0x00534A63`, `0x0068ACA0`, `0x004AD011`, `0x00686952` | implemented in current source | `src/app_loading.rs`; `src/app_init.rs` | retain only verified milestones at real Rust phase boundaries, including helper-owned values | `loading_progress_standard_skirmish_selected_map_emits_verified_milestone_ledger` | do not claim exact native dwell timing or interpolate |
| Some invoked milestones intentionally do not repaint because the native callback is monotonic. | `0x0069AE90`; `0x00534B65`; `0x00687863`; `0x006879F4` | implemented | `LoadingProgressState::advance_progress`; rendering sink | preserve strict-advance gating for lower/equal values | `loading_progress_suppresses_nonadvancing_raw_native_calls`; `recording_sink_suppresses_nonadvancing_and_duplicate_milestones` | do not count every raw callsite as a redraw |
| Dynamic theater rebuild values use the pre-load scheme count and a changed-only `13..25` sequence on cache mismatch. | `0x005349C0`; [LOADING_THEATER_DYNAMIC_13_25_PROGRESS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_THEATER_DYNAMIC_13_25_PROGRESS_GHIDRA_REPORT.md) | implemented | `NativeLoadingScreenState::resolve_player_colors`; `theater_cache_mismatch`; `theater_ramp_changed_values`; `load_map_from_initial` | retain count-derived sequence and cache gate | stock `42`, non-multiple `38`, invalid-small, and first/same/changed tests in `app_loading.rs` | Rust emits after monolithic theater load; do not claim native per-item timing |
| Successful selected-map completion emits terminal raw/effective `100` outside `Full_Init`. | `Read_Scenario` `0x00684B0F..0x00684B2B`; `0x0069AE90` | implemented | `pump_loading_after_present`; `advance_and_present_native_progress` | retain synchronous `100` presentation before `Finished` | selected-map cadence terminal test and full-ledger recording test | do not call the wgpu present mechanism-identical to native Present/Flip |

Proposed tests:

- `loading_full_init_milestone_ledger_excludes_suppressed_lower_and_duplicate_values`
- `loading_full_init_emits_nested_read_ini_and_map_section_milestones`
- `loading_full_init_progress_sequence_reaches_98_before_ingame`

## Negative Facts / Do Not Do

- Do not wire only direct `Full_Init` calls; helper-owned callbacks are part of the visible sequence.
- Do not treat `Init_Theater`'s raw `6` call as a visible backwards movement after `8`; native `FUN_0069AE90` suppresses it.
- Do not redraw the direct `Full_Init` `58` or duplicate direct `60` after `Read_INI_Basic` has already advanced to `60`.
- Do not smooth or animate between milestone values.
- Do not apply random-map halving to ordinary selected-map Skirmish loads.
- Do not describe native loading callbacks as explicit Present/Flip calls; the verified native mechanism is state advance plus a conditional synchronous `WM_PAINT` or non-dialog surface-copy redraw.
- Do not treat `0x00642A60(..., 0)` as proof that `ProgressClass+0x64` is null.

## Remaining Uncertainty

- The lifecycle value of native `ProgressClass+0x64` on the standard selected-map path remains **UNVERIFIED**; the zero-argument setup call does not clear it.
- Native-vs-Rust pixel output, per-milestone dwell time, and presentation-mechanism identity remain **UNCHECKED**.
- The outer terminal `100` callback is verified, but a complete inventory of every other outer-wrapper callback remains outside this report.

## Stale Docs / Follow-up Docs

Suggested replacement for `docs/plans/2026-05-23-standard-offline-skirmish-loading-plan.md` rows that say later milestones are not yet known:

> The `Full_Init` milestone ledger after first renderer milestone `3` is verified in `LOADING_FULL_INIT_PROGRESS_SEQUENCE_AFTER_00552D60_GHIDRA_REPORT.md`. App-loop loading may use the Section 6 ordered ledger for `Full_Init` phase boundaries, preserve monotonic suppression for raw lower/duplicate calls, and present the separately verified outer selected-map terminal `100` before game entry. Other outer-wrapper callbacks remain separate scope.

Suggested replacement for `docs/implementation-queue/2026-05-23-implementation-queue-loading-screen.md` readiness language:

> The `Full_Init` later-milestone blocker and outer selected-map terminal `100` boundary are closed for the scoped standard Skirmish loading path. Remaining exactification gates include native `ProgressClass+0x64` lifecycle, pixel/dwell capture, and the separate `mmpb.shp`/post-marker text blockers.

## Sources

- Ghidra decompile: `0x00686B20`, `0x005349C0`, `0x00689E90`, `0x004ACE70`, `0x00686890`, `0x0069AE90`, `0x00643C50`.
- 2026-07-27 correction recheck: `get_function_by_address 0x00684B2B`; `disassemble_bytes 0x00684AF0..0x00684B50`; `decompile_function 0x00684620`; `disassemble_bytes 0x006846E0..0x00684715`; `decompile_function 0x00642A60`; `decompile_function 0x00643C50`; `decompile_function 0x00643AE0`; `disassemble_bytes 0x00643C2F..0x00643C50`; `decompile_function 0x004F4780`; `decompile_function 0x0066D3A0`; read-only stock `rulesmd.ini [Colors]` count; `get_assembly_context` for `0x00687594`, `0x00534D9A`, `0x0068ACA0`, `0x004AD011`, `0x00687BBE`, `0x00686952`, and `0x00687C4C` (all Ghidra calls explicitly against program `gamemd.exe`).
- Ghidra xrefs: `get_function_xrefs 0x0069AE90`.
- Ghidra assembly context: `0x00687594`, `0x00687667`, `0x0068769B`, `0x006876B8`, `0x0068775B`, `0x00687847`, `0x00687863`, `0x006879F4`, `0x00687A28`, `0x00687A96`, `0x00687AB8`, `0x00687ADC`, `0x00687AFB`, `0x00687B82`, `0x00687BA3`, `0x00687BBE`, `0x00687C07`, `0x00687C4C`, `0x00534A63`, `0x00534B65`, `0x00534BE9`, `0x00534D9A`, `0x00534DC5`, `0x0068ACA0`, `0x0068AD34`, `0x0068AD53`, `0x004AD011`, `0x004AD0AF`, `0x004AD339`, `0x004AD716`, `0x004AD74F`, `0x00686952`.
- Prior reports: `LOADING_FIRST_RENDERER_00552D60_GHIDRA_REPORT.md`, `LOADING_FUN_0069AE90_SKIRMISH_CALLERS_AFTER_FIRST_RENDERER_GHIDRA_REPORT.md`, `PROGRESSCLASS_REPAINT_CADENCE_HWND_GHIDRA_REPORT.md`, `PROGBARM_PROGRESSCLASS_DRAW_GEOMETRY_GHIDRA_REPORT.md`, [LSLOADMESSAGE_SKIRMISH_LOADING_TEXT_SPLIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LSLOADMESSAGE_SKIRMISH_LOADING_TEXT_SPLIT_GHIDRA_REPORT.md).
- Rust scan (2026-07-27): `rg` plus direct reads of `src/app_loading.rs` and `src/app_init.rs`; no native pixel/dwell/presentation-equivalence claim.

## 2026-07-27 Audit Correction Note

- **Terminal boundary corrected:** selected-map terminal `100` is the outer `ScenarioClass__Read_Scenario` call at `0x00684B2B`, while random-map raw `200` is halved to effective `100` by `0x0069AE90`. Root cause: **SCOPE_BOUNDARY_STALE**. Evidence: `get_function_by_address 0x00684B2B`; `disassemble_bytes 0x00684AF0..0x00684B50`; `decompile_function 0x00684620`; `decompile_function 0x0069AE90`.
- **Presentation wording corrected:** callbacks prove monotonic state advance and a conditional synchronous `WM_PAINT`/surface-copy redraw path, not an explicit Present/Flip call. Root cause: **INFERENCE_HARDENED**. Evidence: `decompile_function 0x00643C50`; `decompile_function 0x00643AE0`; `disassemble_bytes 0x00643C2F..0x00643C50`; `decompile_function 0x004F4780`.
- **`+0x64` initialization corrected:** `ScenarioClass__Read_Scenario` passes zero to `0x00642A60`, but that branch does not write `ProgressClass+0x64`; its standard-path lifecycle value remains **UNVERIFIED**. Root cause: **PARAMETER_SEMANTICS_MISREAD**. Evidence: `decompile_function 0x00684620`; `disassemble_bytes 0x006846E0..0x00684715`; `decompile_function 0x00642A60`.
- **Rust status refreshed:** current source routes visible `3`, count-derived/cache-gated `13..25`, `55`, `90`, `93`, and synchronously presented terminal `100`. Root cause: **RUST_STATUS_STALE**. Evidence: 2026-07-27 `rg` and direct reads of `src/app_loading.rs` and `src/app_init.rs`. This is source completeness only; native pixel output, dwell timing, and Present/Flip equivalence are not claimed.

**Status:** COMPLETE for the scoped `Full_Init` milestone ledger after `0x00552D60` / `FUN_0069AE90(3)`, with the bounded outer terminal `100` handoff included. Audit verdict remains **YELLOW** because native `ProgressClass+0x64` lifecycle, pixel output, dwell timing, and presentation-mechanism equivalence remain unverified/unchecked.
