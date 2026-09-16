# Skirmish UI Current System Model Synthesis

Date: 2026-05-23
Output type: model-synthesis
System: standard offline Yuri's Revenge Skirmish setup UI

## Scope

Included: parent dialog `0x102`, common shell paint/chrome, owner-draw buttons, checkboxes, trackbars, combo/dropdown popups, map preview and `STARTBUT.SHP` overlays, Choose Map dialog `0x6B`, current Rust implementation state, and launch handoff boundaries.

Non-scope: WOL/network lobbies, post-launch gameplay beyond the Skirmish launch handoff, full retail scrollbar drag/repeat polish, and unrelated bridge/build failures.

Spot-check note: this synthesis attempted read-only Ghidra `batch_decompile` spot-checks for `0x006AE3F0`, `0x00640710`, `0x0060F9A0`, and `0x00618D40`. Address lookup returned "Function not found"; name lookup timed out. No new binary facts are claimed here. Exact binary evidence remains the cited Ghidra reports and audit-log entries.

## Claim Table

| Claim | Best evidence | Status | Confidence | Active in YR | Safe? |
|---|---|---|---|---|---|
| Offline Skirmish creates dialog `0x102` and uses the common shell owner-draw path | Audit log 2026-05-17; active render/layout reports | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Parent shell background and paint order are common shell paint, preview surface, `STARTBUT` sprites, numeric labels, ordinary shell text | `SKIRMISH_0X102_FIRST_PAINT...`, `SKIRMISH_PREVIEW_STARTBUT...`, `SKIRMISH_START_MARKER_CLIPPING...` | confirmed | high | yes/conditional markers | IMPLEMENTATION_SAFE |
| 0x102 child rects are not scaled; only right-panel/button/status helpers and fixup controls move | `SKIRMISH_0X102_COMPLETE_CHILD_RECT_MATRIX...` | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Unit-count trackbar final rect is `(404,340,128,21)` at 800 and remains unshifted at 1024 | same rect matrix report; current Rust scan | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Checkboxes `0x54E/0x693/0x696/0x69A` shift to x=71; `0x69D` stays x=302 | same rect matrix report; current Rust scan | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Standard trackbars `0x529/0x511/0x50C` remain enabled in normal offline Skirmish | `SKIRMISH_TRACKBAR_DISABLED_RUNTIME_ENABLE_FLOW...`; INI spot-check | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Trackbar disabled paint exists in shared owner-draw code but is not a normal 0x102 parity requirement | same report | confirmed | high | conditional | DOC_PATCH_READY |
| Combo dropdowns are `ComboDropWin`, not real `LISTBOX`; rows, hit-test, top-index and child scrollbar are owned by the popup WndProc | `SKIRMISH_COMBODROPWIN_0060D540...` | confirmed | high | yes while open | IMPLEMENTATION_SAFE |
| Combo selected row fill is full content row, with scrollbar content width removed | ComboDropWin report; current Rust scan | confirmed | high | yes while open | IMPLEMENTATION_SAFE |
| Combo scrollbar arrows step one row; track clicks center the thumb proportionally and clamp top index | ComboDropWin/scrollbar reports; current Rust scan | confirmed | high | conditional overflow | IMPLEMENTATION_SAFE |
| Color dropdown population is sentinel `-2`, then colors `0..7`; row 8 must not appear | color/dropdown reports; current Rust scan | confirmed | high | yes | IMPLEMENTATION_SAFE |
| PreviewPack uses RGB pixels and child `0x468` is an anchor for parent-drawn preview | preview channel/order and overlay rect reports | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Preview aspect fit uses integer per-mille truncation and half-scaled centering | `SKIRMISH_PREVIEW_STARTBUT_OVERLAY_RECTS...`; current Rust scan | confirmed | high | yes | IMPLEMENTATION_SAFE |
| `STARTBUT` overlays and labels derive from `[Header]` start data, not `[Waypoints]` fallback | preview overlay and clipping reports | confirmed | high | conditional | IMPLEMENTATION_SAFE |
| Marker and label clipping boundary is destination surface/backbuffer, not the fitted preview rect | `SKIRMISH_START_MARKER_CLIPPING_FOOTPRINT...` | confirmed | high | conditional | IMPLEMENTATION_SAFE |
| Label numbers are 1-based at `(anchor_x-2, anchor_y-6)` using the Yellow shell/overlay color | preview overlay/clipping reports; current Rust scan | confirmed | high | conditional | IMPLEMENTATION_SAFE |
| Choose Map dialog `0x6B` is live, modal, and uses real owner-drawn listboxes `0x6EB` and `0x553` | Choose Map modal/listbox reports | confirmed | high | yes | IMPLEMENTATION_SAFE |
| Choose Map listbox row height is active shell font height + 2, standard inferred 19 px, not Rust's current 16 px constant | `SKIRMISH_OWNERDRAW_LISTBOX_00618D40...`; current Rust scan | confirmed | medium-high | yes | IMPLEMENTATION_SAFE for formula; DOC_PATCH_READY for Rust-status correction |
| Choosing/highlighting map rows in `0x553` does not refresh preview before Use Map; Create Random Map is the exception | `SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH...` | confirmed | high | yes/conditional random | IMPLEMENTATION_SAFE |
| Current Rust has modal state skeleton but app/render routing still treats `ChooseMap`, `SelectMap`, and `SelectColor` as no-ops at app level | current Rust scan | confirmed | high | n/a | IMPLEMENTATION_SAFE as status |
| Settings persistence through RA2MD.INI and exact player-name edit behavior remain incomplete model surfaces | older start-position/settings reports and current Rust scan | unknown/partial | medium | yes likely | NEEDS_REINVESTIGATE or plan before implementation |

## Current Model

Standard offline Skirmish is a full-screen parent dialog `0x102` with a mostly fixed 800x600-era child layout. At 640/800/1024, the ordinary left/middle controls keep resource-derived positions; right-panel controls and SDBTNANM navigation buttons are repositioned by shell helper branches. A small fixup pass moves `0x50C` up one pixel, moves four option checkboxes left one pixel, adjusts player-name edit width/x, and leaves BuildOffAlly untouched.

The first-paint model is common shell background/chrome first, then the Skirmish preview path. The preview surface is fit inside child `0x468` with integer per-mille math. Live start overlays come only from `[Header]` start fields already loaded into ScenarioClass, draw `STARTBUT.SHP` frame 0 at `anchor-9,-6`, and then draw 1-based yellow numeric labels at `anchor-2,-6`. Marker submissions are not filtered by the fitted preview rect; the destination render target clip is the boundary.

Combos use the custom `ComboDropWin` popup, not the normal listbox owner-draw renderer. Popup rows are painted directly, selected fill covers the content row, optional scrollbars reserve 20 px, arrow clicks step rows, and track clicks map proportionally by centering the thumb on the click. Normal color rows are sentinel `-2` followed by colors `0..7`; a synthetic row 8 is wrong.

Choose Map `0x6B` is a separate modal dialog with real owner-drawn listboxes. Map-list highlighting is not a commit and does not refresh the preview. Use Map commits selected globals and returns to the parent refresh path; Create Random Map is a special command path. Current Rust has enough state structure for a correct modal split, but no app/render integration yet.

## Implementation-Safe Facts

- Keep the 0x102 shell active behind the dev-gated Skirmish setup path, but do not make it the default launch flow until the remaining modal/settings gaps are closed.
- Use the complete rect matrix for 0x102 layout and preserve the verified one-pixel fixups.
- Keep standard game-speed, credits, and unit-count trackbars enabled in normal offline Skirmish.
- Render preview overlays after the preview surface and before ordinary shell text, with destination clipping only.
- Do not derive live start overlays from `[Waypoints]` or `LocalSize`.
- Implement combo dropdown behavior as `ComboDropWin`-style direct popup state, not as a real ListBox.
- Implement Choose Map listboxes separately from combo dropdowns; use font-height-plus-2 row height and real ListBox paint rules.
- Keep Choose Map row highlight and parent committed selection separate.

## Doc-Patch-Ready Facts

- Any older current-status prose saying Rust still lacks checkbox/dropdown/preview marker basics is stale after the recent 0x102 parity pass.
- Older statements that imply Choose Map list rows are a fixed 16 px are stale; the binary formula is active shell font height + 2.
- Older interpretations that browsing `0x553` refreshes preview are contradicted by the 0x6B preview-refresh report.
- Disabled trackbar visuals are real shared owner-draw behavior, but normal offline 0x102 trackbars do not reach that state.
- Any preview-marker prose still treating fitted-preview containment as the marker clip is stale.

## Stale Or Superseded Claims

- "0x102 needs trackbar disabled-state reveal handling for normal Skirmish" is superseded by the trackbar-disabled runtime-flow report.
- "Choose Map can reuse combo dropdown row paint" is superseded by the OwnerDraw ListBox report.
- "Preview overlays can be synthesized from Waypoints when Header data is absent" is unsafe and contradicted for the live overlay path.
- "Color dropdown should include row 8" is superseded by the normal color population evidence.
- "Dustbowl 138x75 fits to `(644,54,144,78)`" is superseded by integer per-mille fit: `(645,54,143,78)` at 800x600.

## Cross-Doc Conflicts

No current high-impact conflict blocks the 0x102 shell/dropdown/preview facts. The important reconciliation is architectural: combo popups and Choose Map listboxes are different owner-draw systems, and reports that describe one must not be applied to the other.

Some older gap scans and synthesis docs contain stale Rust-status claims because multiple 0x102 parity passes have landed since they were written. Treat those as backlog history, not canonical current status.

## Needs Re-Investigation

- Exact RA2MD.INI read/write persistence for all Skirmish UI settings, including player name, side/color/start/team, checkboxes, sliders, and selected map/mode, should get a bounded implementation handoff before coding persistence.
- Player-name edit control behavior still needs focused treatment: caret, text limits, key filtering, default value source, and commit/writeback.
- Post-launch mode-specific behavior needs a separate synthesis or investigation: MPModes callbacks, start-unit budget, random map side effects, and house/start assignment after leaving the shell.
- A current-tree 1024x768 screenshot/pixel audit should verify that the implemented right-panel shell matches the high-res parent-background model.

## Do-Not-Implement Notes

- Do not implement Choose Map preview refresh on row highlight.
- Do not use ComboDropWin scrollbar/row paint facts for `0x6B` listboxes.
- Do not add right-panel reveal/disabled-state text as part of normal 0x102 trackbar parity.
- Do not synthesize marker overlays from Waypoints or LocalSize.
- Do not expand the color dropdown past sentinel `-2` plus colors `0..7`.
- Do not make the dev Skirmish shell default solely from this synthesis; the modal and persistence gaps are still visible player-facing surfaces.

## Source Ledger

- [docs/research/skirmish-ui/SKIRMISH_0X102_COMPLETE_CHILD_RECT_MATRIX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_0X102_COMPLETE_CHILD_RECT_MATRIX_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_PREVIEW_STARTBUT_OVERLAY_RECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PREVIEW_STARTBUT_OVERLAY_RECTS_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_START_MARKER_CLIPPING_FOOTPRINT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_MARKER_CLIPPING_FOOTPRINT_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_COMBODROPWIN_0060D540_FUNCTION_BOUNDARY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_COMBODROPWIN_0060D540_FUNCTION_BOUNDARY_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_LISTBOX_00618D40_ROW_PAINT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_LISTBOX_00618D40_ROW_PAINT_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_TRACKBAR_DISABLED_RUNTIME_ENABLE_FLOW_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_TRACKBAR_DISABLED_RUNTIME_ENABLE_FLOW_GHIDRA_REPORT.md)
- [docs/research/AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) lines around 99-112 and 221-226 for Skirmish audit confirmations/stale-status notes.
- `ini/rulesmd.ini` `[MultiplayerDialogSettings]` lines 3017-3026; base fallback in `ini/rules.ini` lines 2497-2506.
- Current Rust scan: `src/ui/skirmish_shell/layout.rs`, `src/ui/skirmish_shell/state.rs`, `src/app_skirmish_shell_render.rs`, `src/app.rs`.
