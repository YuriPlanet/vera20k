# Skirmish Start To Loading-Screen Activation - Ghidra Research Report

**Address(es):** `0x006ACEE0`, `0x006AE2C0`, `0x0052D9A0`, `0x00683AB0`, `0x00684620`, `0x00686B20`, `0x00552D60`, `0x00621E90`, `0x0060A330`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** successful standard offline Skirmish Start Game command `0x617` from shell success through dialog exit, `Main_Game` launch routing, selected scenario read, loading-progress setup, first verified loading renderer, and the relation to `WM_PAINT_Handler` mode `2`.  
**Non-Scope:** Start validation failure modals, detailed house creation/spawn generation, exact MCV placement, complete `PUDLGBG*` load/free lifecycle, and progress-bar pixel geometry.  
**Confidence:** High for ordering from Start success through `ScenarioClass__Read_Scenario` / early `Full_Init` / `0x00552D60`; Medium for the negative relation to `WM_PAINT` mode `2` because static analysis did not include a runtime HWND trace of every progress repaint.  
**Active in YR:** Yes for standard offline Skirmish (`g_GameMode == 5`) unless noted.

## 1. Target Question

Trace successful offline Skirmish Start Game from command `0x617` / `FUN_006ACEE0` success through dialog exit and launcher routing to activation of the loading-screen surface/state, especially the state consumed by `WM_PAINT_Handler @ 0x00621E90` mode `2`.

Confirm order relative to:

- session packing and selected scenario filename handoff,
- selected side/country for loading art,
- Skirmish setup dialog exit and input/window suppression,
- first verified loading paint/draw,
- `ScenarioClass__Read_Scenario`, `ScenarioClass__Full_Init`, and map read.

## 2. Non-Goals

- Do not re-investigate validation failures, house creation internals, spawn placement, or starting unit generation beyond placing the loading surface in the order.
- Do not implement Rust.
- Do not mutate Ghidra or repo source.
- Do not claim `WM_PAINT` mode `2` covers progress overlay; the sibling mode-2 report already proves that branch draws background only.

## 3. Evidence Needed To Mark COMPLETE

- Binary evidence that Start success writes result `0x617` only after packing and preview cleanup.
- Binary evidence that `FUN_006AE2C0` destroys/exits the Skirmish dialog before launch.
- Binary evidence for `Main_Game` routing from successful `g_GameMode == 5` dialog return to `ScenarioClass__Start_Scenario`.
- Binary evidence for the selected filename argument (`ScenarioClass+0x125C`) and its prior selected-record source.
- Binary evidence for loading-progress setup and selected side write before scenario/map read.
- Binary evidence for the first verified loading renderer and its position relative to early `Full_Init` calls.
- Binary evidence identifying the `WM_PAINT_Handler` mode-2 writer path and whether that writer is on the standard offline Skirmish Start-to-load chain.

## 4. Stop Conditions

- Stop after the first verified loading renderer/progress activation is placed relative to Start success, `Read_Scenario`, and early `Full_Init`.
- Stop before decoding every draw in `0x00552D60`; a sibling MMPB/loading visual report owns that subpass.
- Stop before tracing `PUDLGBG*` allocation/free and exact progress text/rect geometry.

## 5. Core Verified Ordering

| Order | Stage | Verified behavior | Evidence | Active in YR |
|---:|---|---|---|---|
| 1 | Start success | `FUN_006ACEE0` disables Start, validates/packs, mirrors selected token/index, destroys preview, then writes `0x617` through the saved dialog result pointer. It does not spawn or load the map. | `0x006ACEE0`; prior packing report; final result write `0x006AD8C7..0x006AD8D5` | Yes |
| 2 | Dialog loop exits | `FUN_006AE2C0` observes local result `0x617`, calls `FUN_00622720`, clears `DAT_00B0B59C`, performs preview fallback cleanup if needed, calls `0x0072CF90` / `0x006990A0`, and returns true. | `0x006AE2C0`; `0x00622720` | Yes |
| 3 | Setup dialog is gone | `FUN_00622720` calls `DestroyWindow`, removes the dialog from the shell window stack, and restores focus to the previous dialog or `g_hWnd`. The Skirmish `0x102` HWND is not retained as the loading surface. | `0x00622720` | Yes |
| 4 | Main launcher routing | `Main_Game` case `g_GameMode == 5` calls `FUN_006AE2C0`; on true return it falls through the second switch to the scenario-launch label for modes `1/2/4/5`. | `Main_Game @ 0x0052D9A0`, call at `0x0052E168` | Yes |
| 5 | Pre-load setup | Before `ScenarioClass__Start_Scenario`, `Main_Game` runs `FUN_0054F720`, `FUN_0052FC20`, `FUN_006370B0`, display-chain begin/end, and a screen update. | `0x0052D9A0`; `0x0054F720`; `0x0052FC20`; `0x006370B0` | Yes |
| 6 | Selected filename consumer | For non-campaign Skirmish, `Main_Game` passes `ECX = ScenarioClass+0x125C` and stack arg `-1` to `ScenarioClass__Start_Scenario`. The filename buffer was populated earlier by selected-record loader `0x005E7BF0`, not by Start deriving a display string. | `0x0052E737..0x0052E745`; `0x00683AB0`; selected-map report | Yes |
| 7 | Scenario read begins | `ScenarioClass__Start_Scenario` copies/normalizes the filename, opens it, starts `LOADING`, then calls `ScenarioClass__Read_Scenario @ 0x00684620`. | `0x00683AB0`, call to `0x00684620` at `0x00683D21` | Yes |
| 8 | Loading/progress setup | `ScenarioClass__Read_Scenario` sets `ScenarioClass+0x3598 = 1`, initializes ProgressClass max `100.0`, constructs `LoadProgressMgr`, stores it into ProgressClass, computes local side from first node country, writes `ScenarioClass+0x34B8`, loads loading resources, chooses `PROGBARM.SHP` for non-campaign, and positions progress UI. | `0x00684620`; calls `0x00642A60`, `0x00552A40`, `0x00643E80`, `0x00642B10`, `0x00552CC0`, `0x00642C20`, `0x00642C80`, `0x00642DF0` | Yes |
| 9 | Map/Full_Init read | Ordinary non-SED maps call `ScenarioClass__Read_Scenario_INI @ 0x00686730`, which reaches `ScenarioClass__Full_Init @ 0x00686B20`. | `0x00684620`; `0x00686730`; prior selected-map report | Yes |
| 10 | Early Full_Init gameplay setup before first verified loading renderer | In the non-campaign path, `Full_Init` calls `ScenarioClass__Create_Houses @ 0x00687F10`, selected mode vtable `+0x80`, then `AssignStartingPoints @ 0x005EE9D0` when `DAT_00A8B244 == 2` or selected mode vtable `+0x84` otherwise. Only after that does it call `LoadProgressMgr__Constructor @ 0x00552A40` and `0x00552D60`. | `0x0068745E`; `0x00687558`; `0x0068756B`; `0x00687581..0x00687588` | Yes |
| 11 | First verified loading renderer/progress kick | `Full_Init` calls `0x00552D60` and immediately then `FUN_0069AE90(3)`. This is the first verified loading renderer/progress activation in the Start-to-load chain. | assembly context `0x00687581..0x00687594`; sibling [SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md) | Yes |

## 6. WM_PAINT Mode-2 Relation

`WM_PAINT_Handler @ 0x00621E90` mode `2` is a real loading-background composition branch. The sibling mode-2 report verifies it copies the current surface into the dialog cached `BSurface`, selects one `PUDLGBG*` SHP plus DIALOG-family palette, draws frame `0` at `(0,0)`, then blits back. It draws no text and no progress.

The mode field that `WM_PAINT_Handler` reads is the shell/window record data pointer plus offset `0xB0` (`piVar9[0x2C]`, where `piVar9 = record + 4`). `FUN_0060A330` is a verified writer: during shell child enumeration it writes `param_1[0x2C] = 2` when `FUN_0069BBE0()` is nonzero and the child/window classifiers pass. `FUN_00622B50` and `FUN_00622820` call that enumeration during shell dialog initialization/refresh.

For standard offline Skirmish Start-to-load, this slot did not find a live retained `0x102` dialog or new loading dialog creation between `FUN_006AE2C0` success and `ScenarioClass__Read_Scenario`. The verified path destroys the `0x102` dialog, then enters `ScenarioClass__Start_Scenario` and loading/progress direct-render setup. Therefore, the handoff-critical ordering should be modeled as:

```text
Start 0x617 success
  -> pack session and selected map state
  -> destroy/exit Skirmish dialog
  -> Main_Game pre-launch setup
  -> ScenarioClass::Start_Scenario(ScenarioClass+0x125C, -1)
  -> ScenarioClass::Read_Scenario
  -> load/progress setup and side selection
  -> ScenarioClass::Full_Init early non-campaign house/start setup
  -> LoadProgressMgr draw path 0x00552D60
  -> FUN_0069AE90(3), then later milestones/map sections
```

Active in YR: Yes for the Start-to-load ordering. Active in YR: Conditional for `WM_PAINT` mode `2` itself: it is active for shell/dialog records whose mode writer selects `2`, but this slot did not find that writer as the first standard offline Skirmish loading activation.

## 7. Current Rust Implementation Status

Current Rust sets `GameScreen::Loading { map_name }` directly from Start success (`src/app.rs` `start_skirmish_session` / old `start_selected_skirmish`), renders egui loading for one presented frame, then immediately calls `app_transitions::transition_to_in_game` after present.

Delta:

- Rust activates Loading too early: native destroys/returns from the Skirmish shell and performs launcher/scenario setup before the first verified loading renderer.
- Rust presents one static egui frame before loading; native builds a loading/progress manager during `ScenarioClass__Read_Scenario` and first verified renderer/progress kick occurs inside early `Full_Init`.
- Rust loading text/map-name UI does not match the verified mode-2 background branch and does not model the `PROGBARM.SHP` milestone path.

## 8. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `FUN_006ACEE0` success branch | verified | prior packing report plus `0x006AD8C7..0x006AD8D5` | none for ordering |
| `FUN_006AE2C0` exit/teardown | verified | decompile `0x006AE2C0`; `0x00622720` | none |
| Main_Game `g_GameMode == 5` success route | verified | decompile `0x0052D9A0`; xref to `0x006AE2C0` at `0x0052E168` | none |
| selected filename into scenario start | verified | `0x0052E737..0x0052E745`; selected-map token report | none |
| loading/progress setup in `Read_Scenario` | verified | `0x00684620`; sibling progress report | exact progress rect/text deferred |
| early `Full_Init` order before `0x00552D60` | verified | `0x0068745E`; `0x00687558`; `0x0068756B`; `0x00687581..88` | none for ordering |
| `WM_PAINT_Handler` mode-2 composition | verified by sibling | `0x00621E90`; mode-2 report | upstream activation is conditional |
| mode-2 writer path | verified | `FUN_0060A330`; `FUN_00622B50`; `FUN_00622820` | runtime HWND trace can prove exact dialog/control instance |
| standard Skirmish path uses retained `0x102` as loading surface | verified negative | `0x00622720` destroys `0x102`; no `0x00622650`/`0x00622B50` between success route and `Read_Scenario` in `Main_Game`/`Start_Scenario` | runtime trace optional |

## 9. Open Questions - Final State

- `[RESOLVED] OQ-1 - Does Start success directly activate loading? -> No; it packs and writes modal result after preview cleanup.` (evidence: `0x006ACEE0`)
- `[RESOLVED] OQ-2 - Does the Skirmish dialog remain as the loading window? -> No; `FUN_00622720` destroys it before `FUN_006AE2C0` returns true.` (evidence: `0x006AE2C0`; `0x00622720`)
- `[RESOLVED] OQ-3 - What is the launcher path after success? -> `Main_Game` routes mode `5` true return to the scenario-launch label, then to `ScenarioClass__Start_Scenario`.` (evidence: `0x0052D9A0`)
- `[RESOLVED] OQ-4 - Which map/file reaches scenario start? -> `ScenarioClass+0x125C`, populated by selected-record loader `0x005E7BF0`; Start mirrors token/index but does not derive the file.` (evidence: `0x0052E737..0x0052E745`; selected-map report)
- `[RESOLVED] OQ-5 - When is loading side selected? -> During `ScenarioClass__Read_Scenario`, before map/full init, from first node country into `ScenarioClass+0x34B8`.` (evidence: `0x00684620`)
- `[RESOLVED] OQ-6 - What is the first verified loading renderer in the standard Start-to-load chain? -> `0x00552D60`, called from `Full_Init` immediately before `FUN_0069AE90(3)`.` (evidence: `0x00687581..0x00687594`)
- `[RESOLVED] OQ-7 - Is that first renderer before `Full_Init`? -> No; it is inside `Full_Init`, after the early non-campaign house/start assignment block.` (evidence: `0x0068745E`; `0x00687558`; `0x0068756B`; `0x00687588`)
- `[RESOLVED] OQ-8 - What writes the `WM_PAINT` mode-2 field? -> `FUN_0060A330` writes the shell record data field to `2` under in-game/session-active conditions; `WM_PAINT_Handler` reads that field at data `+0xB0`.` (evidence: `0x0060A330`; `0x00621E90`)
- `[DEFERRED] OQ-9 - Does every progress update in offline Skirmish use direct draw or an HWND `WM_PAINT` branch?` (category: needs-runtime-debugger; reason: static evidence shows both branches in `0x00643C50`; Start-to-load ordering does not depend on which repaint branch a later progress update takes; next-step-if-pursued: trace ProgressClass `+0x64` during an offline Skirmish load)

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Start success exits/destroys the Skirmish setup dialog after packing; loading is not the same retained shell window. | `0x006ACEE0`; `0x006AE2C0`; `0x00622720` | mismatch: Rust flips `screen = GameScreen::Loading` directly inside Start action | `src/app.rs`, Skirmish shell action handling | Separate Start packing/dialog teardown from scenario load activation; clear shell input/modal state before loading. | Click Start in Skirmish; shell controls are gone/disabled and no setup dialog input remains while loading begins. | Do not keep the Skirmish shell visible behind an egui loading overlay. |
| Scenario filename comes from `ScenarioClass+0x125C` selected-record path; Start only mirrors token/index. | `0x005E7BF0`; `0x0052E737..0x0052E745`; `0x00683AB0` | partial: Rust passes `map_name` from selected record but lacks scenario filename buffer/record-loader semantics | `src/skirmish_launch.rs`, `src/app.rs`, map/scenario loader | Preserve accepted selected-record file path through launch, including future random `.SED` branch, rather than re-resolving from UI labels. | Choose custom map whose display name differs from filename; Start opens the selected file path. | Do not use display names or launch mirrors as the file opener. |
| First verified loading renderer/progress kick occurs inside early `Full_Init`, after house/start setup, then `FUN_0069AE90(3)` starts milestones. | `0x0068745E`; `0x00687558`; `0x0068756B`; `0x00687581..0x00687594` | mismatch: Rust presents a loading frame before doing synchronous map load, then transitions after one frame | `src/app.rs`, `src/app_transitions.rs`, future scenario/load-progress pipeline | Loading UI should be driven by scenario-load milestones and native ordering, not a one-frame pre-load shortcut. | Start stock Skirmish and observe native-style loading/progress persists through map/theater/full-init milestones before entering game. | Do not call `transition_to_in_game` immediately after one loading present. |

Proposed Rust test names:

- `skirmish_start_exits_shell_before_loading_state`
- `skirmish_launch_uses_selected_record_filename_buffer`
- `skirmish_loading_progress_starts_after_full_init_early_setup`

## 11. Negative Facts / Do Not Do

- Do not treat `FUN_006ACEE0` as a map-loader or loading-screen activator; it is session packing plus modal result write. Evidence: `0x006ACEE0`, `0x006AE2C0`.
- Do not keep dialog `0x102` alive as the loading screen; it is destroyed on success before `ScenarioClass__Start_Scenario`. Evidence: `0x00622720`.
- Do not present native parity as "one static loading frame, then block until game"; native has loading/progress manager state and milestone callbacks. Evidence: `0x00684620`, `0x00687594`, `LOADING_PROGRESS_CALLBACK_VISIBLE_UI_GHIDRA_REPORT.md`.
- Do not put map name, explanatory text, rounded panels, or egui status text on the mode-2 background surface; sibling mode-2 branch draws only `PUDLGBG*` frame `0` plus blits. Evidence: `0x00621E90`, mode-2 report.
- Do not reorder first visible load/progress before early non-campaign `Full_Init` setup unless runtime captures prove `0x00552D60` is not the first visible renderer; static ordering places it after `Create_Houses`/mode-start assignment. Evidence: `0x0068745E`, `0x00687558`, `0x00687588`.

## 12. Remaining Uncertainty

- Runtime trace still needed to pin whether later offline Skirmish progress updates use ProgressClass `+0x64` `SendMessage(WM_PAINT)` or the direct draw fallback in every configuration. Static evidence proves the branches; this report's ordering does not depend on the branch.
- Full `0x00552D60` visual composition was not decoded here. Sibling MMPB/loading reports verify it is the loading-screen/preview renderer context; this report uses it only as the first verified renderer anchor.
- Exact `PUDLGBG*` load/free lifecycle remains sibling-slot territory.

## 13. Stale Docs / Follow-Up Docs

- `docs/research/LOADING_PROGRESS_CALLBACK_VISIBLE_UI_GHIDRA_REPORT.md`: replace the visual ledger wording that implies mode-2 `WM_PAINT` is unconditionally the first standard Skirmish loading draw with: "For standard offline Skirmish, `ScenarioClass__Read_Scenario` sets up the loading/progress manager, and the first verified renderer in the Start-to-load chain is `0x00552D60` called from early `ScenarioClass__Full_Init` after non-campaign house/start setup, immediately before `FUN_0069AE90(3)`. `WM_PAINT_Handler` mode `2` remains the verified shell/dialog loading-background composition when a shell record's mode field is set to `2`, but this slot did not find the destroyed Skirmish `0x102` dialog retained as that loading surface."
- `docs/research/skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md`: replace "selected map token/index are mirrored at Start and consumed by scenario/map load path before or around Full_Init" with: "Start mirrors selected token/index for persistence/session state; the post-shell Skirmish file argument is `ScenarioClass+0x125C`, previously populated by `0x005E7BF0(DAT_00A8B254)` from selected record `+0x58`, and passed by `Main_Game` to `ScenarioClass__Start_Scenario` before `Read_Scenario`."

## Sources

- Ghidra read-only decompile: `FUN_006ACEE0 @ 0x006ACEE0`, `FUN_006AE2C0 @ 0x006AE2C0`, `FUN_00622720 @ 0x00622720`, `Main_Game @ 0x0052D9A0`, `ScenarioClass__Start_Scenario @ 0x00683AB0`, `ScenarioClass__Read_Scenario @ 0x00684620`, `WM_PAINT_Handler @ 0x00621E90`, `FUN_0060A330 @ 0x0060A330`.
- Ghidra assembly context: `0x0052E737..0x0052E745`, `0x00684753..0x00684800`, `0x0068745E`, `0x00687558`, `0x0068756B`, `0x00687581..0x00687594`.
- Existing reports: [skirmish-ui/SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md), [skirmish-ui/SKIRMISH_SELECTED_MAP_TOKEN_LOAD_CONSUMER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SELECTED_MAP_TOKEN_LOAD_CONSUMER_GHIDRA_REPORT.md), [skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md), [LOADING_SCREEN_WM_PAINT_MODE2_COMPOSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_SCREEN_WM_PAINT_MODE2_COMPOSITION_GHIDRA_REPORT.md), `LOADING_PROGRESS_CALLBACK_VISIBLE_UI_GHIDRA_REPORT.md`, [skirmish-ui/SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md).
- Current Rust scan: `src/app.rs`, `src/app_transitions.rs`, `src/ui/main_menu.rs`, `src/skirmish_launch.rs`.
