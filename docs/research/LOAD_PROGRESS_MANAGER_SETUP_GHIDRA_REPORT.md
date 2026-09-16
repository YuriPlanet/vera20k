# LoadProgressMgr Setup Rects, Side, and Message Inputs - Ghidra Report

**Target question:** In the standard YR Skirmish scenario-load path, what loading-progress manager state is created, how is the loading side selected and passed, which progress origin/rect values are computed, are setup messages/text used for Skirmish, and which stored fields feed the later progress draw?

**Primary addresses:** `0x00552A40`, `0x00552CC0`, `0x00642C80`, `0x00642B10`, plus immediate setup calls in `ScenarioClass__Read_Scenario @ 0x00684620`.

**Investigation Mode:** exhaustive-slice.

**Non-goals:** ProgressClass fill/bar drawing internals beyond identifying data inputs; full `0x00552D60` loading composition; `PUDLGBG*` background lifecycle; network wait cadence; campaign briefing/message layout beyond Skirmish-negative proof.

**Evidence needed to mark COMPLETE:** decompile named functions; verify `0x00684620` setup ordering and arguments; verify side source and storage; verify coordinate helper return values and width override; verify Skirmish message pointer/default text behavior through the first later draw input read; scan current Rust for LoadProgressMgr equivalent.

**Stop conditions:** stop after fields/arguments that feed later draw are identified; do not decode the lower-level progress-bar pixel drawing or `0x00552D60` composition.

**Overall confidence:** HIGH for Skirmish setup state, side, origin, width override, and setup-message negative; MEDIUM for the semantic names of some helper-derived UI assets because static decompile shows values but not runtime screenshots.

**Active in YR:** Yes for standard offline Skirmish (`g_GameMode == 5`). Campaign-only branches are labeled separately.

## Verified Facts

| Fact | Evidence | Confidence | Active in YR? |
|---|---|---:|---|
| `LoadProgressMgr__Constructor @ 0x00552A40` lazily creates a singleton at `DAT_00abc9bc`, allocates `100` bytes, sets the LoadProgressMgr vtable, and zeroes the manager resource/status fields it owns (`+0x04`, `+0x08`, `+0x3c..+0x4c`, bytes `+0x50..+0x52`, and `+0x54..+0x60`). If the singleton already exists, it does not reinitialize it. | `0x00552A40` decompile; xrefs from `0x00684753`, `0x006847CE`, `0x00687581`. | HIGH | Yes |
| `ScenarioClass__Read_Scenario @ 0x00684620` creates/configures progress before reading the scenario: initializes ProgressClass max/lane count, constructs LoadProgressMgr, registers manager pointer into ProgressClass, computes side, stores it at `ScenarioClass+0x34B8`, passes it to `FUN_00642B10`, loads manager resources, selects `PROGBARM.SHP` for non-campaign, computes origin, stores origin/options, then stores width override. | `0x00684620` call order: `0x00642A60`, `0x00552A40`, `0x00552B10`, `0x00643E80`, side branch, `0x00642B10`, `0x00552A40`, `0x00552CC0`, `0x00642C20`, `0x00552BE0`, `0x00642C80`, `0x00552C90`, `0x00642DF0`. | HIGH | Yes |
| Skirmish side selection comes from the first session node country, with sentinel `-3` mapped to `0`; that country indexes `HouseTypeClass`, reads `+0xBC`, writes `ScenarioClass+0x34B8`, then `FUN_00642B10` stores the same value into ProgressClass `+0x80`. | `0x00684620` non-campaign branch reads `*(int *)(*DAT_00a8da78 + 0x4B)`, maps `-3 -> 0`, reads `*(g_HouseTypeClass_Array[index]+0xBC)`, writes `g_ScenarioClass_Instance+0x34B8`, calls `0x00642B10`; `0x00642B10` writes ProgressClass `+0x80`. | HIGH | Yes |
| Skirmish does not use the campaign/LAN `LSLoadMessage` setup string path. The only setup message pointer built in `Read_Scenario` is gated by `g_GameMode == 4 && DAT_00B779C4 != 0`; Skirmish (`g_GameMode == 5`) passes null to `FUN_00642C80`, which stores it at ProgressClass `+0x50`. Later single-lane non-campaign draw input substitutes `*DAT_00A8DA78` when `+0x50` is null, so Skirmish can still have a node/player text input, but not the setup message. | `0x00684620` `puVar9` construction gate; `0x00642C80` stores message pointer to `+0x50`; `0x00643AE0` fallback from null `+0x50` to `*DAT_00A8DA78` for non-campaign single-lane draw. | HIGH | Yes |
| Skirmish origin is explicit, not auto-centered. `FUN_00552BE0` returns `x = base_x + 0x0C, y = base_y + 0x100` when `g_ScreenWidth == DAT_007F5BE0`; otherwise `x = base_x + 0x10, y = base_y + 0x141`, where `base_x/base_y` come from `FUN_0072A9C0`. `FUN_00642C80` receives those non-`-1` values and writes ProgressClass `+0x68/+0x6C` directly. | `0x00552BE0` decompile; `0x00684620` call to `0x00552BE0` then `0x00642C80`; `0x00642C80` non-auto branch. | HIGH | Yes |
| Skirmish width override is explicit. `FUN_00552C90` returns `0x146` at `g_ScreenWidth == DAT_007F5BE0`, otherwise `0x196`; `FUN_00642DF0` stores that at ProgressClass `+0x78`. Later draw input code uses `+0x78` when it is not `-1` to override the computed progress-row width. | `0x00552C90`, `0x00642DF0`, `0x00643720`. | HIGH | Yes |
| `FUN_00552CC0` ensures manager file handles for `LOADMD.MIX` (`+0x08`) and `LOAD.MIX` (`+0x04`) exist, then uses the non-campaign setup path for Skirmish (`FUN_00696F10`, `FUN_0072B530`), while campaign uses `FUN_0072B3E0`. | `0x00552CC0` decompile. | HIGH | Yes for non-campaign branch |

## Key Layout / Field Inputs

| Field | Owner | Verified writer | Later input reader | Meaning for implementation |
|---|---|---|---|---|
| `DAT_00ABC9BC` | LoadProgressMgr singleton | `0x00552A40` | setup/resource functions | Allocate once; do not re-zero existing manager on repeated constructor calls. |
| `+0x04`, `+0x08` | LoadProgressMgr | `0x00552CC0` | manager resource setup | `LOAD.MIX` / `LOADMD.MIX` handles are created lazily. |
| `+0x80` | ProgressClass | `0x00642B10` | `0x00643AE0` campaign side path | Stores selected loading side id from `ScenarioClass+0x34B8`. |
| `+0x50` | ProgressClass | `0x00642C80` | `0x00643AE0` | Optional setup text pointer; null for Skirmish setup. |
| `+0x68/+0x6C` | ProgressClass | `0x00642C80` | `0x00643AE0`, `0x00643720` | Explicit progress origin used when later draw receives `(-1,-1)`. |
| `+0x70/+0x71` | ProgressClass | `0x00642C80` | `0x00643AE0`, `0x00643720`, `0x00643400` | Non-campaign option flags; `Read_Scenario` passes both as `g_GameMode != 0`, so both are true for Skirmish. |
| `+0x78` | ProgressClass | `0x00642DF0` | `0x00643720` | Width override: `0x146` or `0x196` for Skirmish depending on screen-width branch. |
| `+0x54` | ProgressClass | `0x00642C20` | `0x00643AE0`, `0x00643400` | Progress SHP pointer; Skirmish selects `PROGBARM.SHP`. |

## Rect / Origin Details

For Skirmish, `Read_Scenario` does not pass `(-1,-1)` to `FUN_00642C80`, so the auto-center branch in `0x00642C80` is not active for the scoped path. The active coordinates come from `0x00552BE0`:

- `g_GameMode != 0` and `g_ScreenWidth == DAT_007F5BE0`: `x = FUN_0072A9C0()[0] + 0x0C`, `y = FUN_0072A9C0()[1] + 0x100`.
- `g_GameMode != 0` and other width: `x = FUN_0072A9C0()[0] + 0x10`, `y = FUN_0072A9C0()[1] + 0x141`.

The inactive auto-center branch in `0x00642C80` computes `+0x68/+0x6C` from background rect size, progress SHP frame rect, text width, optional side emblem dimensions, and the client rectangle from `FUN_0072AD20`. This matters only when callers pass `(-1,-1)`; `Read_Scenario` Skirmish does not.

## Current Rust Status

Current Rust has no LoadProgressMgr analog or native `PROGBARM.SHP` progress state. `src/app.rs` draws a single `GameScreen::Loading` frame, then calls `app_transitions::transition_to_in_game` after present. `src/ui/main_menu.rs::draw_loading_screen` paints egui text/panel content, and `loading_screen_image()` returns `None`.

Evidence: `src/app.rs:2024`, `src/app.rs:2149`, `src/app_transitions.rs:32`, `src/ui/main_menu.rs:301`, `src/ui/main_menu.rs:388`.

## Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `0x00552A40` constructor | verified | decompile, xrefs | none for setup state |
| `0x00552CC0` manager resource setup | verified | decompile | exact asset drawing in `0x00552D60` out-of-scope |
| `0x00642B10` side setter | verified | decompile | none |
| `0x00642C80` origin/message/options setter | verified | decompile, `0x00684620` caller | none for Skirmish inputs |
| `0x00552BE0` coordinate helper | verified | decompile, sole xref from `0x00684620` | exact `FUN_0072A9C0` semantic name not required |
| `0x00552C90` width helper | verified | decompile, sole xref from `0x00684620` | none |
| `0x00642DF0` width setter | verified | decompile, xref from `0x00684620` | none |
| `0x00643AE0` first later draw input read | touched-not-exhausted | decompile | lower-level pixel composition deliberately out-of-scope |
| `0x00643720` width/origin use | touched-not-exhausted | decompile | full progress fill drawing deliberately out-of-scope |
| `0x00552D60` early Full_Init composition | deferred | user-supplied settled fact; decompile touched only to avoid expansion | follow separate visual composition investigation if needed |

## Open Questions - Final State

- `[RESOLVED] OQ-01 - Does constructor create fresh state or reuse existing manager? -> Lazy singleton; fresh 100-byte allocation only when `DAT_00ABC9BC` is null, with no reinit when non-null.` (evidence: `0x00552A40`)
- `[RESOLVED] OQ-02 - Is Skirmish on this setup path? -> Yes, `g_IsMapEditor == 0` and `g_GameMode == 5` take the non-campaign setup path in `Read_Scenario`.` (evidence: `0x00684620`)
- `[RESOLVED] OQ-03 - How is side selected for Skirmish? -> first node country at `*DAT_00A8DA78 + 0x4B`, `-3` remapped to `0`, then HouseType `+0xBC`.` (evidence: `0x00684620`)
- `[RESOLVED] OQ-04 - Where is side stored? -> `ScenarioClass+0x34B8` and ProgressClass `+0x80`.` (evidence: `0x00684620`, `0x00642B10`)
- `[RESOLVED] OQ-05 - Which progress SHP is selected? -> `PROGBARM.SHP` for Skirmish/non-campaign; `SPLDBR.SHP` is campaign-only in this branch.` (evidence: `0x00684620`, `0x00642C20`)
- `[RESOLVED] OQ-06 - Does Skirmish pass auto-center coordinates? -> No; it passes explicit coordinates returned by `0x00552BE0`.` (evidence: `0x00552BE0`, `0x00642C80`)
- `[RESOLVED] OQ-07 - What are Skirmish origin offsets? -> base `+0x0C,+0x100` at `DAT_007F5BE0` width, else base `+0x10,+0x141`.` (evidence: `0x00552BE0`)
- `[RESOLVED] OQ-08 - What is the Skirmish width override? -> `0x146` at `DAT_007F5BE0` width, otherwise `0x196`, stored to `+0x78`.` (evidence: `0x00552C90`, `0x00642DF0`)
- `[RESOLVED] OQ-09 - Is the setup message used for Skirmish? -> No; the formatted setup message is only built for `g_GameMode == 4 && DAT_00B779C4 != 0`, and Skirmish passes null to `+0x50`.` (evidence: `0x00684620`, `0x00642C80`)
- `[RESOLVED] OQ-10 - Does later draw have any text input for Skirmish? -> Yes, if `+0x50` is null in single-lane non-campaign, `0x00643AE0` falls back to `*DAT_00A8DA78`.` (evidence: `0x00643AE0`)
- `[RESOLVED] OQ-11 - Which fields feed later progress draw? -> `+0x50`, `+0x54`, `+0x68/+0x6C`, `+0x70/+0x71`, `+0x78`, and `+0x80` are the setup-produced fields that later draw reads.` (evidence: `0x00643AE0`, `0x00643720`, `0x00643400`)
- `[DEFERRED] OQ-12 - Exact visual meaning of the `*DAT_00A8DA78` fallback string in a live Skirmish screenshot.` (category: needs-runtime-debugger; reason: static evidence proves the pointer source but not the rendered text content in a specific locale/session; next-step-if-pursued: runtime trace `ProgressClass+0x50` and draw text pointer during Skirmish load)

## Visual/UI Composition Ledger

| Order | Function / address | Condition / flag proof | Asset / frame | Rect / anchor | Palette / convert | Active for target? | Role |
|---|---|---|---|---|---|---|---|
| 1 | `0x00684620` -> `0x00642C20` | `g_GameMode != 0` | `PROGBARM.SHP` | stored as ProgressClass `+0x54`; draw rect later | `+0x74` set by `0x00642C20` | yes | progress chrome/input asset |
| 2 | `0x00684620` -> `0x00552BE0` -> `0x00642C80` | non-campaign Skirmish | n/a | origin `base+0x0C,+0x100` or `base+0x10,+0x141`; stored `+0x68/+0x6C` | n/a | yes | progress placement input |
| 3 | `0x00684620` -> `0x00552C90` -> `0x00642DF0` | non-campaign Skirmish | n/a | width override `0x146/0x196`; stored `+0x78` | n/a | yes | progress row width input |
| 4 | `0x00643AE0` / `0x00643720` | later update/draw only; lower draw out-of-scope | `PROGBARM.SHP` | reads `+0x68/+0x6C`, `+0x78`; text `+0x50` or fallback | color scheme from `0x00642BB0` | yes | progress draw input consumption |

Asset role matrix:

| Asset | Loaded | Drawn | Visible in target | Content/preview | Chrome/container | Overlay | Transition-only | Inactive | Evidence |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| `PROGBARM.SHP` | yes | yes by later ProgressClass path | yes | no | yes | yes | no | no | `0x00684620`, `0x00642C20`, `0x00643AE0`, `0x00643720`, `0x00643400` |
| `SPLDBR.SHP` | branch-only | campaign-only in this branch | no for Skirmish | campaign-style | yes | yes | no | yes for Skirmish | `0x00684620` branch on `g_GameMode == 0` |
| `LOADMD.MIX` / `LOAD.MIX` | yes, handles created lazily | resource source only in this slice | input-only | no | support | no | no | no | `0x00552CC0` |

## Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Skirmish loading progress setup stores side from first node country after `-3 -> 0`, through HouseType `+0xBC`, into scenario loading side and ProgressClass `+0x80`. | `0x00684620`, `0x00642B10` | missing | future loading-screen/progress state outside `sim/` | Create loading progress state with a side id sourced from Skirmish session/country data, not from map name or arbitrary UI faction. | Start Skirmish with Allied/Soviet/Yuri first player and verify loading progress side-dependent inputs match the selected first-node country. Proposed test: `loading_progress_uses_first_node_country_side`. | Do not infer side from theater, map filename, or sidebar color. |
| Skirmish progress placement uses explicit origin from `0x00552BE0` and width override from `0x00552C90`: `base+0x0C,+0x100` and `0x146` at the narrow-width branch, otherwise `base+0x10,+0x141` and `0x196`. | `0x00552BE0`, `0x00642C80`, `0x00552C90`, `0x00642DF0`, `0x00643720` | missing | loading-screen renderer/progress layout | Store native origin and row width before progress milestones draw; use explicit Skirmish coordinates, not auto-centering. | At 640-like and wider loading resolutions, progress bar origin/width match native offsets relative to the `FUN_0072A9C0` base rect. Proposed test: `skirmish_load_progress_origin_and_width_match_native`. | Do not use the inactive `(-1,-1)` auto-center branch for Skirmish. |
| Skirmish passes no setup message pointer; later draw falls back to first session node text when `+0x50` is null. | `0x00684620`, `0x00642C80`, `0x00643AE0` | mismatch: Rust renders egui `"Loading..."`, map name, and explanatory status text | `src/ui/main_menu.rs`, future native loading progress renderer | Remove invented egui loading copy from the parity surface; if text is implemented for the progress row, source it from the native-equivalent node/session input, not `LSLoadMessage` or map name. | Skirmish loading screen has no Rust overlay panel/status prose; any progress-row label follows the first-node native input. Proposed test: `skirmish_load_progress_does_not_use_lsloadmessage_or_map_overlay_text`. | Do not use the `g_GameMode == 4` formatted setup message for offline Skirmish. |

## Negative Facts / Do Not Do

- Do not reinitialize the LoadProgressMgr singleton on every constructor call; `0x00552A40` leaves existing state intact.
- Do not use `SPLDBR.SHP` for Skirmish; `PROGBARM.SHP` is selected for non-campaign.
- Do not auto-center Skirmish progress via `FUN_00642C80`'s `(-1,-1)` branch; `Read_Scenario` supplies explicit coordinates.
- Do not pass or invent `LSLoadMessage` text for offline Skirmish; that setup message is gated to `g_GameMode == 4`.
- Do not render current Rust's egui loading panel/map/status prose as a parity surface.

## Remaining Uncertainty

- Static evidence proves the later Skirmish text pointer fallback to `*DAT_00A8DA78`, but runtime capture is still needed to name the exact displayed string for a concrete locale/session.
- `FUN_0072A9C0` supplies the base rect/origin used by `0x00552BE0`; this report treats it as a verified input source but does not rename or fully decode that provider.

## Stale Docs / Follow-up Docs

Replace the stale deferred wording in `LOADING_PROGRESS_CALLBACK_VISIBLE_UI_GHIDRA_REPORT.md` that says exact pixel rect is deferred with:

> For standard offline Skirmish, `Read_Scenario` uses explicit progress origin values from `0x00552BE0`: `FUN_0072A9C0` base `+0x0C,+0x100` at `g_ScreenWidth == DAT_007F5BE0`, otherwise base `+0x10,+0x141`; `0x00552C90` stores width override `0x146` or `0x196` through `0x00642DF0`. The `LSLoadMessage` setup pointer is null for Skirmish; later ProgressClass draw falls back to `*DAT_00A8DA78` if it needs text.

Do not patch the older report from this slot; this paragraph is the shared correction claim.

## Sources

- Ghidra decompiled: `0x00552A40`, `0x00552CC0`, `0x00642C80`, `0x00642B10`, `0x00684620`, `0x00552BE0`, `0x00552C90`, `0x00642C20`, `0x00642DF0`, `0x00643AE0`, `0x00643720`, `0x00643400`, `0x00642BB0`.
- Ghidra xrefs/callees: xrefs to `0x00552A40`, `0x00552CC0`, `0x00642C80`, `0x00642B10`, `0x00552BE0`, `0x00552C90`, `0x00642DF0`; callees from `0x00684620`.
- Prior reports consulted: `LOADING_PROGRESS_CALLBACK_VISIBLE_UI_GHIDRA_REPORT.md`, `SKIRMISH_START_TO_LOADING_SCREEN_ACTIVATION_GHIDRA_REPORT.md`, [LOADING_SCREEN_WM_PAINT_MODE2_COMPOSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_SCREEN_WM_PAINT_MODE2_COMPOSITION_GHIDRA_REPORT.md), [PUDLGBG_LOADING_SCREEN_SHP_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PUDLGBG_LOADING_SCREEN_SHP_LIFECYCLE_GHIDRA_REPORT.md).
- Current Rust scan: `src/app.rs`, `src/app_transitions.rs`, `src/ui/main_menu.rs`.

**Status:** COMPLETE for the scoped setup inputs; lower-level progress drawing remains intentionally out-of-scope.
