# Skirmish Choose Map Modal Shell Composition - Ghidra Research Report

**Address(es):** `0x006ACEE0`, `0x005E68A0`, callback bytes at `0x005E6920`, `0x0060CF00`, `0x0072D120`, `0x00622820`, `0x0060C540`, `0x0060F9A0`, `0x005E7160`, `0x007757E0`, `0x007759E0`; PE `RT_DIALOG 0x6B`.
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** how dialog `0x6B` composes as the Choose Map shell modal over/replacing Skirmish setup `0x102`: resource rects, source assets, parent hide/show and repaint behavior, modal background/chrome, list/static/preview/button composition, Use/Cancel/Create Random button placement, and return behavior back to `0x102`.
**Non-Scope:** map-list population internals, PreviewPack decode internals, random-map generator visuals after the `0x583` command boundary, broad `0x102` shell parity, non-offline/WOL variants, and Rust implementation changes.
**Confidence:** High for standard offline YR modal entry, parent hide/show ordering, resource `0x6B` geometry, modal asset binding, common fullscreen shell setup, owner-draw control classification, button command/result boundary, and current Rust composition delta. Medium for exact runtime RGB at `>800` widths because native screenshots were not captured.
**Active in YR:** Yes for standard offline Skirmish; Conditional for player-clicked branches (`0x6C5`, `0x5C0`, `0x583`) and for `MnScrnLCustomizeBattle.shp` loading, which the binary gates on exact screen width `800`.

## 0. Investigation Gate

**Target question:** Verify how Skirmish Choose Map modal dialog `0x6B` composes over or replaces the Skirmish setup shell: dialog rect/source assets, parent hiding/redraw behavior, modal chrome, mode list/map list/preview/button/text composition, Use/Cancel/Create Random placement, and repaint/return behavior back to `0x102`.

**Non-goals:** Do not reopen map-list population/order, preview decode, random-map generator internals, broad setup `0x102`, Rust code changes, INI edits, Ghidra mutations, or swarm claims.

**Evidence needed to mark COMPLETE:** Fresh current Rust scan; fresh PE resource read of `RT_DIALOG 0x6B`; Ghidra decompile plus assembly/context for parent hide/show, modal wrapper, asset binding, common shell setup, owner-draw subclassing, and accept/cancel/create-random command boundary; prior docs reconciled; all open questions resolved or explicitly deferred.

**Stop conditions:** Write exactly this report at `docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_SHELL_COMPOSITION_GHIDRA_REPORT.md`; do not modify Rust, INI, in-repo docs, other docs, Ghidra state, or [.swarm-claims.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/.swarm-claims.md).

## 1. Overview

Retail `gamemd.exe` does not paint Choose Map as a translucent child panel over a still-visible Skirmish setup shell. The offline setup command `0x5AA` saves selection state, hides setup `0x102`, enters a separate modal shell dialog created from resource `0x6B`, then shows setup again after modal result handling.

The chooser's composition is resource-driven and shell-owner-drawn: `0x6B` is a `533x369` dialog with two owner-drawn listboxes, three right-column owner-drawn buttons, right-panel title/preview statics, headings, and a bottom status strip. Its shell background/palette binding is the `MnScrnLCustomizeBattle.*` path, distinct from setup `0x102`'s background binding.

Current Rust now opens and renders a primitive `ChooseMapModalState`, so older "no modal exists" reports are stale. The remaining visible mismatch is composition: Rust still builds the setup shell underneath, uses solid modal rectangles instead of `MnScrnLCustomizeBattle` chrome, and uses non-resource rects for the buttons/title/preview.

## 2. Dialog Resource And Control Rects

Fresh PE resource parsing of retail `<ra2-install>/gamemd.exe` found `RT_DIALOG` id `0x6B`, lang `0x409`, RVA `0x7EE6D8`, file offset `0x4F26D8`, size `636`. The template is `DIALOGEX`, style `0x40000040`, exstyle `0`, font `MS Sans Serif` 8 pt, item count `11`, rect `(0,0,533,369)`.

Active in YR: Yes. Evidence: the live modal wrapper passes dialog id `0x6B` to the dialog creation helper (`0x005E68B7..0x005E68C4`), and the parent `0x5AA` branch calls that wrapper (`0x006AD93C..0x006AD94C`).

| ID | Rect | Class | Style / exstyle | Title / role | Active in YR |
|---:|---:|---|---|---|---|
| `0x5C0` | `(425,346,108,23)` | `BUTTON` | `0x5000000B` / `0` | `GUI:Cancel` | Yes, when clicked |
| `0x6C5` | `(425,122,108,23)` | `BUTTON` | `0x5000000B` / `0` | `GUI:UseMap` | Yes, when clicked |
| `0x583` | `(425,149,108,23)` | `BUTTON` | `0x5000000B` / `0` | `GUI:CreateRandomMap` | Conditional, when clicked |
| `0x694` | `(425,1,108,10)` | `STATIC` | `0x50020001` / `0` | `GUI:ChooseMap` title | Yes |
| `-1` | `(80,20,257,12)` | `STATIC` | `0x50000201` / `0` | `GUI:SelectEngagement` | Yes |
| `0x6EB` | `(77,78,130,211)` | `LISTBOX` | `0x50000151` / `0` | mode/category list | Yes |
| `0x553` | `(225,78,130,211)` | `LISTBOX` | `0x50000151` / `0` | map list | Yes |
| `-1` | `(77,60,130,10)` | `STATIC` | `0x50000201` / `0` | `GUI:GameType` heading | Yes |
| `-1` | `(225,60,130,10)` | `STATIC` | `0x50000201` / `0` | `GUI:GameMap` heading | Yes |
| `0x695` | `(2,355,303,12)` | `STATIC` | `0x50000200` / `0` | `GUI:Blank` status/help strip | Yes |
| `0x468` | `(428,23,96,69)` | `STATIC` | `0x50000004` / `0x20` | preview placeholder | Yes |

Important tiny details:

- The preview static is local X `428`, not setup `0x102`'s similar right-panel preview X. Active in YR: Yes; evidence: PE resource `0x6B`.
- The title static `0x694` is a small right-panel title at `(425,1,108,10)`, not a full-dialog title banner. Active in YR: Yes; evidence: PE resource `0x6B`.
- The bottom status/help static `0x695` is `(2,355,303,12)`, not a full-width footer. Active in YR: Yes; evidence: PE resource `0x6B`.
- The two heading statics and select-engagement text have id `-1`; they are not stateful controls. Active in YR: Yes; evidence: PE resource `0x6B`.
- Both listboxes have style `0x50000151`, which includes owner-draw fixed/no-integral-height style and does not set `LBS_SORT`. Active in YR: Yes; evidence: PE resource `0x6B`; item-data use verified by `0x005E7160`.

## 3. Parent Hide, Modal Entry, And Return

The live offline setup command path hides setup before the chooser is created. In `0x006ACEE0`, the `param_2 == 0x5AA` branch saves `DAT_00A8B250` and `DAT_00A8B254`, copies the current path/display buffer, calls `0x00608070`, pushes `0` to `ShowWindow(setup,0)`, calls `0x005E68A0`, then compares the return value to `2`.

Active in YR: Yes. Evidence: decompile `0x006ACEE0`; assembly context `0x006AD8E7` saves selected token, `0x006AD931` calls `0x00608070`, `0x006AD93C..0x006AD93F` hides the setup HWND, `0x006AD947` calls `0x005E68A0`, and `0x006AD94C` compares return with `2`.

`0x005E68A0` is the chooser wrapper. It calls `0x0072D120`, creates dialog id `0x6B` with callback bytes at `0x005E6920`, stores the HWND in `DAT_00AC0D40`, runs common shell setup `0x00622820`, sends message `0x4A9`, shows the chooser with `ShowWindow(chooser,1)`, pumps the modal loop through `0x007759E0`, then calls `0x0072D170` cleanup and returns the modal result.

Active in YR: Yes. Evidence: decompile `0x005E68A0`; assembly context `0x005E68B7 MOV EDX,0x6B`, `0x005E68BE PUSH 0x5E6920`, `0x005E68C4 CALL 0x00775700`, `0x005E68D0` stores `DAT_00AC0D40`, `0x005E68D5 CALL 0x00622820`, `0x005E68E3` sends `0x4A9`, `0x005E68F5..0x005E68F8` shows the chooser, `0x005E690F` pumps, and `0x005E6916` cleans up.

On cancel result `2`, parent setup restores old selected globals, refreshes/reloads the old selection path, and shows setup with `ShowWindow(setup,5)`. On accept result not equal to `2`, setup rebuilds player/combo state, shows setup with `ShowWindow(setup,5)`, loads selected record state, and restores old globals only if the selected-record load fails.

Active in YR: Yes. Evidence: decompile `0x006ACEE0`; assembly context `0x006AD94C..0x006AD961` restores cancel globals, `0x006AD973..0x006AD976` shows setup after cancel, and `0x006ADA72..0x006ADA7D` shows setup then loads selected record on accepted return.

## 4. Modal Asset / Chrome Binding

Dialog `0x6B` uses the `MnScrnLCustomizeBattle` shell asset path. `0x0072D120` loads the SHP pointer into `DAT_00B0FAB8` only if `g_ScreenWidth == 800`; it then loads the palette/convert state through the pointer at `0x00844D68` regardless of that exact-width branch. `0x0072D170` frees/clears `DAT_00B0FAB8`, `DAT_00B0FCD0`, `DAT_00B0FCD4`, and the load-state byte after the modal exits. `0x0072D210` returns `DAT_00B0FCD4`.

Active in YR: Yes for dialog `0x6B`; SHP load conditional on exact width `800`. Evidence: decompile `0x0072D120`, `0x0072D170`, `0x0072D210`; assembly context `0x0072D129 CMP [g_ScreenWidth],0x320`, `0x0072D135` loads SHP string pointer table entry, `0x0072D140` calls the SHP loader, `0x0072D145` writes `DAT_00B0FAB8`, `0x0072D14A..0x0072D15A` loads palette/convert state.

`0x0060CF00` binds this modal asset state to the shell parent record for dialog id `0x6B`: field `+0x74` receives `0x0072D210()` / `DAT_00B0FCD4`, field `+0xE0` receives `DAT_00B0FB50`, and field `+0xE4` receives `DAT_00B0FAB8`. The setup-shell `0x102` branch is different: it calls `0x0072D030`, still uses `DAT_00B0FB50` at `+0xE0`, but writes `DAT_00B0FA18` at `+0xE4`.

Active in YR: Yes. Evidence: decompile `0x0060CF00`; assembly context `0x0060D015 CMP EAX,0x6B`, `0x0060D01A CALL 0x0072D210`, `0x0060D01F` writes `+0x74`, `0x0060D022..0x0060D033` writes `DAT_00B0FB50` and `DAT_00B0FAB8`; setup comparison `0x0060D03C..0x0060D2AE` calls `0x0072D030` and writes `DAT_00B0FA18`.

Implementation meaning: `MnScrnLCustomizeBattle.*` is verified for Choose Map modal `0x6B`, but it is not the base setup `0x102` background. Treat it as modal-specific chrome.

## 5. Common Shell Setup And Owner-Draw Controls

`0x00622820` applies the common shell setup to the chooser. It enumerates children through owner-draw/subclass helpers, calls `0x0060CF00` unless the alternate gate `0x0069BBE0` blocks that path, marks shell flags, calls `0x0060C540`, and if the dialog is in the fullscreen-shell set, moves the parent HWND to `(0,0,g_ScreenWidth,g_ScreenHeight)` and enumerates children through `ResizeShellChildControl_0060C0C0`.

Active in YR: Yes for `0x6B`. Evidence: decompile `0x00622820`; decompile `0x0060C540` includes `iVar1 == 0x6B` in the fullscreen-shell set and sets shell flags; `0x00622820` then calls `MoveWindow(parent,0,0,g_ScreenWidth,g_ScreenHeight,0)` and resizes children.

`0x0060F9A0` maps real Windows classes to owner-draw callbacks. `"ListBox"` maps to `OwnerDraw_ListBox_00618D40`; `"Button"` with `(style & 0x0B) == 0x0B` maps to `OwnerDraw_Button_00612B70`; `"Static"` maps to `OwnerDraw_Static_006153E0`.

Active in YR: Yes. Evidence: decompile `0x0060F9A0`; assembly context `0x0060FC18..0x0060FC29` selects `OwnerDraw_ListBox_00618D40`, and `0x0060FE58..0x0060FE78` reaches the button-style tests for `OwnerDraw_Button_00612B70`.

Implementation meaning: the two chooser lists are not combo dropdowns, and the right-column buttons are not bespoke rectangles. They are normal shell owner-draw controls after resource creation.

## 6. Button Commands And Return Behavior

The callback bytes at `0x005E6920` could not be decompiled as a function in this read-only session because no Ghidra function boundary exists there. The relevant command dispatch was verified by assembly context.

Active in YR: Yes; `0x005E68A0` passes `0x005E6920` as the live callback.

| Command | Verified behavior | Evidence | Active in YR |
|---:|---|---|---|
| `0x6C5` Use Map | Branch calls `0x005E7160`; that helper reads selected map list item data and closes with result `1`. | Assembly `0x005E69C2 CMP EAX,0x6C5`, `0x005E69CD JZ 0x005E6B63`, `0x005E6B67 CALL 0x005E7160`; decompile `0x005E7160`; assembly `0x005E73A8 MOV EDX,0x1`, `0x005E73AD CALL 0x007757E0`. | Yes, when clicked |
| `0x5C0` Cancel | Branch closes the modal with result `2`, causing parent restore path. | Assembly `0x005E69D3 SUB EAX,0x583`, `0x005E69DA SUB EAX,0x3D`, then `0x005E69E7 MOV EDX,0x2`, `0x005E69EC CALL 0x007757E0`; parent compare at `0x006AD94C`. | Yes, when clicked |
| `0x583` Create Random Map | Branch is real and separate: it hides the chooser, calls `0x005E8590`, then continues with list/selection handling. Downstream generator visuals are outside this report. | Assembly `0x005E69D3 SUB EAX,0x583`, `0x005E69D8 JZ 0x005E69FD`; `0x005E69FD..0x005E6A18` calls `0x00608070`, `ShowWindow(chooser,0)`, and `0x005E8590`. | Conditional, when clicked |

`0x005E7160` reads `0x553` with `LB_GETCURSEL (0x188)` and `LB_GETITEMDATA (0x199)`, resolves item data back through `DAT_00A8B8CC`, reads selected mode/category item data from `0x6EB`, writes `DAT_00A8B23C`, `DAT_00A8B254`, and `DAT_00A8B250`, then closes the modal with result `1`.

Active in YR: Yes. Evidence: decompile `0x005E7160`; assembly context `0x005E7367` writes `DAT_00A8B23C`, `0x005E7370` writes `DAT_00A8B254`, `0x005E7376` writes `DAT_00A8B250`, and `0x005E73A8..0x005E73AD` closes with result `1`.

`0x007757E0` stores the modal result in `DAT_00B72F4C`; `0x007759E0` returns that value when the modal leaves the modal stack and also invalidates/updates the prior modal if one remains underneath.

Active in YR: Yes. Evidence: decompile `0x007757E0` and `0x007759E0`.

## 7. Current Rust Implementation Status

This section is implementation contrast, not binary evidence.

| Rust surface | Current status | Evidence |
|---|---|---|
| App route | `ChooseMap` now opens modal state; older no-op/cycle-only claims are stale. | `src/app.rs:617`, `src/app.rs:626..636` |
| Modal state | Exists: saved selection, selected mode, filtered record indices, highlighted row, top indices, accept/cancel/random-map helper. | `src/ui/skirmish_shell/state.rs:127..210` |
| Parent input blocking | Partial: mouse-up/move/wheel on setup are blocked while modal exists; modal mouse-down is handled separately. | `src/app.rs:693..755`, `src/app.rs:797..850` |
| Modal layout | Partially correct: dialog and listboxes match `0x6B`; buttons/title/preview do not. | `src/ui/skirmish_shell/layout.rs:518..532`, tests at `layout.rs:856..870` |
| Modal rendering | Primitive: solid rect background, solid/listbox panels, primitive button rects, text rows. | `src/app_skirmish_shell_render.rs:1115..1175`, `src/app_skirmish_shell_render.rs:1893..1985` |
| Parent visual composition | Mismatch: render still builds setup instances/text first, then modal overlay, instead of making `0x6B` a replacement shell view while setup is hidden. | `src/app_skirmish_shell_render.rs:2156..2218` |
| Modal assets | Missing: atlas packs setup backgrounds only; `MnScrnLCustomizeBattle.shp` remains a research candidate. | `src/render/skirmish_shell_chrome.rs:166..173`, `src/render/skirmish_shell_chrome.rs:794..795` |
| Create Random Map | Command recognized but log-only in player flow. | `src/app.rs:717..720` |

Current Rust specific mismatches against `0x6B`:

- Buttons are local `(374,80,112,30)`, `(374,116,112,30)`, `(374,152,112,30)` instead of Use `(425,122,108,23)`, Create Random `(425,149,108,23)`, Cancel `(425,346,108,23)`.
- Title is local `(0,20,533,24)` instead of `0x694=(425,1,108,10)`.
- Preview is local `(374,202,128,96)` instead of `0x468=(428,23,96,69)`.
- Layout has no fields for select-engagement heading, GameType/GameMap heading statics, or status strip `0x695`.
- Renderer draws the existing setup shell and then the primitive modal; gamemd hides setup before showing modal `0x6B`.
- Renderer repositions preview drawing to the modal preview rect while modal is open; the verified chooser preview boundary is that passive row highlight/category rebuild does not refresh the preview, while Use Map/parent return updates the setup preview.

## 8. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Target question/non-goals/evidence/stop conditions | verified | Section 0 | none |
| Existing report reconciliation | verified | prior reports listed in Sources | none for this slice |
| PE `RT_DIALOG 0x6B` geometry | verified | fresh resource parse from retail `gamemd.exe` | none |
| Parent `0x102` `0x5AA` branch | verified | `0x006ACEE0` decompile plus `0x006AD93C..0x006AD94C` context | none |
| Modal wrapper `0x005E68A0` | verified | decompile plus `0x005E68B7..0x005E6916` context | none |
| Callback `0x005E6920` full function body | touched-not-exhausted | assembly command contexts; no function boundary | full callback decompile requires boundary repair or a disassembly-only report |
| `0x6B` asset load/cleanup | verified | `0x0072D120`, `0x0072D170`, `0x0072D210`; `0x0072D129..0x0072D15A` context | runtime `>800` pixels |
| `0x6B` vs `0x102` asset binding | verified | `0x0060CF00`; `0x0060D015..0x0060D033`, `0x0060D03C..0x0060D2AE` context | none |
| Common shell setup/fullscreen parent move | verified | `0x00622820`, `0x0060C540` decompile | exact per-child resize math is outside this slice |
| Owner-draw control classification | verified | `0x0060F9A0`; `0x0060FC18`, `0x0060FE58` context | exact row pixels covered by sibling listbox report |
| Use/Cancel/Create Random command boundary | verified | callback assembly contexts and `0x005E7160` decompile | downstream RMG visuals out of scope |
| Current Rust contrast | verified | `src/app.rs`, `src/ui/skirmish_shell/layout.rs`, `src/ui/skirmish_shell/state.rs`, `src/app_skirmish_shell_render.rs`, `src/render/skirmish_shell_chrome.rs` | implementation |

## 9. Open Questions - Final State

- `[RESOLVED] OQ-1 - Is dialog 0x6B active in standard YR offline Skirmish? -> Yes; setup command 0x5AA calls wrapper 0x005E68A0, which creates dialog id 0x6B.` (evidence: `0x006ACEE0`, `0x005E68A0`, assembly `0x006AD947`, `0x005E68B7..0x005E68C4`)
- `[RESOLVED] OQ-2 - Does setup remain visible underneath the modal in gamemd? -> No; setup is hidden with ShowWindow(...,0) before modal entry and shown with ShowWindow(...,5) after return.` (evidence: `0x006AD93C..0x006AD94C`, `0x006AD973..0x006AD976`, `0x006ADA72..0x006ADA75`)
- `[RESOLVED] OQ-3 - What exact resource rect/control inventory does 0x6B use? -> DIALOGEX 533x369 with 11 controls listed in Section 2.` (evidence: fresh PE `RT_DIALOG 0x6B` parse; wrapper creation evidence `0x005E68B7..0x005E68C4`)
- `[RESOLVED] OQ-4 - Is MnScrnLCustomizeBattle verified for this modal? -> Yes for 0x6B; SHP load at exact width 800, PAL/convert load through the same modal setup function.` (evidence: `0x0072D120`, assembly `0x0072D129..0x0072D15A`)
- `[RESOLVED] OQ-5 - Does 0x6B share setup 0x102's background binding? -> No; 0x6B writes DAT_00B0FAB8, while 0x102 writes DAT_00B0FA18.` (evidence: `0x0060CF00`, assembly `0x0060D015..0x0060D033`, `0x0060D03C..0x0060D2AE`)
- `[RESOLVED] OQ-6 - Does 0x6B use common fullscreen shell setup? -> Yes; 0x6B is in the 0x0060C540 fullscreen set used by 0x00622820.` (evidence: `0x00622820`, `0x0060C540`)
- `[RESOLVED] OQ-7 - Are 0x6EB and 0x553 combos? -> No; resource class is LISTBOX and 0x005E7160 uses LB_GETCURSEL/LB_GETITEMDATA.` (evidence: PE resource, `0x005E7160`, `0x0060FC18..0x0060FC29`)
- `[RESOLVED] OQ-8 - Are modal buttons shell owner-draw buttons? -> Yes; BUTTON style low bits route through OwnerDraw_Button_00612B70.` (evidence: PE resource style `0x5000000B`, `0x0060F9A0`, `0x0060FE58..0x0060FE78`)
- `[RESOLVED] OQ-9 - Is Create Random Map just a decorative button? -> No; command 0x583 has a live branch that hides the chooser and calls 0x005E8590.` (evidence: `0x005E69D3..0x005E6A18`)
- `[RESOLVED] OQ-10 - Is current Rust still a no-modal no-op? -> No; current Rust opens and renders primitive ChooseMapModalState.` (evidence: `src/app.rs:617..636`, `src/app_skirmish_shell_render.rs:1115..1175`)
- `[RESOLVED] OQ-11 - Does current Rust match modal shell composition? -> No; it draws setup under a primitive overlay and lacks modal asset/resource rect parity.` (evidence: `src/app_skirmish_shell_render.rs:2156..2218`, `src/ui/skirmish_shell/layout.rs:518..532`)
- `[DEFERRED] OQ-12 - Full callback 0x005E6920 decompile.` (category: `requires-different-system-context`; reason: no function boundary exists and Ghidra mutation is forbidden in this swarm; next-step-if-pursued: run a disassembly-only callback slice or approve boundary creation outside this swarm)
- `[DEFERRED] OQ-13 - Native RGB screenshot validation for 800 and >800 first frame.` (category: `needs-runtime-debugger`; reason: this slot did not capture native screenshots; next-step-if-pursued: run native screenshot trace at 800x600 and 1024x768)
- `[DEFERRED] OQ-14 - Downstream Create Random Map UI/generator visuals.` (category: `out-of-scope`; reason: only modal composition and command boundary were in scope; next-step-if-pursued: use existing RMG reports)

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Proposed test name | Risk / do-not-do |
|---|---|---|---|---|---|---|---|
| Choose Map is a separate shell modal: setup hides, `0x6B` is shown/pumped, setup returns after result. | `0x006ACEE0`, `0x005E68A0`; `0x006AD93C..0x006AD94C`, `0x005E68B7..0x005E6916` | partial/mismatch: Rust modal opens but setup shell is still rendered underneath | `src/app_skirmish_shell_render.rs`, `src/app.rs` | While `choose_map_modal` is active, compose the `0x6B` shell as the visible shell view and suppress setup controls/background/text behind it; keep parent input blocked. | Click Choose Map: setup controls disappear/are not visible behind chooser; Cancel returns unchanged. | `choose_map_modal_replaces_parent_setup_shell_until_result` | Do not keep the setup shell as a visible backdrop under the chooser. |
| Dialog `0x6B` resource rects define the modal composition: `533x369`, lists `(77,78,130,211)` and `(225,78,130,211)`, buttons Use `(425,122,108,23)`, Create `(425,149,108,23)`, Cancel `(425,346,108,23)`, title `(425,1,108,10)`, preview `(428,23,96,69)`. | Fresh PE `RT_DIALOG 0x6B` parse; wrapper creation at `0x005E68B7..0x005E68C4` | mismatch: Rust only matches dialog/listboxes; button/title/preview rects and static/status fields are wrong or absent | `src/ui/skirmish_shell/layout.rs`, `src/app_skirmish_shell_render.rs`, hit tests in `src/app.rs` | Align render and hit-test geometry to all `0x6B` controls, including headings and status strip. | Bottom-right Cancel closes; old upper-right Cancel position no longer hit-tests; title/preview appear in right-panel resource positions. | `choose_map_modal_uses_all_resource_0x6b_control_rects` | Do not preserve current hand-entered button/title/preview rects. |
| `0x6B` uses `MnScrnLCustomizeBattle` chrome/palette binding, distinct from `0x102`; SHP load is exact-width 800. | `0x0072D120`, `0x0060CF00`; `0x0072D129..0x0072D15A`, `0x0060D015..0x0060D033`, `0x0060D294..0x0060D2AE` | missing: atlas does not pack modal asset; renderer uses solid colors | `src/render/skirmish_shell_chrome.rs`, `src/app_skirmish_shell_render.rs` | Add modal-specific background/palette asset path and draw it for `0x6B`, preserving distinct setup `0x102` background. | 800x600 chooser background uses Customize Battle art, not solid fill or setup art. | `choose_map_modal_uses_customize_battle_chrome_at_800` | Do not reuse `MnScrnLCoopGameSetup` for the chooser; do not use `MnScrnLCustomizeBattle` as base setup art. |
| `0x6EB` and `0x553` are owner-drawn listboxes; selection is item-data-backed and accept reads `LB_GETCURSEL`/`LB_GETITEMDATA`. | PE resource; `0x0060F9A0`; `0x005E7160`; `0x0060FC18..0x0060FC29` | partial: state/list text exists, visual owner-draw listbox parity incomplete | `src/app_skirmish_shell_render.rs`, `src/ui/skirmish_shell/state.rs` | Render fixed owner-draw listboxes with verified row styling/scrollbar behavior from sibling row-paint report; keep item-data/stable record identity. | Selecting a mode rebuilds map rows; selecting a map highlights but does not commit until Use Map. | `choose_map_modal_listboxes_are_ownerdraw_and_item_data_backed` | Do not model these controls as combo dropdowns or sorted display-name lists. |
| Use/Cancel/Create Random have real modal command boundaries: Use closes result `1`, Cancel closes result `2`, Create Random hides chooser and enters `0x005E8590` flow. | `0x005E69C2..0x005E6A18`, `0x005E7160`, `0x007757E0`, `0x007759E0` | partial: Use/Cancel exist; Create Random is log-only; accept lacks full parent return/load-failure order | `src/app.rs`, modal state, preview/session refresh surfaces | Preserve transient highlight until Use; Cancel restores saved selection; wire `0x583` to verified RMG flow or an explicit blocked modal state, not a silent log. | Highlight row then Cancel leaves setup selection/preview unchanged; Use commits after close; Create Random has visible behavior. | `choose_map_modal_result_codes_restore_commit_and_random_command_boundary` | Do not commit map/preview from passive highlight alone; do not leave `0x583` invisible in player builds. |
| Passive browsing inside `0x6B` should not be treated as setup preview replacement; normal preview replacement happens after Use/parent return. | [SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH_GHIDRA_REPORT.md); parent return in `0x006ACEE0` | partial/mismatch risk: renderer redirects current preview into modal preview rect while modal is active | `src/app_skirmish_shell_render.rs`, preview cache/session state | Draw the modal preview/static area according to `0x6B` without treating row highlight/category rebuild as committed setup preview refresh. | Highlight several maps: committed setup preview remains old until Use Map return path. | `choose_map_modal_highlight_does_not_commit_preview_until_use_map` | Do not make chooser row hover/highlight a committed preview reload. |

## 11. Negative Facts / Do Not Do

- Do not describe current Rust as having no Choose Map modal at all. Current Rust now opens `ChooseMapModalState` and draws a primitive modal. Active in YR: n/a for Rust status; evidence `src/app.rs:617..636`, `src/app_skirmish_shell_render.rs:1115..1175`.
- Do not call the current primitive modal parity-complete. Active in YR: No; gamemd hides setup, creates `0x6B`, binds `MnScrnLCustomizeBattle`, and owner-draws resource controls. Evidence: `0x006ACEE0`, `0x005E68A0`, `0x0060CF00`, PE resource `0x6B`.
- Do not use `MnScrnLCoopGameSetup.*` or setup `0x102` background binding as the chooser background. Active in YR: No; evidence `0x0060CF00` separates `0x6B` and `0x102`.
- Do not treat `MnScrnLCustomizeBattle.*` as merely an unverified candidate. Active in YR: Yes for chooser `0x6B`; evidence `0x0072D120`, `0x0060CF00`.
- Do not model `0x6EB` or `0x553` as combo dropdowns. Active in YR: No; evidence PE resource class `LISTBOX`, `0x0060F9A0`, `0x005E7160`.
- Do not put Cancel in the upper-right area. Active in YR: No; resource Cancel is `(425,346,108,23)`, near the bottom right.
- Do not leave setup controls visible/interactable behind the chooser. Active in YR: No; parent setup is hidden before modal creation.
- Do not make passive row highlight the committed selected map/preview. Active in YR: No for normal browsing per sibling preview-refresh report and parent return contract.

## 12. Stale Docs / Follow-up Docs

- [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_ACTION_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_ACTION_TRACE.md): replace Rust status claims that Choose Map still cycles `selected_map_idx` in-place with: "STALE as of 2026-05-23 current Rust. `ChooseMap` now bubbles to `src/app.rs`, opens `ChooseMapModalState`, handles modal list/button clicks, and renders a primitive modal. Remaining mismatch: Rust still draws setup underneath, lacks `MnScrnLCustomizeBattle` chrome, has non-resource modal button/title/preview rects, leaves Create Random Map log-only, and does not fully reproduce parent return/load-failure preview refresh ordering."
- [docs/research/traces/SKIRMISH_CHOOSE_MAP_MODAL_FIRST_PAINT_VISUAL_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/SKIRMISH_CHOOSE_MAP_MODAL_FIRST_PAINT_VISUAL_TRACE.md): keep the existing 2026-05-23 correction, but replace any remaining lines saying "no branch calls `compute_choose_map_modal_layout`" or "modal text/list rows are absent" with: "STALE as of current Rust. The renderer now computes `choose_map_layout`, draws primitive modal/list/button rectangles, and pushes modal text rows. The current first-paint mismatch is composition and asset/resource parity, not total absence of modal rendering."
- [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_0X6B_VISUAL_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_0X6B_VISUAL_INTEGRATION_GHIDRA_REPORT.md): replace "app routing swallows `ChooseMap`" and "modal render path missing" with: "STALE as of current Rust. App routing opens `ChooseMapModalState` and the renderer draws a primitive modal overlay; remaining deltas are hidden-parent shell composition, modal asset binding, resource rect completeness, owner-draw list/button parity, and Create Random Map flow."

## 13. Remaining Uncertainty

- Full `0x005E6920` callback decompile remains unavailable in this read-only swarm because no function boundary exists there and boundary creation is forbidden. The command branches needed for this report were verified through assembly context.
- Exact native first-frame RGB at 800x600 and the `>800` fallback require runtime screenshot validation. Binary evidence verifies the exact-width `800` SHP load and fullscreen-shell participation, but not final pixels at larger widths.
- Downstream Create Random Map UI/generator composition after `0x005E8590` remains delegated to existing RMG reports.
- Per-child resize math inside `ResizeShellChildControl_0060C0C0` was not reopened; this report only claims `0x6B` enters the common fullscreen shell path and that resource local rects are the source layout.

## Sources

- Fresh read-only Ghidra decompile: `0x006ACEE0`, `0x005E68A0`, `0x0060CF00`, `0x0072D120`, `0x0072D170`, `0x0072D210`, `0x00622820`, `0x0060C540`, `0x0060F9A0`, `0x005E7160`, `0x007757E0`, `0x007759E0`.
- Fresh read-only Ghidra assembly/context: `0x006AD8E7`, `0x006AD931..0x006AD94C`, `0x006AD973`, `0x006ADA72`, `0x005E68B7..0x005E6916`, `0x0072D129..0x0072D15A`, `0x0060D015..0x0060D033`, `0x0060D03C..0x0060D2AE`, `0x0060FC18..0x0060FC29`, `0x0060FE58..0x0060FE78`, `0x005E69C2..0x005E6A18`, `0x005E6B63..0x005E6B67`, `0x005E7367..0x005E73AD`.
- Fresh binary resource parse: `<ra2-install>/gamemd.exe` `RT_DIALOG 0x6B`, lang `0x409`, RVA `0x7EE6D8`, file offset `0x4F26D8`, size `636`.
- Existing docs reconciled: [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_CURRENT_MODAL_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_CURRENT_MODAL_RECHECK_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_0X6B_VISUAL_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_0X6B_VISUAL_INTEGRATION_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_VISUAL_CONTROL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_VISUAL_CONTROL_LAYOUT_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_FLOW_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_FLOW_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_RETURN_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_RETURN_CONTRACT_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_ACTION_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_ACTION_TRACE.md), [docs/research/traces/SKIRMISH_CHOOSE_MAP_MODAL_FIRST_PAINT_VISUAL_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/SKIRMISH_CHOOSE_MAP_MODAL_FIRST_PAINT_VISUAL_TRACE.md), [docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_0X6B_PREVIEW_REFRESH_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_LISTBOX_00618D40_ROW_PAINT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_LISTBOX_00618D40_ROW_PAINT_GHIDRA_REPORT.md).
- Current Rust scan: `src/app.rs`, `src/ui/skirmish_shell/layout.rs`, `src/ui/skirmish_shell/state.rs`, `src/ui/skirmish_shell/mod.rs`, `src/app_skirmish_shell_render.rs`, `src/render/skirmish_shell_chrome.rs`.
