# Retail Soviet Sidebar SHP Dimensions / Offsets - Ghidra Research Report

**Address(es):** `0x006A5840`, `0x0072D460`, `0x0072D830`; supporting prior reports for `0x006D02B0`, `0x0063F7C0`, and `0x005B40B0`
**Investigation Mode:** exhaustive-slice for retail SHP headers and per-frame offsets; coverage-map for runtime resolver precedence
**Claimed Scope:** retail dimensions, frame counts, per-frame `frame_x/frame_y/frame_width/frame_height`, and physical MIX source for the Soviet in-game sidebar/chrome target set named in the swarm prompt
**Non-Scope:** pixel-color validation, palette conversion, tactical minimap content inset, full `CDFileClass` cache lifetime, unrelated shell/sidebar assets
**Confidence:** High for dumped SHP header/frame facts; Medium for active first-match source when duplicate assets exist outside the already-proven load order
**Active in YR:** Yes for the binary load paths and retail files named below; Conditional where the selected asset depends on screen width or missing command-button filenames

## Working Notes

Target question: Dump/verify retail asset dimensions and per-frame offsets needed for Soviet sidebar layout composition.
Non-goals: Do not reverse-engineer unrelated parser internals, render palettes, or unrelated shell/UI assets.
Evidence needed to mark COMPLETE: retail asset dump for named files plus binary filename/load evidence from active YR loader functions or prior fresh Ghidra reports.
Stop conditions: stop after one report and shared claims update; stop before Rust/doc implementation patches; defer unknowns that require runtime debugger or broader resolver proof.

## 1. Overview

The Soviet in-game sidebar mixes a 158px native layout width with mostly 168px-wide chrome canvases. The retail Soviet `SIDE1/SIDE2/SIDE3/ADDON` SHPs are all 168px wide with zero per-frame offsets, while the binary layout globals position the strip/cameo/gadget rectangles inside that wider art.

The asset dump also resolves several stale assumptions: Soviet `POWERP.SHP` is `16x2` per frame, not an 8-10px or 12px bar; Soviet `TAB00..03` are `32x28`, not Allied `28x27`; Soviet `SELL/REPAIR` are `52x32`, not Allied `64x31`; and `GCLOCK2.SHP` has 55 frames with frame 0 empty and frames 1+ on a 60x48 canvas.

## 2. Evidence Sources

- Retail install path: `<ra2-install>/`
- Read-only Python inspector over retail MIX/SHP bytes, using the same documented Westwood MIX/SHP(TS) structures as `src/assets/mix_archive.rs` and `src/assets/shp_file.rs`.
- XCC global mix database was used only to label unknown nested archive hashes: `0x7B512B17 = neutral.mix`, `0xC93B27A0 = ntrlmd.mix`.
- Read-only Ghidra spot-checks:
  - `SidebarClass__LoadSHPs @ 0x006A5840` calls `PowerClass__Init_IO`, then generic `CDFileClass__Constructor` loads for `GCLOCK2`, `SELL`, `REPAIR`, `TAB%02d`, `R-DN`, `R-UP`, `SIDE1`, `SIDE2`, `SIDE3`, `ADDON`.
  - `RadarBackground_SHPLoad @ 0x0072D460` selects radar background/open/close SHPs by side and `g_ScreenWidth == 0x280`.
  - `RadarTransitionMovie_SHPLoad @ 0x0072D830` selects `g_MinimapMovie_SHP` by side and `g_ScreenWidth == 0x280`.
- Prior fresh reports used as loader/string evidence:
  - `SIDEBAR_SOVIET_SHP_LOAD_PATH_FUN_006D02B0_GHIDRA_REPORT.md`
  - [SOVIET_RADAR_LEFT_PANEL_SHP_SELECTORS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_RADAR_LEFT_PANEL_SHP_SELECTORS_FOLLOWUP_GHIDRA_REPORT.md)
  - [SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md)
  - [SIDEBAR_POWER_CREDITS_READY_TEXT_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_POWER_CREDITS_READY_TEXT_LAYOUT_GHIDRA_REPORT.md)

## 3. Main Soviet Sidebar Assets

Active in YR: Yes. Evidence: `SidebarClass__LoadSHPs @ 0x006A5840` plus prior `FUN_006D02B0` report proves these generic filenames are loaded after side MIX setup. Retail source evidence is the physical archive column below.

| Asset | Retail source | File canvas | Frames | Per-frame header summary |
|---|---:|---:|---:|---|
| `SIDE1.SHP` | `ra2.mix -> sidec02.mix` | `168x69` | 1 | frame 0 `xy=0,0 wh=168x69 fmt=0 data=32` |
| `SIDE2.SHP` | `ra2.mix -> sidec02.mix` | `168x50` | 1 | frame 0 `xy=0,0 wh=168x50 fmt=0 data=32` |
| `SIDE3.SHP` | `ra2.mix -> sidec02.mix` | `168x26` | 1 | frame 0 `xy=0,0 wh=168x26 fmt=0 data=32` |
| `ADDON.SHP` | `ra2.mix -> sidec02.mix` | `168x63` | 1 | frame 0 `xy=0,0 wh=168x63 fmt=0 data=32` |
| `GCLOCK2.SHP` | `ra2.mix -> sidec02.mix` | `60x48` | 55 | frame 0 `xy=0,0 wh=0x0 fmt=4 data=0`; frames 1-3 `60x48 fmt=1`; frames 4+ sampled `60x48 fmt=3`; all sampled `xy=0,0` |
| `SELL.SHP` | `ra2.mix -> sidec02.mix` | `52x32` | 2 | frames 0-1 `xy=0,0 wh=52x32 fmt=0 data=56/1720` |
| `REPAIR.SHP` | `ra2.mix -> sidec02.mix` | `52x32` | 2 | frames 0-1 `xy=0,0 wh=52x32 fmt=0 data=56/1720` |
| `TAB00.SHP` | `ra2.mix -> sidec02.mix` | `32x28` | 5 | frames 0-4 `xy=0,0 wh=32x28 fmt=0 data=128,1024,1920,2816,3712` |
| `TAB01.SHP` | `ra2.mix -> sidec02.mix` | `32x28` | 5 | same frame geometry/offset sequence as `TAB00` |
| `TAB02.SHP` | `ra2.mix -> sidec02.mix` | `32x28` | 5 | same frame geometry/offset sequence as `TAB00` |
| `TAB03.SHP` | `ra2.mix -> sidec02.mix` | `32x28` | 5 | same frame geometry/offset sequence as `TAB00` |
| `R-DN.SHP` | `ra2.mix -> sidec02.mix` | `46x27` | 3 | frames 0-2 `xy=0,0 wh=46x27 fmt=0 data=80,1328,2576` |
| `R-UP.SHP` | `ra2.mix -> sidec02.mix` | `46x27` | 3 | frames 0-2 `xy=0,0 wh=46x27 fmt=0 data=80,1328,2576` |
| `POWERP.SHP` | `ra2.mix -> sidec02.mix` | `16x2` | 5 | frames 0-4 `xy=0,0 wh=16x2 fmt=0 data=128,160,192,224,256` |

Tiny details that matter:

- All main Soviet sidebar chrome/control files sampled here have zero `frame_x/frame_y`; layout offsets come from binary layout globals, not embedded SHP offsets.
- `SIDE1/SIDE2/SIDE3/ADDON` use 168px canvases, while [SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md) proves native layout width `g_SidebarWidth = 158`.
- `POWERP.SHP` has height 2, but `PowerClass__Draw @ 0x0063FB20` advances draw y by 3 per segment. The missing third pixel is native spacing, not asset height.
- `GCLOCK2.SHP` frame 0 is an empty zero-size frame. Prior progress draw reports that use `progress + 1` are consistent with retail frame 1 being the first full 60x48 image.
- Soviet `SELL/REPAIR` are smaller than Allied retail `SELL/REPAIR` (`52x32` vs `64x31`), so Allied-size comments cannot be reused for Soviet layout.

## 4. Soviet Radar / Transition Assets

Active in YR: Conditional by side and screen width. Evidence: `RadarBackground_SHPLoad @ 0x0072D460`; `RadarTransitionMovie_SHPLoad @ 0x0072D830`; selector strings and side/screen-width branch details are documented in [SOVIET_RADAR_LEFT_PANEL_SHP_SELECTORS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_RADAR_LEFT_PANEL_SHP_SELECTORS_FOLLOWUP_GHIDRA_REPORT.md).

| Asset | Retail source | Activation condition | File canvas | Frames | Per-frame header summary |
|---|---|---|---:|---:|---|
| `SSCRBKSM.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth == 640` | `472x448` | 1 | frame 0 `xy=0,0 wh=472x448 fmt=0 data=32` |
| `SSCRBKMD.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth != 640` | `632x568` | 1 | frame 0 `xy=0,0 wh=632x568 fmt=0 data=32` |
| `SSCRTSM.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth == 640` | `472x448` | 6 | frames 0-5 all `xy=0,0 wh=472x448 fmt=0`; data starts `152,211608,423064,...` |
| `SSCRTMD.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth != 640` | `472x448` | 6 | frames 0-5 all `xy=0,0 wh=472x448 fmt=0`; same sampled offsets as `SSCRTSM` |
| `SSCRASM.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth == 640` | `424x230` | 44 | frames sampled all `xy=0,0 wh=424x230 fmt=0`; data starts `1064,98584,196104,...` |
| `SSCRAMD.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth != 640` | `424x230` | 44 | frames sampled all `xy=0,0 wh=424x230 fmt=0`; same sampled offsets as `SSCRASM` |
| `MPSSCRNS.SHP` | `ra2.mix -> neutral.mix` | Soviet side, `g_ScreenWidth == 640` | `472x448` | 1 | frame 0 `xy=0,0 wh=472x448 fmt=0 data=32` |
| `MPSSCRNL.SHP` | `ra2.mix -> neutral.mix`; duplicate in `ra2md.mix -> ntrlmd.mix` | Soviet side, `g_ScreenWidth != 640` | `632x568` | 1 | base duplicate `fmt=0 bytes=359008`; YR duplicate `fmt=2 bytes=360144`; both `xy=0,0 wh=632x568 data=32` |

Important radar detail:

- `SSCRTMD.SHP` is not a 632x568 asset despite the `MD` suffix. The dumped retail file is `472x448`, matching `SSCRTSM`. The background/movie large assets (`SSCRBKMD`, `MPSSCRNL`) are `632x568`.
- `MPSSCRNL.SHP` exists in both base `neutral.mix` and YR `ntrlmd.mix`. The YR duplicate has the same file canvas/frame geometry but uses SHP format 2 and a slightly larger byte size.
- Every sampled radar frame has zero `frame_x/frame_y`; the `+80` offset split proven in [SOVIET_RADAR_RECT_AND_SSCR_PLACEMENT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_RADAR_RECT_AND_SSCR_PLACEMENT_GHIDRA_REPORT.md) is a draw-position rule, not an embedded SHP-frame offset.

## 5. Command Button Assets

Active in YR: The loader loop is active and requests `Button00.SHP` through `Button24.SHP`; retail file presence is partial. Evidence: `FUN_006D02B0` loop range in `SIDEBAR_SOVIET_SHP_LOAD_PATH_FUN_006D02B0_GHIDRA_REPORT.md`; retail scan across all top-level and first-level nested MIX archives.

| Requested asset range | Retail result | Geometry |
|---|---|---|
| `Button00.SHP` through `Button11.SHP` | present in `ra2.mix -> sidec02.mix` and `ra2.mix -> sidec01.mix` | each `52x32`, 2 frames, both frames `xy=0,0 wh=52x32 fmt=0 data=56/1720` |
| `Button12.SHP` through `Button24.SHP` | not found by name in the scanned retail archive set | no retail SHP header to dump |

Do not interpret the missing `Button12..24` as proof that the binary skips them. The binary loop still asks the file system for all 25 names; the retail archive set appears to provide only the first 12 for this install.

## 6. Current Rust Implementation Status

| Surface | Current state | Mismatch pressure |
|---|---|---|
| `src/sidebar/mod.rs` | Comments assert one 168px model and `RADAR_HEIGHT = 110` | Native main strip assets are 168px, but layout width is 158; Soviet radar `SSCR*` is not a 168x110 `radar.shp` block |
| `src/sidebar/sidebar_layout.ron` | Uses approximate `side1_height=65`, `side2_height=175`, `side3_height=0` | Retail Soviet SHPs are `SIDE1=69`, repeated `SIDE2=50`, `SIDE3=26`, `ADDON=63`; native visible height is formula-driven |
| `src/render/sidebar_chrome.rs` | Builds Soviet atlas directly from `sidec02.mix`, includes `radar.shp`, optional `power.shp`, expects 5 repair/sell frames | Binary-proven Soviet radar selector uses `SSCR*`/`MPSSCRN*`; retail Soviet repair/sell have 2 frames, not 5; `power.shp` is not part of the proven meter path |
| `src/app_sidebar_build.rs` | Power bar placement/scaling is layout-driven | Native Soviet `POWERP.SHP` is `16x2` and y-advances by 3 from `x=0,y=227` |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Main Soviet sidebar SHP headers | verified | retail `ra2.mix -> sidec02.mix` dump | pixel-color/palette verification |
| Main generic load path | verified | `SidebarClass__LoadSHPs @ 0x006A5840`; `SIDEBAR_SOVIET_SHP_LOAD_PATH...` | full CDFile cache lifetime |
| `POWERP.SHP` dimensions | verified | retail `ra2.mix -> sidec02.mix`; `PowerClass__Draw` prior report | exact rendered pixel colors under `SIDEBAR.PAL` |
| Soviet radar/transition SHP headers | verified | retail `neutral.mix` / `ntrlmd.mix`; `0x0072D460`, `0x0072D830` | runtime proof of which duplicate `MPSSCRNL` wins in every load order |
| `Button00..24` retail presence | touched-not-exhausted | retail all-archive scan by name | absence should be confirmed against live `CDFileClass` miss behavior if command buttons become visible |
| Embedded frame offsets | verified for dumped target set | SHP frame headers | none for named dumped files; all zero except `GCLOCK2` empty frame dimensions |
| Tactical minimap inset | deferred | out of slot | separate `SOVIET_RADAR_MINIMAP_CONTENT_INSET` target |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-1 - Are main Soviet sidebar SHPs physically 168px wide? -> Yes for SIDE1/SIDE2/SIDE3/ADDON; each has 168px canvas.` (evidence: retail `ra2.mix -> sidec02.mix`)
- `[RESOLVED] OQ-2 - Do main Soviet SHPs use embedded frame offsets? -> No for sampled main files; all dumped frames have `frame_x=0, frame_y=0`.` (evidence: SHP frame headers)
- `[RESOLVED] OQ-3 - Is layout width equal to asset width? -> No; prior binary layout report proves `g_SidebarWidth=158` while assets are often 168px.` (evidence: [SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-4 - What is Soviet `POWERP.SHP` native size? -> `16x2`, five frames, all zero-offset.` (evidence: retail `ra2.mix -> sidec02.mix`)
- `[RESOLVED] OQ-5 - Is `GCLOCK2.SHP` 54 or 55 frames? -> 55 frames; frame 0 is zero-size, frames 1+ are 60x48 sampled full frames.` (evidence: retail `ra2.mix -> sidec02.mix`)
- `[RESOLVED] OQ-6 - Are Soviet sell/repair same as Allied? -> No; Soviet is `52x32` two frames; Allied sidec01 dump is `64x31` two frames.` (evidence: retail `sidec02.mix` and `sidec01.mix`)
- `[RESOLVED] OQ-7 - Are Soviet tabs same as Allied? -> No; Soviet `TAB00..03` are `32x28`, Allied are `28x27`.` (evidence: retail `sidec02.mix` and `sidec01.mix`)
- `[RESOLVED] OQ-8 - Are radar SHP embedded offsets nonzero? -> No for dumped `SSCR*` and `MPSSCRN*` frames; all sampled frames have `xy=0,0`.` (evidence: retail `neutral.mix` / `ntrlmd.mix`)
- `[RESOLVED] OQ-9 - Does the binary actively load these filenames in YR? -> Yes for main sidebar load path and radar selector paths.` (evidence: `0x006A5840`, `0x0072D460`, `0x0072D830`)
- `[RESOLVED] OQ-10 - Are all requested `Button00..24` present? -> No by retail all-archive scan: only `Button00..11` were found by name.` (evidence: retail archive scan)
- `[DEFERRED] OQ-11 - Which `MPSSCRNL.SHP` duplicate wins at runtime?` (category: requires-different-system-context; reason: needs full live global MIX insertion order at radar load time; next-step-if-pursued: trace neutral/ntrlmd load order around `MIX_LoadNeutral` and `LoadFileFromMIX` cache state)
- `[DEFERRED] OQ-12 - What is the tactical minimap content inset inside `SSCR*`?` (category: out-of-scope; reason: this slot dumps chrome SHP headers only; next-step-if-pursued: run the dedicated minimap-content-inset slot)

## 9. Visual/UI Composition Ledger

| Order | Function / address | Condition / flag proof | Asset / frame | Rect / anchor | Palette / convert | Active for target? | Role |
|---|---|---|---|---|---|---|---|
| 1 | `SidebarClass::Draw @ 0x006A6C30` via prior draw-order report | redraw/full-strip branch | `SIDE1#0` | x 0, y `g_SidebarWidth` | `DAT_0087f6cc` / sidebar palette path | Yes | chrome top strip |
| 2 | same | repeated while visible height requires it | `SIDE2#0` | x 0, repeated vertical tile | same | Yes | cameo-row background tile |
| 3 | same | after `SIDE2` loop | `SIDE3#0` | x 0, after tile loop | same | Yes | bottom cap |
| 4 | same | after `SIDE3` | `ADDON#0` | x 0, after side strip | same | Yes | addon/extension chrome |
| 5 | `SBGadgetClass::Draw @ 0x0069DEB0` via prior draw-order report | gadget states | `SELL`, `REPAIR`, `TAB00..03`, `R-DN/R-UP` | gadget rect globals | sidebar palette path | Yes | controls |
| 6 | `StripClass::Draw @ 0x006A9540` via prior grid/text reports | active strip | cameo art plus `GCLOCK2#progress+1` | 60x48 cameos | cameo/sidebar palette split | Yes | build strip content/overlay |
| 7 | `PowerClass::Draw @ 0x0063FB20` via prior power report | active sidebar | `POWERP#0/#4/#1/#2/#3` | Soviet x 0, y 227, y += 3 | sidebar palette path | Yes | power meter |
| 8 | `RadarBackground_SHPLoad @ 0x0072D460` + radar draw reports | Soviet side, width branch | `SSCRBK*`, `SSCRT*`, `SSCRA*` | prior placement report: `DAT_00b0fc1c` and selective `+80` | radar/sidebar convert path not fully rechecked here | Conditional | radar chrome/background/transition |
| 9 | `RadarTransitionMovie_SHPLoad @ 0x0072D830` + radar draw reports | Soviet side, width branch | `MPSSCRNS/L` | prior placement report: `DAT_00b0fc1c` | not rechecked here | Conditional | minimap/radar transition movie |

## 10. Asset Role Matrix

| Asset | Loaded | Drawn | Visible in target | Content/preview | Chrome/container | Overlay | Transition-only | Inactive | Evidence |
|---|---|---|---|---|---|---|---|---|
| `SIDE1/2/3.SHP` | Yes | Yes | Yes | No | Yes | No | No | No | `0x006A5840`, draw-order report, retail dump |
| `ADDON.SHP` | Yes | Yes | Yes | No | Yes | No | No | No | same |
| `GCLOCK2.SHP` | Yes | Yes when production/progress visible | Conditional | No | No | Yes | No | No | `0x006A5840`, prior progress docs, retail dump |
| `SELL/REPAIR.SHP` | Yes | Yes as gadgets | Conditional by gadget visibility/state | No | No | Control overlay | No | No | `0x006A5840`, retail dump |
| `TAB00..03.SHP` | Yes | Yes | Yes | No | No | Control overlay | No | No | `0x006A5840`, retail dump |
| `R-DN/R-UP.SHP` | Yes | Yes | Conditional by scroll state | No | No | Control overlay | No | No | `0x006A5840`, retail dump |
| `POWERP.SHP` | Yes | Yes | Yes when sidebar visible | No | No | Meter overlay | No | No | `0x0063F7C0`/prior report, retail dump |
| `SSCRBK*` | Conditional | Yes in Soviet radar path | Conditional by side/width/radar mode | No | Yes | No | No | No | `0x0072D460`, retail dump |
| `SSCRT*` | Conditional | Yes in Soviet radar transition/open path | Conditional | No | Yes | No | Yes | No | `0x0072D460`, retail dump |
| `SSCRA*` | Conditional | close consumer not fully re-traced here | Conditional | No | Yes | No | Yes | No | selector proof + retail dump |
| `MPSSCRN*` | Conditional | Yes in minimap/movie path | Conditional | No | Yes | No | Yes | No | `0x0072D830`, retail dump |
| `Button00..11` | Requested and present | Unchecked | Unchecked | No | No | Command-button art | No | No | `0x006D02B0`, retail dump |
| `Button12..24` | Requested but not found in retail scan | Unchecked | Unchecked | No | No | Command-button request slots | No | Conditional missing | `0x006D02B0`, retail all-archive scan |

## 11. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Soviet chrome art uses 168px SHP canvases while binary layout uses 158px `g_SidebarWidth` | retail `SIDE1/2/3/ADDON` dump; layout report | Rust comments/model still conflate all sidebar width as 168 | `src/sidebar/mod.rs`, `src/sidebar/layout_spec.rs`, `src/sidebar/sidebar_layout.ron` | separate canvas width from native layout/strip width | Render a Soviet sidebar at 800x600 and assert chrome quads use 168px assets while strip/gadget placement uses native 158px layout coordinates | `test_soviet_sidebar_canvas_width_differs_from_native_layout_width`; HIGH |
| Soviet control SHP dimensions differ from Allied: `SELL/REPAIR=52x32`, `TAB00..03=32x28`, `R-UP/DN=46x27`, `POWERP=16x2` | retail `sidec02.mix` dump plus active load path | Rust comments and layout RON contain approximated/Allied-sized values; repair/sell renderer tries 5 frames | `src/render/sidebar_chrome.rs`, `src/sidebar/sidebar_layout.ron`, `src/app_sidebar_build.rs` | consume actual SHP pixel sizes and available frame counts instead of hardcoded Allied assumptions | Soviet theme atlas reports exact pixel sizes and does not warn/fallback because frames 2-4 of `SELL/REPAIR` are absent | `test_soviet_sidebar_control_dimensions_come_from_retail_shp_headers`; HIGH |
| Soviet radar assets are `SSCR*`/`MPSSCRN*` with large canvases and zero embedded offsets, not `radar.shp` 168x110 | `0x0072D460`, `0x0072D830`, retail `neutral.mix`/`ntrlmd.mix` dump | `src/render/sidebar_chrome.rs` loads `"radar.shp"` for Soviet | `src/render/sidebar_chrome.rs`, radar animation/layout code | add separate Soviet radar asset family and draw-position rules from prior radar reports | At 640 load/use `SSCRBKSM/SSCRTSM/SSCRASM/MPSSCRNS`; at 800+ load/use `SSCRBKMD/SSCRTMD/SSCRAMD/MPSSCRNL`; no 168x110 `radar.shp` fallback for this path | `test_soviet_radar_chrome_uses_sscr_assets_not_generic_radar_shp`; HIGH |

## 12. Negative Facts / Do Not Do

- Do not use `168px` as the native layout width. Active in YR: Yes, layout path uses 158 while retail chrome canvases are 168. Evidence: prior layout report plus retail dump.
- Do not size Soviet `POWERP.SHP` as 8, 10, or 12 pixels wide. Active in YR: Yes, Soviet retail `POWERP.SHP` is `16x2` with five zero-offset frames. Evidence: `ra2.mix -> sidec02.mix`.
- Do not assume `GCLOCK2.SHP` frame 0 is a normal visible progress frame. Active in YR: Yes, loaded file has 55 frames and frame 0 is zero-size; progress draw reports use `progress + 1`. Evidence: retail dump plus prior progress docs.
- Do not reuse Allied `SELL/REPAIR/TAB` dimensions for Soviet. Active in YR: Yes, Soviet and Allied retail headers differ. Evidence: `sidec02.mix` vs `sidec01.mix` dump.
- Do not make embedded SHP offsets explain the Soviet radar `+80` placement split. Active in YR: Yes for radar draw path; all dumped `SSCR*`/`MPSSCRN*` frames have `xy=0,0`. Evidence: retail dump plus prior radar placement report.

## 13. Remaining Uncertainty

- Which physical duplicate of `MPSSCRNL.SHP` wins in every runtime load state remains deferred; both `neutral.mix` and `ntrlmd.mix` contain the same geometry, but `ntrlmd.mix` uses SHP format 2 and a different byte size.
- `Button12.SHP` through `Button24.SHP` were not found by name in the scanned retail archives despite the active binary loop requesting them; live miss/null handling for those command-button globals was not traced here.
- Tactical minimap content/inset inside `SSCR*` remains outside this slot.
- Pixel colors and palette conversion were not verified; this report is strictly dimensions/frame headers/source membership.

## 14. Stale Docs / Follow-up Wording

- `src/sidebar/mod.rs`: replace `Original RA2 sidebar chrome width (all SHPs are 168px wide).` with `Retail sidebar chrome canvases are often 168px wide, but native gamemd layout width is 158px; keep canvas width and layout width separate.`
- `src/sidebar/mod.rs`: replace the module layout sentence `radar (168x110) -> side1 (168x69) -> tabs (168x16) -> side2 tiled (168x50) -> side3 (168x26)` with `For the Soviet in-game sidebar, main strip chrome is SIDE1 168x69, repeated SIDE2 168x50, SIDE3 168x26, ADDON 168x63; Soviet radar uses separate SSCR*/MPSSCRN* assets rather than a 168x110 radar.shp block.`
- `src/render/sidebar_chrome.rs`: replace the art-piece comments `repair.shp (64x31)`, `sell.shp (64x31)`, and `power.shp (27x30)` with `Soviet repair/sell retail SHPs are 52x32 with 2 frames; the binary-proven power meter asset is POWERP.SHP, 16x2 with 5 frames for Soviet.`
- `src/sidebar/sidebar_layout.ron`: replace approximate `side1_height: 65.0`, `side2_height: 175.0`, `side3_height: 0.0` with wording in a future config/doc pass that these are non-native approximation fields; retail Soviet SHP heights are `SIDE1=69`, `SIDE2=50`, `SIDE3=26`, `ADDON=63`, and visible row count is binary-formula-driven.
- [docs/research/traces/POWER_BAR_PIXEL_RENDERING_LAYOUT_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/POWER_BAR_PIXEL_RENDERING_LAYOUT_TRACE.md): replace `POWERP.SHP width not directly measured here but assumed ~8-10px` with `Retail Soviet POWERP.SHP is 16x2, five zero-offset frames; native PowerClass draws at Soviet x=0,y=227 and advances y by 3, so the inter-segment gap is a draw cadence detail, not SHP height.`

## Sources

- Ghidra read-only decompile: `SidebarClass__LoadSHPs @ 0x006A5840`
- Ghidra read-only decompile: `RadarBackground_SHPLoad @ 0x0072D460`
- Ghidra read-only decompile: `RadarTransitionMovie_SHPLoad @ 0x0072D830`
- `<ra2-install>/ra2.mix -> sidec02.mix`
- `<ra2-install>/ra2.mix -> sidec01.mix`
- `<ra2-install>/ra2.mix -> neutral.mix`
- `<ra2-install>/ra2md.mix -> ntrlmd.mix`
- `C:/Program Files (x86)/XCC/Utilities/global mix database.dat`
- `docs/research/SIDEBAR_SOVIET_SHP_LOAD_PATH_FUN_006D02B0_GHIDRA_REPORT.md`
- [docs/research/SOVIET_RADAR_LEFT_PANEL_SHP_SELECTORS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_RADAR_LEFT_PANEL_SHP_SELECTORS_FOLLOWUP_GHIDRA_REPORT.md)
- [docs/research/SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_INIT_LAYOUT_GLOBALS_EXACT_RECHECK_GHIDRA_REPORT.md)
- [docs/research/SIDEBAR_POWER_CREDITS_READY_TEXT_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_POWER_CREDITS_READY_TEXT_LAYOUT_GHIDRA_REPORT.md)
