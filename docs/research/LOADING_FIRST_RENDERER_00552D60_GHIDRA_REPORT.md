# Loading First Renderer 0x00552D60 - Ghidra Report

**Address(es):** `0x00552D60`, caller `ScenarioClass__Full_Init @ 0x00687588`, next progress call `FUN_0069AE90(3) @ 0x00687594`.
**Investigation Mode:** exhaustive-slice.
**Claimed Scope:** first verified loading renderer used during standard offline Skirmish startup, its draw order, assets, palette/ConvertClass sources, rect/origin inputs, and ordering relative to `FUN_0069AE90(3)`.
**Non-Scope:** complete `WM_PAINT_Handler` mode 2 composition, full progress fill internals after `FUN_0069AE90`, campaign `LSLoadMessage`/briefing composition, and Rust implementation changes.
**Confidence:** High for caller order, active YR path, asset selection, and top-level draw order; Medium for exact live text string content because no runtime locale/session capture was taken.
**Active in YR:** Yes for standard offline Skirmish (`g_GameMode == 5`) after `ScenarioClass__Read_Scenario` setup and early non-campaign `Full_Init` house/start assignment.

## Target Question

What does `0x00552D60` draw for the first verified standard offline Skirmish loading renderer, in what order, from which assets and ConvertClass/palette sources, at which origins/rects, and how does it relate to `WM_PAINT` mode 2 and the immediately following `FUN_0069AE90(3)` milestone?

## Non-Goals

- Do not re-decode `WM_PAINT_Handler` mode 2 beyond reconciling whether `0x00552D60` invokes or depends on it.
- Do not redo the `PUDLGBG*` lifecycle or DIALOG palette startup reports.
- Do not decode every later `PROGBARM.SHP` fill row after progress callback updates.
- Do not edit Rust, INI, in-repo docs, or Ghidra state.

## Evidence Needed To Mark Complete

- Prove standard offline Skirmish reaches `0x00552D60`.
- Prove exact ordering relative to `FUN_0069AE90(3)` with caller assembly/xref evidence.
- Identify the first renderer's background asset family and ConvertClass source.
- Confirm whether this function draws `PUDLGBG*`, `PROGBARM.SHP`, and/or `mmpb.shp`.
- Record rect/origin inputs that are visible to a Rust loading-screen implementation.

## Stop Conditions

- Stop after the ordered composition inside `0x00552D60` and the immediate `0x00687588..0x00687594` caller ordering are closed.
- Stop before later progress callback draw internals unless needed to classify `PROGBARM.SHP`.
- Stop before mode-2 `WM_PAINT` except for dependency/negative relation.

## Verified Facts

| Fact | Evidence | Confidence | Active in YR? |
|---|---|---:|---|
| `ScenarioClass__Full_Init` calls `0x00552D60`, then immediately pushes `3` and calls `FUN_0069AE90`; no intervening paint/progress call sits between them. | xref `0x00687588`; assembly context `0x00687581..0x00687594` | High | Yes |
| `0x00552D60` does not call `WM_PAINT_Handler @ 0x00621E90` and does not select `PUDLGBG*`; it has its own BSurface-backed composition path. | decompile `0x00552D60`; xrefs to `CC_Draw_Shape`; no call/xref to `0x00621E90`; sibling mode-2 report | High | Yes |
| The first background SHP is formatted as `ls%s<country>.shp`, where `%s` is `640` or `800`, then loaded into manager `+0x48`; examples include `ls%sustates.shp`, `ls%srussia.shp`, and `ls%syuri.shp`. | decompile/assembly `0x005533C5..0x005534F8`; string table pointers `0x008297D4/0x008297D8` and `0x008297E4..0x00829880` | High | Yes |
| The ConvertClass/palette for that LS background comes from `FUN_00552CC0 -> FUN_0072B530 -> FUN_0072B500`, stored in `DAT_00B0FB98`; country mappings use `MPLS*.PAL` (`MPLSU`, `MPLSR`, `MPYLS`, etc.), not DIALOG-family palettes. | decompile `0x00552CC0`, `0x0072B530`, `0x0072B500`; assembly `0x0072B666..0x0072B804`; palette pointer table `0x00844C0C..0x00844C30` | High | Yes |
| `0x00552D60` calls the assigned-player marker subpass `FUN_00640A40` at `0x00553687`; that subpass loads/draws `mmpb.shp` frame 0 conditionally from assigned slots, after the LS background draw. | xref `0x00553687`; decompile `0x00640A40`; string `mmpb.shp @ 0x00836DF4` | High | Conditional: assigned entries and color schemes must exist |
| `PROGBARM.SHP` is configured before this renderer in `ScenarioClass__Read_Scenario`, but `0x00552D60` itself does not draw the first progress milestone; `FUN_0069AE90(3)` immediately after the renderer advances/draws progress. | setup assembly `0x006847E1..0x00684800`; caller assembly `0x00687588..0x00687594`; progress callback report | High | Yes |

## Composition Ledger

| Order | Function / address | Condition / flag proof | Asset / frame | Rect / anchor | Palette / convert | Active for target? | Role |
|---|---|---|---|---|---|---|---|
| 0 | `0x00552D60` setup | allocates manager BSurface at `+0x60`; screen-sized pixel buffer | none | full `g_ScreenWidth x g_ScreenHeight`; vtable clear/copy path | n/a | yes | render target setup |
| 1 | `CC_Draw_Shape @ 0x005535FE` | requires LS SHP `+0x48` and convert `DAT_00B0FB98` non-null | `ls640/ls800<country>.shp`, frame `0` | manager rect `+0x1C/+0x20`; centered `(screen - 640/800, screen - 480/600)/2` from `0x00552B10` | `MPLS*.PAL` ConvertClass from `0x0072B530`; passed in `EDX` | yes | loading background / content frame |
| 2 | `FUN_00640A40 @ 0x00553687` | after background draw; caller passes manager surface `+0x60` | `mmpb.shp`, frame `0` when gates pass | local map-preview constants; marker offsets include `-3` X and `-2` Y | assigned-house color scheme field `+0x30C` | conditional | assigned-player marker overlay |
| 3 | `FUN_00554100` / text helpers around `0x00553EC0..0x005540A8` | non-campaign side/text branch after marker pass | font/text, no `PUDLGBG*` | 640 branch starts from `(0x10,0x7E,0x13E,0x68)` / `(0x10,0x7E,0x13E,0x68)` variants; 800 branch starts from `(0x14,0x17C/0x9E,0x18E,0x82)` then centered by high-res offset | GAME font/color conversion from selected color scheme | yes | labels/status chrome |
| 4 | `FUN_0069AE90(3) @ 0x00687594` | caller executes immediately after renderer returns | `PROGBARM.SHP` via ProgressClass already configured | progress origin from setup: base `+0x0C,+0x100` at 640 branch, else `+0x10,+0x141`; width `0x146/0x196` | ProgressClass convert/surface path | yes, after renderer | first milestone progress update |

## Asset Role Matrix

| Asset | Loaded | Drawn by `0x00552D60` | Visible in target | Role | Evidence |
|---|---:|---:|---:|---|---|
| `ls640/ls800<country>.shp` | yes | yes | yes | first renderer background/content | `0x005533C5..0x005535FE` |
| `MPLS*.PAL` / `MPYLS.PAL` | yes | convert source | yes | LS background ConvertClass | `0x0072B530`, `0x0072B500` |
| `mmpb.shp` | yes, local in marker subpass | conditional | conditional | assigned-player marker overlay | `0x00553687`, `0x00640A40`, string `0x00836DF4` |
| `PROGBARM.SHP` | yes before renderer | no, not inside `0x00552D60` | yes after `FUN_0069AE90(3)` | progress chrome/fill | `0x006847F2..0x00684800`, `0x00687594` |
| `SPLDBR.SHP` | campaign branch only | no | no for Skirmish | campaign progress asset | `0x006847E1..0x006847F0` |
| `PUDLGBG*.SHP` | yes by shell startup | no | separate mode-2 path only | WM_PAINT loading background | `0x00621E90` sibling report |

## Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| First Skirmish renderer draws LS country background (`ls640/ls800<country>.shp`) with `MPLS*.PAL` ConvertClass before any progress milestone. | `0x005533C5..0x005535FE`; `0x0072B530`; `0x00687588..94` | missing; Rust uses egui loading text/panel | `src/ui/main_menu.rs`, future loading renderer/assets | Load and draw the native LS country SHP at the manager background rect using the matching `MPLS*.PAL` convert before milestone `3`. | Start stock Skirmish as several countries; first verified loading render shows native LS country art, not egui text. Proposed test: `loading_first_renderer_draws_ls_country_background_before_progress_3`. | Do not substitute `PUDLGBG*` for this renderer. |
| `mmpb.shp` belongs to the `0x00552D60` assigned-player marker overlay after the LS background, not to the offline setup dialog first paint. | `0x00553687`; `0x00640A40`; `mmpb.shp @ 0x00836DF4` | partly mismatched risk in shell preview paths | loading renderer marker layer; keep setup preview separate | Implement `mmpb` only for assigned-player/loading marker context with assigned-slot/color-scheme gates. | Loading render with assigned slots overlays `mmpb` markers after LS art; setup preview still uses `STARTBUT.SHP`. Proposed test: `loading_first_renderer_mmpb_after_ls_background_not_setup_preview`. | Do not use `mmpb.shp` as preview backing or available-start marker art. |
| `PROGBARM.SHP` is configured before `0x00552D60` but first progress advancement is after it via `FUN_0069AE90(3)`. | `0x006847F2..0x00684800`; `0x00687588..0x00687594` | missing milestone-driven loading | app loading/progress pipeline | Preserve order: setup `PROGBARM`, render first LS/marker composition, then apply milestone `3`. | Trace a Skirmish start and assert render event precedes progress milestone `3`; proposed test: `loading_first_renderer_precedes_progress_milestone_3`. | Do not draw progress before the first LS composition in this path. |

## Negative Facts / Do Not Do

- Do not treat `0x00552D60` as the same as `WM_PAINT` mode 2; it does not call `0x00621E90` and uses LS/MPLS assets, not `PUDLGBG*`/DIALOG assets.
- Do not draw `PUDLGBG*` as the first `0x00552D60` background; that belongs to the separate mode-2 paint branch.
- Do not use `SPLDBR.SHP` for Skirmish; non-campaign setup selects `PROGBARM.SHP`.
- Do not draw `PROGBARM.SHP` before `0x00552D60` returns; the milestone callback `FUN_0069AE90(3)` follows it in the caller.
- Do not reuse setup-dialog `STARTBUT.SHP` semantics for `mmpb.shp`; `mmpb` is assigned-player/loading marker overlay.

## Remaining Uncertainty

- Exact localized text content rendered by the post-marker text helpers was not runtime-captured; static evidence closes the draw order and helper rects but not the final locale string values.
- The low-level blit from manager `+0x60` to the display surface is inferred from BSurface/XSurface calls in the function family; a runtime screenshot would confirm the exact final present timing.

## Stale Docs / Follow-up Docs

Replace any wording that says "the first standard Skirmish loading draw is the mode-2 `PUDLGBG*` background" with:

> For standard offline Skirmish, `ScenarioClass__Read_Scenario` configures `PROGBARM.SHP`, then early `ScenarioClass__Full_Init` calls `0x00552D60` as the first verified loading renderer. `0x00552D60` draws the `ls640/ls800<country>.shp` loading art through an `MPLS*.PAL` ConvertClass, conditionally overlays `mmpb.shp` assigned-player markers, and returns before `FUN_0069AE90(3)` advances the progress surface. `WM_PAINT` mode 2 remains a separate `PUDLGBG*`/DIALOG composition branch and was not found as a dependency of `0x00552D60`.

## Sources

- Ghidra decompile/xrefs: `0x00552D60`, `0x00687500`, `0x00552B10`, `0x00552CC0`, `0x0072B530`, `0x0072B500`, `0x00640A40`, `0x00684620`, `0x0069AE90`.
- Assembly/disassembly evidence: caller order `0x00687581..0x00687594`; LS asset formatting/load/draw `0x005533C5..0x005535FE`; marker call `0x00553687`; `PROGBARM.SHP` setup `0x006847E1..0x00684800`; palette load table `0x0072B666..0x0072B804`.
- String/data evidence: `640 @ 0x008297E0`, `800 @ 0x008297DC`, LS format strings `0x008297E4..0x00829880`, `mmpb.shp @ 0x00836DF4`, `PROGBARM.SHP @ 0x0083DA30`, `SPLDBR.SHP @ 0x0083DA40`, `MPLS*.PAL` table `0x00844C0C..0x00844C30`.
- Prior reports consulted: [LOADING_SCREEN_WM_PAINT_MODE2_COMPOSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_SCREEN_WM_PAINT_MODE2_COMPOSITION_GHIDRA_REPORT.md), `LOADING_PROGRESS_CALLBACK_VISIBLE_UI_GHIDRA_REPORT.md`, `LOAD_PROGRESS_MANAGER_SETUP_GHIDRA_REPORT.md`, [skirmish-ui/SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MMPB_ASSIGNED_PLAYER_MARKER_CONTEXT_GHIDRA_REPORT.md), [PUDLGBG_LOADING_SCREEN_SHP_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PUDLGBG_LOADING_SCREEN_SHP_LIFECYCLE_GHIDRA_REPORT.md).

**Status:** COMPLETE for the scoped first-renderer composition and handoff claims.
