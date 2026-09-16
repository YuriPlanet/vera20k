# Sidebar Layer Palette / ConvertClass Pixel Colors - Ghidra Research Report

Date: 2026-05-27

**Slot:** /re-swarm soviet-sidebar-pixel-edge-cases slot 4  
**Target question:** Map ordinary Soviet sidebar layers to their native palette / `ConvertClass` / packed-color routes, and identify which source controls their pixel colors.  
**Investigation Mode:** coverage-map. The layer set spans several paint owners; this report claims a palette-routing contract, not a full pixel image diff or every minimap terrain color generator.  
**Claimed Scope:** ordinary Soviet in-game sidebar chrome, build cameos, overlays/progress, `GCLOCK2`/progress global, `POWERP`, in-game radar chrome/minimap aperture path, observer branch contrast, Ready/queue/credits text color, and dark strips where the binary proves the route.  
**Non-Scope:** full tactical minimap terrain/object-dot color generation, every observer-row asset semantic, right-panel shell `SSCR*`/`MPSSCRN*` color conversion beyond sibling handoff scope, retail screenshot/image diff, and Rust implementation patches.  
**Confidence:** High for ordinary player layer palette routes and text packed-color route; Medium for minimap generated-surface internals because this report identifies the route boundary but does not drain terrain pixel generation.  
**Active in YR:** Yes for ordinary in-game sidebar draw path. Conditional for observer branch and Yuri/fallback color branches.

## Evidence Needed To Mark COMPLETE

- Read-only Ghidra evidence for `SidebarClass::LoadSHPs`, `SidebarClass::Draw`, `StripClass::Draw`, `SBGadgetClass::Draw`, `PowerClass::Draw`, `RadarClass::Draw/Update`, `CreditsClass::Draw`, `SetSidebarTextColor`, and `PaletteLoad` / `LoadPal` inner mechanics.
- Assembly context proving handoff-critical `CC_Draw_Shape` ConvertClass arguments for base cameo, overlay/progress, gadgets, main chrome, power, radar chrome, and observer branch.
- Prior fresh report evidence for `DAT_0087f6b0` source-palette construction and text RGB initialization where this slot does not re-read string bytes.
- Focused Rust scan for existing palette shortcuts and current text/chrome surfaces.

## Stop Conditions

- Stop before mutating Ghidra or Rust.
- Stop before expanding into a full minimap terrain-color renderer investigation.
- Stop before changing stale docs; provide replacement wording only.

## 1. Overview

The ordinary Soviet sidebar uses several distinct color routes that should not be collapsed into one "theme palette":

- Base build cameo art uses `DAT_0087f6b0`, constructed from `CAMEO.PAL`.
- Most ordinary sidebar chrome, gadgets, tabs, cameo overlays/progress, and `POWERP` use `DAT_0087f6cc`, constructed in `SidebarClass::LoadSHPs` from the shared `SIDEBAR.PAL` raw buffer.
- Some active radar chrome draws use `DAT_00b0fbf8`, which is also `SIDEBAR.PAL` for Soviet/non-Yuri, but it is a separate `ConvertClass` object built by `PaletteLoad`.
- Observer-branch sidebar SHPs use `DAT_0087f6d0`, constructed from `OBSERVER.PAL`.
- Ready/status, queue-count, and credits text do not use a SHP `ConvertClass`; they use packed source RGB bytes selected by `SetSidebarTextColor`, then packed through DirectDraw loss/shift globals. Soviet uses RGB `(255,255,0)`.
- Dark text strips are `AlphaBlendRect(0, 0xAF)`: black with alpha `0xAF`, not a palette remap.

## 2. Palette / ConvertClass Sources

| Route | Source palette / color | Writer / accessor | Active in YR | Evidence |
|---|---|---|---|---|
| `DAT_0087f6b0` | `CAMEO.PAL` | game init `0x0052BA60` cluster | Yes | prior fresh `ORDINARY_BUILD_CAMEO_PALETTE_PATH`, assembly `0x0052C089..0x0052C129`; consumer `0x006A9A2A` |
| `DAT_0087f6cc` | `SIDEBAR.PAL` raw buffer via `FUN_0072f4a0 -> DAT_00b0fbe4` | `SidebarClass::LoadSHPs @ 0x006A5840` | Yes | decompile `0x006A5840`; side-piece consumer `0x006A6D2A`; gadget field consumer `0x0069DF8F`; overlay consumer `0x006A9B2B`; power consumer `0x0063FC65` |
| `DAT_0087f6d0` | `OBSERVER.PAL` raw buffer via `FUN_0072f4e0 -> DAT_00b0fbfc` | `SidebarClass::LoadSHPs @ 0x006A5840` | Conditional observer branch | decompile `0x006A5840`; observer consumers `0x006AA144`, `0x006AA2BA` |
| `DAT_00b0fbf8` | Soviet/non-Yuri `SIDEBAR.PAL`; Yuri `RADARYURI.PAL` | `PaletteLoad @ 0x0072F350`; accessor `FUN_0072F510` | Yes, side-dependent | decompile `0x0072F350`, `0x0072F510`; radar consumers `0x0065758E..0x006575FC`, `0x006533BB..0x00653409` |
| `DAT_00b0fbf0` | Soviet/non-Yuri `UIBKGD.PAL`; Yuri `UIBKGDY.PAL` | `PaletteLoad @ 0x0072F350` | Yes for left-panel/background paths, not ordinary strip chrome | prior [ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md); this slot did not find it in ordinary strip/chrome calls |
| `DAT_00b0fa1c/fa1e` | current sidebar source RGB | `SetSidebarTextColor @ 0x0072F440` | Yes | decompile `0x0072F440`; assembly `0x0072F440..0x0072F495`; text consumers in `0x006A9540` and `0x004A2370` |

`LoadPal` inner mechanics: `0x0072ADE0` reads 256 RGB triples from `.PAL`, shifts each 6-bit component left by 2, stores a 768-byte buffer, allocates `0x188`, and constructs a `ConvertClass` from the same buffer as source/dest. Active in YR via `PaletteLoad @ 0x0072F350`.

## 3. Layer Routing Contract - Ordinary Soviet Sidebar

| Layer / visual role | Native asset/global | Palette / color route | Active in YR | Evidence |
|---|---|---|---|---|
| Main strip chrome `SIDE1/SIDE2/SIDE3/ADDON` | `DAT_00b0b468/46c/470/474` | `DAT_0087f6cc` = `SIDEBAR.PAL` | Yes | `SidebarClass::Draw @ 0x006A6C30`; assembly `0x006A6D2A`, `0x006A6E14`, `0x006A6E65` load `DAT_0087f6cc` before `CC_Draw_Shape` |
| Sell/repair/tabs/scroll gadgets | gadget `+0x58` SHP pointer, `+0x50` convert | `gadget+0x50`, assigned `DAT_0087f6cc` | Yes | `SidebarClass::LoadSHPs @ 0x006A5840` assigns `+0x50`; `SBGadgetClass::Draw @ 0x0069DF8F` loads `EDX=[ESI+0x50]` |
| Ordinary base build cameo art | per-item cameo SHP pointer | `DAT_0087f6b0` = `CAMEO.PAL` | Yes | `StripClass::Draw`; assembly `0x006A9A2A..0x006A9A3E` |
| Unaffordable/flash overlay SHP | `DAT_00b07bc0` | `DAT_0087f6cc` = `SIDEBAR.PAL`; flags `0x401` or `0x404` | Yes, conditionally per slot state | assembly `0x006A9B2B..0x006A9B46`, `0x006A9B9D..0x006A9BC0` |
| Production progress / `GCLOCK2`-role overlay | `DAT_00b0b484`, frame `progress+1` | `DAT_0087f6cc` = `SIDEBAR.PAL`; flags `0x404` | Yes, when item is building | assembly `0x006A9E73..0x006A9E97`; retail `GCLOCK2.SHP` facts in sibling asset report |
| Power meter strips | `g_PowerBarSHP` / `POWERP.SHP` frames `0,4,1,2,3` | `DAT_0087f6cc` = `SIDEBAR.PAL` | Yes, when sidebar/power dirty path draws | `PowerClass::Draw @ 0x0063FB20`; assembly `0x0063FC65`, `0x0063FCBB`, `0x0063FD19` |
| Radar top/static frame from `RadarClass::Draw` | `DAT_00b0f9e0` | `DAT_0087f6cc` = `SIDEBAR.PAL` | Yes, dirty/force path | assembly `0x00653184..0x006531C0` loads `DAT_00b0f9e0` and `DAT_0087f6cc` |
| Radar active/open chrome frame | `DAT_00b04a38` frame `0/0x20/current` | `DAT_00b0fbf8`; for Soviet this is `SIDEBAR.PAL` via `FUN_0072F510` | Yes, radar-state dependent | `RadarClass::Draw @ 0x00653100`; `RadarClass::Update @ 0x00656EC0`; assembly `0x0065758E..0x006575FC`, `0x006533BB..0x00653409` |
| Generated minimap terrain/content | `this+0x121C` generated surface | generated surface blit, not `CC_Draw_Shape` palette remap | Yes when radar online/active | `RadarClass::Update @ 0x00656EC0`; surface blit at `0x0065760F..0x0065764F`; terrain color generation deferred |
| Observer sidebar row/icon SHPs | `DAT_00b0b490..b4c8` branches | `DAT_0087f6d0` = `OBSERVER.PAL` | Conditional observer branch (`g_PlayerPtr == DAT_00ac1198`) | `StripClass::Draw`; assembly `0x006AA144`, `0x006AA2BA` |
| Ready/status text | GAME.FNT text draw | packed current sidebar RGB, Soviet `(255,255,0)` | Yes | `SetSidebarTextColor @ 0x0072F440`; `StripClass::Draw` text packing and calls from sibling report; decompile `0x006A9540` |
| Queue-count text | GAME.FNT text draw | same packed current sidebar RGB as Ready/status | Yes, conditionally | decompile `0x006A9540`; sibling assembly `0x006A9D0C..0x006A9D7B` |
| Credits text / observer elapsed time | GAME.FNT text draw | same packed current sidebar RGB; flags `0x4108` | Yes | `CreditsClass::Draw @ 0x004A2370` reads `DAT_00b0fa1c/fa1e`; y `2`, x sidebar-surface width/2 |
| Dark strips behind Ready/queue/Hold | `AlphaBlendRect` | black `0` with alpha `0xAF`; not a palette route | Yes, text-present paths | decompile `0x006A9540` calls `AlphaBlendRect(0,0xaf)` after `ComputeTextRect` |

## 4. Side / Mode Notes

- Soviet ordinary player chrome has no Soviet-only palette branch for `DAT_0087f6cc`; the Soviet distinction is the SHP art selected by active MIX state, not a unique palette. Active in YR: Yes. Evidence: `SidebarClass::LoadSHPs @ 0x006A5840` and `PaletteLoad @ 0x0072F350`.
- `DAT_00b0fbf8` does branch for Yuri: side `2` gets `RADARYURI.PAL`, side `0/1` gets `SIDEBAR.PAL`. Active in YR: Yes. Evidence: `0x0072F350` decompile and `FUN_0072F510`.
- `SetSidebarTextColor` receives post-Yuri-substitution side from `InitSideMixFiles`; Soviet side `1` uses yellow and Yuri fallback also ends up yellow. Active in YR: Yes. Evidence: prior [SOVIET_SIDEBAR_TEXT_COLOR_READY_STATUS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_SIDEBAR_TEXT_COLOR_READY_STATUS_FOLLOWUP_GHIDRA_REPORT.md), plus fresh decompile `0x0072F440`.
- DIALOG-family palettes are not ordinary in-game sidebar chrome/text palettes. Active in YR: negative for this target. Evidence: [ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md); no DIALOG route in the ordinary strip/chrome consumers above.

## 5. Current Rust Implementation Status

Focused scan:

- `src/render/sidebar_chrome.rs` decodes almost every chrome/control/radar/gclock/powerp SHP in a theme atlas with one `palette_name`, currently `sidebar.pal` for Soviet. This matches many Soviet `DAT_0087f6cc` and `DAT_00b0fbf8` source-palette cases but loses the fact that base cameos use `CAMEO.PAL`, observer uses `OBSERVER.PAL`, and radar/gadget routes are distinct `ConvertClass` objects.
- `src/render/sidebar_cameo_atlas.rs` accepts a caller-provided cameo palette and uses it for build cameo SHPs. That is the correct surface for `DAT_0087f6b0` / `CAMEO.PAL`.
- `src/app_sidebar_build.rs` builds base cameos separately from `GCLOCK2` and dark-strip overlays, but its comment says `GCLOCK2` handles darkening; native overlay/progress SHPs use `SIDEBAR.PAL` via `DAT_0087f6cc` and draw flags, while dark text strips are separate `AlphaBlendRect(0,0xAF)`.
- `src/app_sidebar_text.rs` uses egui/system text and hardcoded `Color32::from_rgb(230,240,255)` for credits; native credits consume the current sidebar text RGB table, Soviet yellow.
- `src/render/sidebar_text.rs::side_highlight_color` has the verified RGB values for Ready/queue, but its comments name this as selected-unit fade/highlight rather than current sidebar text color.

## 6. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `SidebarClass::LoadSHPs` `DAT_0087f6cc/d0` construction | verified | decompile `0x006A5840` | none for source routes |
| `PaletteLoad` / `LoadPal` mechanics | verified | decompile `0x0072F350`, `0x0072ADE0`, `0x0072F510` | exact caller list beyond known side init not expanded |
| Main strip chrome ConvertClass | verified | decompile `0x006A6C30`; assembly `0x006A6D2A`, `0x006A6E14`, `0x006A6E65` | none |
| Gadget ConvertClass | verified | decompile `0x006A5840`, `0x0069DEB0`; assembly `0x0069DF8F` | none for palette |
| Ordinary cameo art route | verified from sibling + spot-check | assembly `0x006A9A2A..0x006A9A3E`; prior `ORDINARY_BUILD_CAMEO_PALETTE_PATH` | none for ordinary base art |
| Overlay/progress route | verified | assembly `0x006A9B2B`, `0x006A9B9D`, `0x006A9E7B` | exact `DAT_00b07bc0` asset semantic can be refined separately |
| Power route | verified | decompile `0x0063FB20`; assembly `0x0063FC65`, `0x0063FCBB`, `0x0063FD19` | none for palette |
| Radar chrome route | touched-not-exhausted | decompile `0x00653100`, `0x00656EC0`; assembly `0x006531AB`, `0x0065758E`, `0x006533BB` | right-panel `SSCR*` and every radar state frame outside ordinary in-game path remain sibling-scope |
| Generated minimap content colors | deferred | route boundary in `0x00656EC0` | full terrain/object/shroud pixel-color generation |
| Text source color route | verified | decompile `0x0072F440`, `0x004A2370`, `0x006A9540`; sibling text-color report | DirectDraw descriptor runtime values not rechecked |
| Dark strips | verified | decompile `0x006A9540` `AlphaBlendRect(0,0xaf)` | exact blend equation internals not rechecked |
| Observer branch route | touched-not-exhausted | assembly `0x006AA144`, `0x006AA2BA` | full observer asset matrix deferred |

## 7. Open Questions - Final State

- `[RESOLVED] Q1 - Does ordinary Soviet sidebar use one palette route for everything? -> No; base cameos, chrome/overlays/power, radar active chrome, observer branch, and text use distinct routes.` (evidence: `0x006A9A2A`, `0x006A6D2A`, `0x0065758E`, `0x006AA144`, `0x0072F440`)
- `[RESOLVED] Q2 - What feeds main strip chrome? -> `DAT_0087f6cc`, built from `SIDEBAR.PAL`.` (evidence: `0x006A5840`, `0x006A6D2A`)
- `[RESOLVED] Q3 - What feeds base cameo art? -> `DAT_0087f6b0`, built from `CAMEO.PAL`.` (evidence: `0x006A9A2A`; prior `ORDINARY_BUILD_CAMEO_PALETTE_PATH`)
- `[RESOLVED] Q4 - What feeds overlay/progress SHPs? -> `DAT_0087f6cc`, not the cameo ConvertClass.` (evidence: `0x006A9B2B`, `0x006A9B9D`, `0x006A9E7B`)
- `[RESOLVED] Q5 - What feeds `POWERP`? -> `DAT_0087f6cc`.` (evidence: `0x0063FC65`, `0x0063FCBB`, `0x0063FD19`)
- `[RESOLVED] Q6 - What feeds ordinary radar active chrome? -> split: `DAT_00b0f9e0` frame path uses `DAT_0087f6cc`; active/open `DAT_00b04a38` paths call `FUN_0072F510`, returning Soviet `DAT_00b0fbf8` = `SIDEBAR.PAL` ConvertClass.` (evidence: `0x006531AB`, `0x0065758E`, `0x0072F510`)
- `[RESOLVED] Q7 - Do Ready/queue/credits use a ConvertClass? -> No; they use packed sidebar source RGB bytes selected by `SetSidebarTextColor`.` (evidence: `0x0072F440`, `0x004A2370`, `0x006A9540`)
- `[RESOLVED] Q8 - What is the Soviet ordinary sidebar text source color? -> RGB `(255,255,0)`.` (evidence: [SOVIET_SIDEBAR_TEXT_COLOR_READY_STATUS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_SIDEBAR_TEXT_COLOR_READY_STATUS_FOLLOWUP_GHIDRA_REPORT.md); fresh branch copy at `0x0072F45D..0x0072F47A`)
- `[DEFERRED] Q9 - What exact colors does generated minimap terrain/object/shroud content use?` (category: out-of-scope; reason: this slot maps the palette route boundary, not full radar terrain pixel generation; next-step-if-pursued: trace `RadarClass__RenderCellPixel` and house/object color inputs)
- `[DEFERRED] Q10 - What exact observer row assets correspond to every `DAT_00b0b490..b4c8` global?` (category: out-of-scope; reason: observer branch is contrast-only for this report; next-step-if-pursued: run observer sidebar asset matrix investigation)
- `[DEFERRED] Q11 - What is the exact alpha equation inside `AlphaBlendRect`?` (category: bounded-cost-too-high; reason: source color and alpha constant are proven and sufficient for route contract; next-step-if-pursued: trace `AlphaBlendRect` implementation)

## 8. Visual/UI Composition Ledger

| Order | Function / address | Condition | Asset / frame | Palette / convert | Active for target? | Role |
|---:|---|---|---|---|---|---|
| 1 | `SidebarClass::Draw @ 0x006A6C30` | background redraw gate | `SIDE1#0` | `DAT_0087f6cc` / `SIDEBAR.PAL` | Yes | main chrome |
| 2 | same | repeated visible rows | `SIDE2#0` | `DAT_0087f6cc` | Yes | strip background |
| 3 | same | after side2 loop | `SIDE3#0`, `ADDON#0` | `DAT_0087f6cc` | Yes | lower/addon chrome |
| 4 | `SBGadgetClass::Draw @ 0x0069DEB0` | gadgets visible | sell/repair/tabs/scroll frames | gadget `+0x50` = `DAT_0087f6cc` | Yes | controls |
| 5 | `StripClass::Draw @ 0x006A9540` | visible build slot | base cameo frame 0 | `DAT_0087f6b0` / `CAMEO.PAL` | Yes | content |
| 6 | same | unavailable/flash/progress | overlay/progress SHPs | `DAT_0087f6cc` / `SIDEBAR.PAL` | Conditional | overlay |
| 7 | same | text present | Ready/queue/Hold text + dark rect | packed sidebar RGB + `AlphaBlendRect(0,0xAF)` | Conditional | text/strip |
| 8 | `PowerClass::Draw @ 0x0063FB20` | power dirty/forced | `POWERP` frames | `DAT_0087f6cc` / `SIDEBAR.PAL` | Yes | meter |
| 9 | `RadarClass::Draw/Update` | radar dirty/state | `DAT_00b0f9e0`, `DAT_00b04a38` | `DAT_0087f6cc` for first frame path; `DAT_00b0fbf8` for active radar chrome | Conditional | radar chrome |
| 10 | `RadarClass::Update @ 0x00656EC0` | online active radar | generated minimap surface | surface blit, no SHP ConvertClass | Conditional | minimap content |
| 11 | `CreditsClass::Draw @ 0x004A2370` | credits dirty/forced | text | packed sidebar RGB | Yes | credits |
| 12 | `StripClass::Draw` observer branch | observer player pointer | observer row/icon SHPs | `DAT_0087f6d0` / `OBSERVER.PAL` | Conditional | observer sidebar |

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Ordinary Soviet sidebar must keep base cameo art on `CAMEO.PAL` while chrome/gadgets/overlay/progress/power use `SIDEBAR.PAL` routes. | `0x006A9A2A`, `0x006A6D2A`, `0x006A9B2B`, `0x0063FC65` | Partly matched: cameo atlas is separate, but chrome atlas comments/shortcuts imply one theme palette for many roles. | `src/render/sidebar_cameo_atlas.rs`, `src/render/sidebar_chrome.rs`, `src/app_sidebar_build.rs` | Preserve separate palette inputs and tests for base cameo vs overlay/progress/power. | Base Soviet cameo pixel sample remains decoded through `cameo.pal`, while `GCLOCK2/POWERP` samples decode through `sidebar.pal`. Proposed test: `test_soviet_sidebar_cameo_and_overlay_palette_routes_are_distinct`. | HIGH; do not "simplify" by decoding all sidebar SHPs with one palette. |
| Radar ordinary in-game chrome has two sidebar-palette `ConvertClass` routes: `DAT_0087f6cc` for `DAT_00b0f9e0`, and `DAT_00b0fbf8` via `FUN_0072F510` for `DAT_00b04a38`; generated minimap content is a surface blit. | `0x006531AB`, `0x0065758E`, `0x0072F510`, `0x0065760F` | Rust currently loads generic `radar.shp` under one atlas palette and derives content from transparency. | `src/render/sidebar_chrome.rs`, `src/render/radar_anim.rs`, `src/render/minimap.rs`, `src/app_render/build_instances.rs` | Split ordinary radar chrome palette route from generated content blit and from right-panel `SSCR*`/transition paths. | Forced Soviet radar full redraw draws active chrome through the radar palette route and then blits generated minimap content, not a paletted SHP. Proposed test: `test_soviet_radar_chrome_palette_route_is_not_minimap_content_palette`. | HIGH; do not make minimap terrain colors a `SIDEBAR.PAL` SHP decode problem. |
| Ready/queue/credits text uses current sidebar packed RGB, Soviet `(255,255,0)`, and dark strips are `AlphaBlendRect(0,0xAF)`. | `0x0072F440`, `0x004A2370`, `0x006A9540`; sibling assembly `0x006A9BF1..0x006A9C46` | Ready/queue tint values match; credits uses egui `230,240,255`; dark strips use a texture approximation. | `src/render/sidebar_text.rs`, `src/app_sidebar_build.rs`, `src/app_sidebar_text.rs`, `src/render/bit_font.rs` | Use the native sidebar text color table for credits/Ready/queue and model black `0xAF` dark rects separately from text color. | Soviet credits, Ready, queue-count share yellow source RGB; dark strip color remains black alpha `0xAF`. Proposed tests: `test_soviet_sidebar_text_consumers_share_set_sidebar_text_color` and `test_sidebar_ready_queue_dark_strip_uses_black_alpha_af`. | HIGH for text screenshot parity; do not render credits with egui/system font/color for parity. |

## 10. Negative Facts / Do Not Do

- Do not decode ordinary base build cameos with `SIDEBAR.PAL`; the normal player draw loads `DAT_0087f6b0` at `0x006A9A2A`.
- Do not decode overlay/progress/`GCLOCK2`/`POWERP` with `CAMEO.PAL`; those draw through `DAT_0087f6cc` in the proven ordinary path.
- Do not use `OBSERVER.PAL` for ordinary player sidebar art; `DAT_0087f6d0` direct consumers found here are observer-branch draws at `0x006AA144` and `0x006AA2BA`.
- Do not use DIALOG-family palettes for ordinary in-game sidebar chrome or text; those belong to separate shell/loading-screen paths.
- Do not treat generated minimap terrain content as a paletted sidebar SHP; `RadarClass::Update` blits a generated surface, and its pixel generator remains a separate system.

## 11. Remaining Uncertainty

- Full minimap terrain/object/shroud pixel-color generation is deferred to a radar-pixel investigation.
- Exact observer sidebar asset matrix for every `DAT_00b0b490..b4c8` global is deferred.
- Exact `AlphaBlendRect` blend equation was not re-decompiled here; this report proves the input color `0` and alpha `0xAF`.
- Right-panel `SSCR*` / `MPSSCRN*` transition color conversion was not fully expanded here; this report keeps ordinary in-game radar separate and cites sibling reports for lifecycle/placement.

## 12. Stale Docs / Follow-up Wording

- [docs/research/SIDEBAR_CAMEO_CHROME_CONVERTCLASS_SETUP_0052BA60_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_CAMEO_CHROME_CONVERTCLASS_SETUP_0052BA60_GHIDRA_REPORT.md): replace the global mapping that says `DAT_0087f6b4 = CAMEO.PAL` and `DAT_0087f6b0 = MOUSEPAL.PAL` with: "`DAT_0087f6b0` is constructed from the palette loaded from `CAMEO.PAL` and is the ConvertClass used by ordinary player build-cameo base art; `DAT_0087f6cc` is the `SIDEBAR.PAL` ConvertClass for chrome/overlay/progress, and `DAT_0087f6d0` is the `OBSERVER.PAL` observer-branch ConvertClass."
- [docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md): replace the `Q5` note that `DAT_0087f6cc/d0` are set in `0x0052BA60` with: "`DAT_0087f6cc/d0` are rebuilt by `SidebarClass::LoadSHPs @ 0x006A5840`; `0x0052BA60` constructs the earlier general palette globals including the `CAMEO.PAL` route used by ordinary base cameos."
- `docs/research/CREDITS_COUNTER_SYSTEM.md`: replace any current Rust/implementation wording that treats credits as screen/egui-colored text with: "Native credits text is drawn on `g_SidebarSurface`, centered at surface width/2, y=2, using the current sidebar text RGB selected by `SetSidebarTextColor`; Soviet uses source RGB `(255,255,0)`."

## Sources

- Ghidra read-only decompile: `0x006A5840`, `0x006A6C30`, `0x006A9540`, `0x0069DEB0`, `0x0063FB20`, `0x00653100`, `0x00656EC0`, `0x004A2370`, `0x0072F440`, `0x0072F350`, `0x0072ADE0`, `0x0072F510`.
- Ghidra read-only assembly contexts: `0x006A6D2A`, `0x006A6E14`, `0x006A6E65`, `0x0069DF8F`, `0x006A9A2A`, `0x006A9B2B`, `0x006A9B9D`, `0x006A9E7B`, `0x006AA144`, `0x006AA2BA`, `0x0063FC65`, `0x0063FCBB`, `0x0063FD19`, `0x006531AB`, `0x0065758E`, `0x006575E5`, `0x0065760F`, `0x0072F440..0x0072F495`.
- Prior fresh docs: [ORDINARY_BUILD_CAMEO_PALETTE_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ORDINARY_BUILD_CAMEO_PALETTE_PATH_GHIDRA_REPORT.md), [SOVIET_SIDEBAR_TEXT_COLOR_READY_STATUS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_SIDEBAR_TEXT_COLOR_READY_STATUS_FOLLOWUP_GHIDRA_REPORT.md), [ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ALLIED_SIDEBAR_PALETTE_SELECTOR_GHIDRA_REPORT.md), [SIDEBAR_POWER_CREDITS_READY_TEXT_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_POWER_CREDITS_READY_TEXT_LAYOUT_GHIDRA_REPORT.md), [SOVIET_RADAR_MINIMAP_CONTENT_INSET_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SOVIET_RADAR_MINIMAP_CONTENT_INSET_GHIDRA_REPORT.md), `RETAIL_SOVIET_SIDEBAR_SHP_DIMENSIONS_OFFSETS_GHIDRA_REPORT.md`.
- Rust scan: `src/render/sidebar_chrome.rs`, `src/render/sidebar_cameo_atlas.rs`, `src/app_sidebar_build.rs`, `src/app_sidebar_text.rs`, `src/render/sidebar_text.rs`.

## Status

COMPLETE for the palette-routing contract across ordinary Soviet sidebar layers. Not a complete pixel-color generator proof for minimap terrain/object/shroud internals.
