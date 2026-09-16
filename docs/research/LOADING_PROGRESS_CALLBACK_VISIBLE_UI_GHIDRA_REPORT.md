# Loading Progress Callback Visible UI - Ghidra Research Report

**Address(es):** `0x0069AE90`, `0x00643C50`, `0x00643AE0`, `0x00686B20`, `0x00684620`, `0x005349C0`  
**Investigation Mode:** coverage-map  
**Claimed Scope:** loading progress/update callback path during standard scenario load from Start Game through `ScenarioClass__Read_Scenario` / `ScenarioClass__Full_Init`, with narrow verification of visible UI effects and progress values.  
**Non-Scope:** full loading-screen background composition, exact PUDLGBG SHP load/free lifecycle, exact dialog palette startup path, and complete campaign-only `LSLoadMessage`/briefing layout.  
**Confidence:** High for the callback, progress value chain, visible repaint/direct-draw behavior, and Rust mismatch; Medium for exact child-control repaint ownership because Ghidra does not expose the Windows child-paint ordering as a runtime trace.  
**Active in YR:** Yes. Standard offline Skirmish uses `g_GameMode == 5`, enters `ScenarioClass__Read_Scenario @ 0x00684620`, constructs the load progress manager, and then calls `ScenarioClass__Full_Init @ 0x00686B20`.

## 1. Overview

`FUN_0069AE90` is the scenario-load progress callback. It is not a no-op telemetry hook: it converts milestone integers into a visible progress value and repaints/draws the loading progress surface when the value changes.

The standard YR loading path creates a progress manager before reading the scenario, selects `PROGBARM.SHP` for non-campaign loads, draws/positions the progress UI, then drives it with milestone values throughout `ScenarioClass__Full_Init`. The current Rust path presents one egui loading frame and then blocks in `transition_to_in_game`, so it misses native's visible progress/cadence surface.

## 2. Target Question

Does `gamemd.exe` expose visible loading progress/status updates while starting a game, how are parser/load callbacks wired to the loading screen, what numeric ranges/cadence are used, and is mode-2 painting invalidated/repainted during synchronous load?

## 3. Non-Goals

- Do not decode every asset parser.
- Do not prove every pixel of the loading-screen background.
- Do not audit campaign-only loading briefing text beyond recognizing that the same progress manager has campaign branches.
- Do not write Rust.

## 4. Evidence Needed To Mark Complete

- Identify the progress callback owner and visible-effect callees.
- Tie asset parser milestone calls to the scenario-load path used by YR Skirmish.
- Enumerate the material milestone values in the load path.
- Determine whether progress changes schedule/asynchronously invalidate or synchronously repaint/draw.
- Compare against current Rust loading surface.

## 5. Stop Conditions

- Stop after all progress callback xrefs in `ScenarioClass__Read_Scenario`, `ScenarioClass__Full_Init`, `Init_Theater`, and `Read_Map_Section_And_IsoMapPacks` are accounted for.
- Stop before full PUDLGBG/background composition; slot 1 owns that.
- Stop before full SHP/palette load lifecycle; slots 2 and 3 own those.

## 6. Core Verified Findings

| Finding | Evidence | Confidence | Active in YR? |
|---|---|---:|---|
| `ScenarioClass__Read_Scenario` constructs and enables the loading progress UI before scenario parsing/full init. | `0x00684620`: calls `FUN_00642A60` on `0x00AC4F58` with max `100.0`, calls `LoadProgressMgr__Constructor`, `FUN_00643E80(local_240)`, side setup, `FUN_00642C20`, `FUN_00642C80`, `FUN_00642DF0`, then reads scenario/full init. | High | Yes |
| Non-campaign/YR Skirmish uses `PROGBARM.SHP` as the progress-bar shape, not `SPLDBR.SHP`. | `0x006847E1..0x00684800`: `g_GameMode == 0` selects `SPLDBR.SHP`; else selects `PROGBARM.SHP`, then calls `FUN_00642C20` on ProgressClass. | High | Yes for Skirmish |
| Progress value changes cause visible drawing/repaint, not just stored state. | `0x00643C50`: if stored progress changed, it sends message `0x0F` (`WM_PAINT`) to ProgressClass `+0x64` HWND when present; otherwise calls `FUN_00643AE0`. `0x00643AE0` draws progress rows with `FUN_00643720`/`FUN_00643400` and ends with `FUN_004F4780(0)`. | High | Yes |
| The update is monotonic percent-like progress, capped by ProgressClass max. | `0x00643E90` returns current / max; `0x0069AE90` compares that fraction times `100` to the requested milestone; `0x00643C50` writes `max * 0.01 * milestone` and caps at max. | High | Yes |
| Random-map/scenario-generation flag halves intermediate milestone inputs, but callers pass doubled final values so completion still reaches 100. | `0x0069AE90` divides `param_2` by 2 when `ScenarioClass+0x34BD` is nonzero; `ScenarioClass__Read_Scenario` final call computes `(-random_flag & 100) + 100`, giving 100 normally and 200 before halving for random maps. | High | Conditional: random maps only |
| `Init_Theater` progress callbacks are part of a larger visible load progress sequence. | `0x005349C0`: progress milestones `8`, `6`, `12`, loop-derived `13..25`, and final `25`; all call `FUN_0069AE90`, which performs visible progress update. | High | Yes when theater changes/loads |
| No asynchronous `InvalidateRect` was found in the progress callback. Repaint is synchronous: `SendMessageA(hwnd, 0x0F, 0, 0)` or direct draw. | `0x00643C50`; no `InvalidateRect` call in callback body. | High | Yes |
| Network service calls are interleaved with loader milestones. | `0x005349C0`, `0x00686B20`, and `0x00684620` call `Network_ServiceLoop` between loader chunks/progress updates. | High | Yes; service is present even offline, but network wait loops are not active for `g_GameMode == 5` |

## 7. Numeric Progress Ranges / Cadence

Milestones are integer inputs to `FUN_0069AE90`; they map to percent-like values against ProgressClass max `100.0`. The callback only redraws when the stored progress changes.

| Stage | Values | Evidence | Notes |
|---|---|---|---|
| Scenario progress setup | max `100.0`, player-count lanes | `0x00684700..0x00684706` calls `FUN_00642A60` on `0x00AC4F58` | One lane in campaign/skirmish; multiplayer can use player count. |
| Initial scenario/full-init bridge | `3` | `0x00687592..0x00687594` | After load progress manager construction in `Full_Init`. |
| Theater/MIX/palette load | `8`, `6`, `12`, `13..25`, final `25` | `0x00534A63`, `0x00534B65`, `0x00534BE9`, `0x00534D9A`, `0x00534DC5` | Loop only calls when calculated step changes; first loop-visible step after `12` is `13`. |
| Post-theater core load | `30`, `31`, `35`, `45` | `0x00687667`, `0x0068769B`, `0x006876B8`, `0x0068775B` | Command bar, scenario file, rules/process stages. |
| Side/basic setup | `50`, `58` | `0x00687847`, `0x00687863` | Side mix/palette and basic scenario read. |
| Map pack / overlay / terrain | `60`, `70`, `72` | `0x006879F4`, `0x00687A28`, `0x00687A96` | `Read_Map_Section_And_IsoMapPacks`, overlay packs, radar/terrain/unit start. |
| Map-section internals | `63`, `65`, `67`, `68`, `69` | `0x004AD011`, `0x004AD0AF`, `0x004AD339`, `0x004AD716`, `0x004AD74F` | These are nested callbacks inside map/pack reading. |
| Units/buildings/post-map | `74`, `76`, `78`, `82`, `86`, `90`, `96`, `98` | `0x00687AB8`, `0x00687ADC`, `0x00687AFB`, `0x00687B82`, `0x00687BA3`, `0x00687BBE`, `0x00687C07`, `0x00687C4C` | Final `Full_Init` milestones before return. |
| Scenario read finalization | `100` | `0x00684B2B` and `0x00684620` final section | Normal maps pass `100`; random maps pass `200` then `FUN_0069AE90` halves to `100`. |
| Multiplayer wait only | `99`, repeated `100` wait pulses | `0x00684AE8`, `0x00684370` | Not active for standard offline Skirmish because `FUN_00684370` early-returns for `g_GameMode == 5`. |

## 8. Integration Points

| Point | What happens | Evidence | Active in YR? |
|---|---|---|---|
| `ScenarioClass__Read_Scenario @ 0x00684620` | Sets `ScenarioClass+0x3598 = 1`, configures ProgressClass, constructs loading manager, selects side/progress SHP, then reads scenario/full init. | Decompiled `0x00684620` | Yes |
| `ScenarioClass__Full_Init @ 0x00686B20` | Runs the main loader and calls `FUN_0069AE90` at coarse milestones. | Decompiled `0x00686B20`; assembly context for xrefs | Yes |
| `Init_Theater @ 0x005349C0` | Calls progress callback around theater MIX/palette setup and within 13-level lighting loop. | Decompiled `0x005349C0` | Yes |
| `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70` | Emits nested progress values `63..69` around map pack work. | Assembly context xrefs to `0x0069AE90` | Yes |
| `ProgressClass update @ 0x00643C50` | Stores new value and synchronously repaints or draws. | Decompiled `0x00643C50`, `0x00643AE0` | Yes |
| mode-2 loading background paint | `WM_PAINT_Handler @ 0x00621E90` draws the loading background when shell record mode is `2`. | Decompiled `0x00621E90`; sibling [ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md) | Yes |

## 9. Current Rust Implementation Status

| Surface | Current behavior | Delta |
|---|---|---|
| `src/app.rs` `GameScreen::Loading` | Draws `main_menu::draw_loading_screen` for one presented frame, then calls `app_transitions::transition_to_in_game` after present. | Missing native milestone-driven progress updates during load. |
| `src/ui/main_menu.rs::draw_loading_screen` | egui dark client screen, text `Loading...`, no native SHP progress bar; `loading_screen_image()` returns `None`. | Missing `PUDLGBG*` background, dialog palette, `PROGBARM.SHP` progress surface, milestone cadence. |
| `src/app_transitions.rs::transition_to_in_game` | Performs synchronous `app_init::load_map` while no loading progress is presented. | Loader needs a progress channel/state surface or staged/pumpable loading to expose native milestones. |

## 10. Visual/UI Composition Ledger

This slot verifies the progress overlay path, not the full background stack.

| Order | Function / address | Condition / flag proof | Asset / frame | Rect / anchor | Palette / convert | Active for target? | Role |
|---|---|---|---|---|---|---|---|
| 1 | `WM_PAINT_Handler @ 0x00621E90` | shell record `+0xB0 == 2` | `PUDLGBG*.SHP` frame 0 | full client rect via mode-2 path | DIALOG/DIALOGY/DIALOGN | yes | loading background |
| 2 | `LoadProgressMgr__Constructor` path around `0x00684620` / `0x00552D60` | after progress setup and side selection | `PROGBARM.SHP` for non-campaign | computed by `FUN_00642C80`; exact pixel rect deferred | palette selected during loading manager setup | yes for Skirmish | progress chrome |
| 3 | `FUN_00643C50` -> `SendMessageA(hwnd, WM_PAINT)` or `FUN_00643AE0` | only when stored value changes | progress SHP via ProgressClass `+0x54` | stored/computed `+0x68/+0x6C`; child-control path uses `GetDlgItem(hwnd, 0x639)` | ProgressClass convert/surface | yes | progress fill/status update |
| 4 | owner-draw progress proc around `0x0061D6D0` | standard `msctls_progress32` subclass | generic progress fill | control client rect | DirectDraw surface | conditional when HWND/control path present | progress control fill |

Asset role matrix:

| Asset | Loaded | Drawn | Visible in target | Content/preview | Chrome/container | Overlay | Transition-only | Inactive | Evidence |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| `PROGBARM.SHP` | yes | yes | yes | no | yes | yes | no | no | `0x006847E1..0x00684800`, `0x00643AE0`, `0x00643720`, `0x00643400` |
| `SPLDBR.SHP` | conditional | conditional | no for Skirmish | campaign-style | yes | yes | no | yes for Skirmish | `0x006847E1..0x006847F0` branch on `g_GameMode == 0` |
| `PUDLGBG*.SHP` | yes by sibling slots | yes | yes | no | background | no | no | no | `0x00621E90`; sibling palette report |

## 11. Open Questions - Final State

- `[RESOLVED] OQ-01 - What is the progress callback? -> FUN_0069AE90, called with ECX=0x00A8B238 and milestone int on stack; it updates ProgressClass at 0x00AC4F58.` (evidence: `0x0069AE90`, xrefs)
- `[RESOLVED] OQ-02 - Is the callback visible? -> Yes; it calls ProgressClass update `0x00643C50`, which either sends `WM_PAINT` synchronously to an HWND or calls direct draw helper `0x00643AE0`.` (evidence: `0x00643C50`, `0x00643AE0`)
- `[RESOLVED] OQ-03 - Is `Init_Theater` progress visible? -> Yes; its `FUN_0069AE90` values go through the same callback and update path.` (evidence: `0x005349C0`, `0x0069AE90`)
- `[RESOLVED] OQ-04 - What are the key theater values? -> `8`, `6`, `12`, loop-changed `13..25`, final `25`.` (evidence: `0x00534A63`, `0x00534B65`, `0x00534BE9`, `0x00534D9A`, `0x00534DC5`)
- `[RESOLVED] OQ-05 - Does standard offline Skirmish use this path? -> Yes; `ScenarioClass__Read_Scenario` configures progress UI and `ScenarioClass__Full_Init` is the active loader for non-campaign `g_GameMode == 5`.` (evidence: `0x00684620`, `0x00686B20`)
- `[RESOLVED] OQ-06 - Does the callback use asynchronous invalidation? -> No callback-local `InvalidateRect`; it uses synchronous `SendMessageA(hwnd, WM_PAINT)` or direct draw.` (evidence: `0x00643C50`)
- `[RESOLVED] OQ-07 - Does the native progress update every parser byte/file? -> No; it updates at coarse milestone calls, and redraw is skipped if the stored value does not change.` (evidence: `0x0069AE90`, `0x00643C50`)
- `[RESOLVED] OQ-08 - What final value completes loading? -> `100` for normal maps; random maps pass doubled values that are halved inside `FUN_0069AE90`.` (evidence: `0x00684B2B`, `0x0069AE90`)
- `[RESOLVED] OQ-09 - Does network wait cadence apply to offline Skirmish? -> No; `FUN_00684370` returns immediately for `g_GameMode == 5`, so the wait-loop repeated `100` pulses are multiplayer-only.` (evidence: `0x00684370`)
- `[RESOLVED] OQ-10 - What is current Rust's mismatch? -> Rust presents one egui loading frame and blocks through synchronous map load without milestone updates.` (evidence: `src/app.rs`, `src/ui/main_menu.rs`, `src/app_transitions.rs` scan)
- `[DEFERRED] OQ-11 - Exact pixel rect and every text/status string on the progress surface.` (category: requires-different-system-context; reason: slot target is callback/update path, full composition belongs with loading-screen visual slot; next-step-if-pursued: continue from `0x00552D60`, `0x00642C80`, `0x00643720`)
- `[DEFERRED] OQ-12 - Runtime confirmation of whether `ProgressClass+0x64` HWND points to parent loading window or child progress control in every shell mode.` (category: needs-runtime-debugger; reason: static evidence proves both branches and synchronous repaint/direct-draw semantics, but exact HWND identity is runtime window-record data; next-step-if-pursued: trace `FUN_00643C50` arguments and `+0x64` during skirmish load)

## 12. Negative Facts / Do Not Do

- Do not implement the loading screen as only a static background; native updates visible progress during load.
- Do not invent smooth per-file progress; native uses coarse integer milestones and skips redraw when the value does not change.
- Do not use `SPLDBR.SHP` for Skirmish loading progress; non-campaign uses `PROGBARM.SHP`.
- Do not schedule progress repaint through a deferred-only invalidation model if it changes user-visible cadence; native uses synchronous `SendMessage(WM_PAINT)` or direct draw.
- Do not remove `Network_ServiceLoop`-style pump points from a parity model just because offline Skirmish does not wait for peers.

## 13. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Scenario load creates a visible progress manager before full init and drives it with milestone values. | `0x00684620`, `0x00686B20`, `0x0069AE90` | missing | `src/app.rs`, `src/app_transitions.rs`, future loading state/progress bridge | keep loading UI live while load work emits milestones | Starting standard Skirmish presents loading screen, then visible progress reaches milestones before entering game | Do not block through the full load after one frame |
| Skirmish loading progress uses `PROGBARM.SHP` and percent-like milestone values. | `0x006847E1..0x00684800`, `0x00643C50` | missing | asset loading/render UI surface | render native progress chrome/fill from retail assets and DIALOG/load palette pipeline | Loading a Skirmish map shows `PROGBARM.SHP` progress, not egui text-only bar | Do not substitute campaign `SPLDBR.SHP` |
| Theater and map parsers emit visible coarse milestones including `8/6/12..25`, `60/63/65/67/68/69/70`, and later `72..98/100`. | xrefs to `0x0069AE90` in `0x005349C0`, `0x004ACE70`, `0x00686B20`, `0x00684620` | missing | `src/map/theater.rs`, map-load orchestration, app loading state | expose loader milestones to UI without making sim depend on UI | Test load collects milestone sequence for a standard TEMPERATE Skirmish map and includes theater/map/final milestones in native order | Do not make progress continuous or reorder milestones by Rust module convenience |

Proposed Rust acceptance test names:

- `loading_progress_standard_skirmish_emits_native_milestones`
- `loading_progress_skirmish_uses_progbarm_not_spldbr`
- `loading_screen_does_not_transition_after_single_static_frame`

## 14. Remaining Uncertainty

- Exact progress surface geometry/text is not fully closed here; it requires a composition-focused pass over `0x00552D60`, `0x00642C80`, and `0x00643720`.
- Static Ghidra proves synchronous repaint/direct draw; a runtime trace would pin whether the HWND branch targets parent mode-2 paint or a child progress control for each shell configuration.

## 15. Retry Spot-Check

Retry slot 4 rechecked the handoff-critical claims in Ghidra read-only. `FUN_0069AE90` still resolves as the progress callback that halves random-map milestones, compares current percent against the requested integer, and calls `FUN_00643C50` only when progress should advance. `FUN_00643C50` stores `max * 0.01 * milestone`, caps at max, and on value change either sends `WM_PAINT` (`0x0F`) to `ProgressClass+0x64` or calls `FUN_00643AE0` for immediate draw. `ScenarioClass__Read_Scenario @ 0x00684620` still constructs/enables the load progress manager and selects `PROGBARM.SHP` for non-campaign loads. Assembly context around `ScenarioClass__Full_Init`, `Init_Theater`, and `Read_Map_Section_And_IsoMapPacks` confirms the listed milestone pushes immediately before calls to `FUN_0069AE90`.

## Sources

- Ghidra decompile: `0x00684620`, `0x00686B20`, `0x0069AE90`, `0x00643C50`, `0x00643AE0`, `0x00643E90`, `0x00643E80`, `0x00642A60`, `0x00642AD0`, `0x00642B10`, `0x00642C20`, `0x00642C80`, `0x00643400`, `0x00643720`, `0x00643670`, `0x00684370`, `0x005349C0`, `0x00621E90`.
- Ghidra xrefs/assembly context: xrefs to `0x0069AE90`, selected xrefs to `0x00643C50`, mode-2 paint context, progress-control procedure context around `0x0061D6D0`.
- Prior docs: `ASSET_PARSING_BRIDGES_GHIDRA_REPORT.md`, [ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md), `SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md`, [SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md).
- Rust scan: `src/app.rs`, `src/ui/main_menu.rs`, `src/app_transitions.rs`.
