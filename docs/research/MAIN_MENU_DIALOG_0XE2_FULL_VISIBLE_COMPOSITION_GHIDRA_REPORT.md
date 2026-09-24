# Main Menu Dialog 0xE2 Full Visible Composition - Ghidra Report

Date: 2026-05-17

Scope: deeper investigation of the standard Yuri's Revenge initial main menu
dialog `0xE2`, focused on the complete visible composition: parent paint,
background assets, all child controls, static behavior, text, layout, failure
paths, and TS-legacy traps.

Parent reports:

- [MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md)
- [MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md)
- [MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md)
- [MAIN_MENU_DIALOG_0XE2_OWNERDRAW_VISUAL_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_DIALOG_0XE2_OWNERDRAW_VISUAL_FOLLOWUP_GHIDRA_REPORT.md)
- [BITFONT_SHELL_TEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md)
- [SKIRMISH_OWNERDRAW_CALLBACKS_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_CALLBACKS_FOLLOWUP_GHIDRA_REPORT.md)
- [SKIRMISH_SHELL_RIGHT_PANEL_BACKGROUND_PALETTE_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SHELL_RIGHT_PANEL_BACKGROUND_PALETTE_FOLLOWUP_GHIDRA_REPORT.md)

No Rust code was modified.

## Executive Summary

The previous "movie plus buttons" model was incomplete. Dialog `0xE2` is in the
common full-shell-background dialog set. Its parent `WM_PAINT` path draws the
generic shell parent background plus the right-panel SHP stack before the main
menu dialog proc sends the explicit RA2TS movie draw message to child `0x71A`.

Verified live composition for standard YR:

1. `FUN_00531CC0` creates RT_DIALOG `0xE2` with dialog proc `0x00531F60`.
2. `FUN_00622B50` handles `WM_INITDIALOG`, subclasses children through
   `FUN_0060F9A0`, and marks dialog `0xE2` as common paint mode `1`.
3. Parent `WM_PAINT_Handler @ 0x00621E90` draws:
   - common right-panel stack through `RightPanel__Draw @ 0x0072E450`;
   - generic parent shell background through `Background_Overlay @ 0x0072E730`;
   - then blits the composed offscreen shell surface to the main surface.
4. Dialog proc `0x00531F60` then handles `WM_PAINT` by sending `0x4F0` to child
   static `0x71A`, which draws/copies the current `Ra2ts_s/l` Bink frame.
5. The six right buttons are generic owner-draw PCX buttons, not GraphicMenu
   items.
6. Static `0x695` is the live tooltip/status text sink used by the common shell
   hit-test path. Static `0x71D` is initialized by the dialog proc with a
   bottom-right version/status string. Static `0x71C` has no proven visible
   output in the `0xE2` path.

Overall confidence: High for the active `0xE2` paint/background/control path.
Medium for exact runtime behavior after corrupt Bink object construction because
the decompiler loses some fastcall arguments and allocation initialization
details. Active in YR: Yes.

## Dialog and Child Controls

RT_DIALOG `0xE2` is `DIALOGEX`, style `0x40000040`, rect `0,0,533,369`, font
`MS Sans Serif`, 8 pt. The standard shell base units observed in adjacent dialog
research are `baseX=6`, `baseY=13`, so `533x369` DLU maps to `800x600` px.

| Id | Class | DLU rect | Approx px rect | Title | Visible role |
|---:|---|---|---|---|---|
| `0x694` | Static | `425,1,108,10` | `638,2,162,16` | `GUI:MainMenu` | Centered yellow heading text |
| `0x695` | Static | `2,355,303,12` | `3,577,455,20` | `GUI:Blank` | Common hover tooltip/status text sink |
| `0x71A` | Static | `0,0,304,266` | template `0,0,456,432`; runtime movie size | none | RA2TS Bink movie panel |
| `0x71C` | Static | `447,29,61,33` | `671,47,92,54` | none | Animated SDWRNANM monitor (corrected, see below) |
| `0x71D` | Static | `425,357,108,10` | `638,580,162,16` | `GUI:Blank` | Bottom-right version/status text |
| `0x683` | Button | `425,125,108,23` | `638,203,162,37` | `GUI:SinglePlayer` | Owner-draw PCX button |
| `0x684` | Button | `425,152,108,23` | `638,247,162,37` | `GUI:WWOnline` | Owner-draw PCX button |
| `0x578` | Button | `425,179,108,23` | `638,291,162,37` | `GUI:Network` | Owner-draw PCX button |
| `0x686` | Button | `425,206,108,23` | `638,335,162,37` | `GUI:MoviesAndCredits` | Owner-draw PCX button |
| `0x55C` | Button | `425,233,108,23` | `638,379,162,37` | `GUI:Options` | Owner-draw PCX button |
| `0x3EE` | Button | `425,330,108,23` | `638,536,162,37` | `GUI:ExitGame` | Owner-draw PCX button |

Tiny layout detail: `425` DLU maps to `638` px under Win32 `MulDiv`, not `637`.
Final pixel capture should still verify exact font metrics, but the binary path
does not apply a second per-control scale after Win32 dialog creation.

## Full WM_PAINT Path

### Dialog proc `0x00531F60`

Ghidra initially had no function at `0x00531F60`; this pass created and
decompiled it as `MainMenuDialog0xE2_Proc_00531F60`.

The proc first delegates to `FUN_00622B50`. Only if that returns `0` does it
handle main-menu-specific messages.

For `WM_PAINT` (`0x0F`):

```text
GetDlgItem(hwnd, 0x71A)
SendMessage(child_0x71A, 0x4F0, 0, 0)
```

This is an explicit movie draw/copy request. It does not draw the background or
buttons itself.

For `WM_COMMAND` (`0x111`), it writes return codes through the pointer stored at
`GetWindowLong(hwnd, 8)`:

| Control | Return code |
|---:|---:|
| `0x683` Single Player | `1` |
| `0x684` WW Online | `2` |
| `0x578` Network | `3` |
| `0x686` Movies/Credits | `4` |
| `0x55C` Options | `5` |
| `0x3EE` Exit Game | `6` |

For custom `0x497`, it formats a string using `StringTable__LoadString(...,
0x1757)` and `FUN_00735120()`, then sends the result to child `0x71D` through
static message `0x4B2`.

### Common shell proc `FUN_00622B50`

On `WM_INITDIALOG` (`0x110`), `FUN_00622B50`:

- calls `FUN_0060F4B0`, which internally calls `FUN_0060F9A0` for every child,
  installing the owner-draw window procs (corrected 2026-05-28: was described as
  a direct call to `FUN_0060F9A0`; binary shows `FUN_00622B50` calls
  `FUN_0060F4B0` on first-init path, and `FUN_0060F4B0` calls `FUN_0060F9A0` —
  verified via `decompile_function 0x00622b50` and `decompile_function 0x0060f4b0`;
  ROOT_CAUSE: INFERENCE_HARDENED);
- calls `FUN_0060CF00`, which assigns dialog background/convert pointers;
- calls `FUN_0060C540`, which includes dialog id `0xE2`, writes mode `1`, and
  then `FUN_0060C4A0` expands the dialog window to `g_ScreenWidth` by
  `g_ScreenHeight`.

On parent `WM_PAINT`, `FUN_00622B50` calls `WM_PAINT_Handler @ 0x00621E90`,
then validates the parent rect. If a record byte at `+0xBE` is set, it also
sends `0x4E2` to `0x71A` to stop/destroy the movie handle and clears that byte.

### `WM_PAINT_Handler @ 0x00621E90`

Because `FUN_0060C540` marks `0xE2` as mode `1`, the active paint branch is:

```text
if paint_mode == 1:
    if not in alternate left-panel mode:
        if FUN_0072E260() != 0:
            RightPanel__Draw(record_byte_D4 == 0)
            Background_Overlay(convert, small_parent, large_parent)
            optional top/minimap/radar overlays only if their record bytes are set
```

For `0xE2`, `FUN_0060CAF0`, `FUN_0060C930`, `FUN_0060CCC0`, and
`FUN_0060CDB0` clear the Skirmish/radar/minimap-specific record bytes. So the
standard initial menu gets the common right panel plus parent background, but
not Skirmish preview/radar extras.

The handler composes into an offscreen `BSurface` stored in the dialog record,
then blits that surface to `DAT_00887310`.

## Parent Background and Right Panel Assets

### Dialog `0xE2` background pointer assignment

`FUN_0060CF00` has special cases for Skirmish and many other dialogs. `0xE2` is
not one of those special cases, so it falls into the generic common-shell branch:

```text
record[0x1E] = FUN_0072E280()     ; returns DAT_00B0FBCC
record[0x39] = DAT_00B0FB50       ; MNSCRNS.SHP
record[0x3A] = DAT_00B0FA04       ; MNSCRNL.SHP
```

`FUN_0072E280` returns `DAT_00B0FBCC`, the convert object built from
`SHELL.PAL`. Therefore the generic parent background for `0xE2` is:

| Width | Parent SHP | Convert/palette | Evidence |
|---:|---|---|---|
| `640` | `MNSCRNS.SHP` | `DAT_00B0FBCC` / `SHELL.PAL` | `FUN_0060CF00`, `Background_Overlay`, string table `0x00845144..` |
| non-640 | `MNSCRNL.SHP` | `DAT_00B0FBCC` / `SHELL.PAL` | same |

This differs from Skirmish dialog `0x102`: Skirmish uses `DAT_00B0FA18`
(`MnScrnLCoopGameSetup.shp`) as its non-640 alternate parent with its own
`MnScrnLCoopGameSetup.PAL` convert. Standard `0xE2` does not use that Skirmish
background.

### Right-panel draw stack

`RightPanel__Draw @ 0x0072E450` is shared with other shell dialogs. For `0xE2`
it draws in this order:

| Order | Asset | Frame | Palette/convert |
|---:|---|---:|---|
| 1 | `SDTP.SHP` | `0` | `SHELL.PAL` / `DAT_00B0FBCC` |
| 2 | repeated `SDBTNBKGD.SHP` | `0` | `SHELL2.PAL` / `DAT_00B0FBD4` |
| 3 | repeated `SDBTNANM.SHP` | `10` | `SDBTNANM.PAL` / `DAT_00B0FBDC`; conditional on caller flag |
| 4 | `SDBTM.SHP` | `0` | `SHELL.PAL` / `DAT_00B0FBCC` |
| 5 | `LWSCRNS.SHP` at 640, otherwise `LWSCRNL.SHP` | `0` | `SHELL.PAL` / `DAT_00B0FBCC` |

Loader/string evidence:

- `FUN_0072DFB0` loads the common right-panel SHPs and palette converts.
- String block `0x00845104..0x00845188` contains `LWSCRNL.SHP`,
  `LWSCRNS.SHP`, `SDBTM.SHP`, `SDBTNBKGD.SHP`, `SDTP.SHP`, `MNSCRNL.SHP`,
  `MNSCRNS.SHP`, `SDWRNTMP.SHP`, `SDMPBTN.SHP`, and `SDBTNANM.SHP`.
- Palette string block `0x00845438..` contains `SDBTNANM.PAL`, `SHELL2.PAL`,
  and `SHELL.PAL`; `FUN_0072ADE0` builds 6-bit-left-shifted palette data and a
  `ConvertClass` for the active 16-bit destination.

No evidence was found that `SDMPBTN.SHP`, `SDWRNTMP.SHP`, `dbak6440.pcx`,
`dlgsys*.pcx`, `MAINBTTN.PAL`, `DIALOG.PAL`, `SHELL2.PAL`, or
`SDBTNANM.PAL` are used for the main menu PCX button pieces. The PCX buttons
still use embedded PCX palettes.

## Static Controls

All four static children are subclassed by `FUN_0060F9A0` to
`OwnerDraw_Static_006153E0`, because class dispatch checks `"Static"` before any
style-specific button dispatch.

### `0x71A` RA2TS movie

`FUN_00531CC0` and `FUN_0052B9B0` both perform:

```text
SetWindowPos(0x71A, x, y, -1, -1, 0x0D)
SendMessage(0x71A, 0x4E3, 1, 0)
SendMessage(0x71A, 0x4E4, 0, screen_width == 640 ? "Ra2ts_s" : "Ra2ts_l")
```

Position rule in those functions:

```text
x = 0 if screen_width  < 801 else (screen_width  - 800) / 2
y = 0 if screen_height < 601 else (screen_height - 600) / 2
```

`OwnerDraw_Static_006153E0` handles:

- `0x4E3`: stores loop flag in owner-draw record `+0x5C`; main menu passes `1`;
- `0x4E4`: destroys any previous movie, kills timer `0x65`, constructs a movie
  handle from the base name, resizes the static to movie width/height, and starts
  timer `0x65` with interval `0x22` ms;
- `0x4F0`: calls movie vtable `+0x28`, the explicit copy/draw path;
- `WM_TIMER`, id `0x65`: update/copy ready Bink frames, invalidate if changed,
  then loop with `BinkGoto(frame=1, wait=1)` if the stored loop flag is nonzero.

Asset rule:

| Width | Base name | Retail physical path |
|---:|---|---|
| `640` | `Ra2ts_s` | resolves `.BIK` before `.VQA`; retail uses Bink |
| non-640 | `Ra2ts_l` | resolves `.BIK` before `.VQA`; retail uses Bink |

### `0x694` heading

`0x694` is a normal owner-draw static with title `GUI:MainMenu`. Its low static
style bit is `1`, so the static paint code passes horizontal center plus
vertical-center behavior to `FUN_00621040`.

Color comes from `DAT_00AC18A4 = 0x0000FFFF`, initialized in `FUN_0060F9A0`.
`FUN_00621040` interprets the low/middle/high bytes as RGB, so this is yellow
`#FFFF00`.

### `0x695` tooltip/status line

`FUN_00622B50` handles hit-testing around message `0x84` by:

1. fetching child `0x695`;
2. finding the child under the cursor;
3. sending the hovered child `0x4E8`;
4. resolving tooltip text through `FUN_006040B0`;
5. sending the resulting text to `0x695` via static message `0x4B2`.

`OwnerDraw_Static_006153E0` handles `0x4B2` by updating its backing text and
invalidating the control. Therefore `0x695` is not merely a decorative blank;
it is the bottom-left tooltip/status output area.

### `0x71D` bottom-right status/version line

The dialog proc handles custom `0x497` by formatting a string with
`StringTable__LoadString(..., 0x1757)` plus `FUN_00735120()` and sending it to
child `0x71D` via `0x4B2`. The resource starts as `GUI:Blank`, but the live
initialization path gives it visible text.

The deeper follow-up resolved this string construction:

- `FUN_0074FAE0` builds the version string on the global `VersionClass`
  instance at `0x00A8ECE0` (verified via `inspect_memory_content 0x004E7DC0`
  → trampoline bytes `B9 E0 EC A8 00 E8 …` = `MOV ECX, 0x00A8ECE0; CALL
  0x0074FAE0`). It reads up to 16 bytes from `VERSION.TXT` via raw
  `CreateFileA` into the cache buffer at `this+0x2A` (verified via
  `decompile_function 0x0074FAE0` + `decompile_function 0x0065CBF0`). The
  **`"%d.%3.3dTUC"` format runs unconditionally** — NOT as a fallback. It
  always formats the uint16 pair at `this+0x08` / `this+0x0A` (both default
  to `1`, producing `"1.001TUC"`); the VERSION.TXT bytes go into a separate
  buffer and are parsed by the companion `FUN_0074F760` but are not pasted
  into the version label. See [VERSION_TXT_RESOLUTION_AND_FALLBACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VERSION_TXT_RESOLUTION_AND_FALLBACK_GHIDRA_REPORT.md).
- `FUN_00735120` converts the ASCII version string into a temporary UTF-16
  buffer.
- The format string at `0x00826960` is wide `"%s %s"`.
- The adjacent string at `0x0082696C` is `GUI:Version`; the dialog proc loads
  string-table id `0x1757`, then formats `"<localized GUI:Version> <version>"`.

`0x71D` uses static style low bit `1`, so it is centered vertically and
horizontally inside its rect, with the same default yellow text color unless a
later `0x498` overrides it. No such override was found on the `0xE2` path.

### `0x71C` blank/transparent static

> **Corrected 2026-09-24.** `0x71C` is the animated SDWRNANM warning monitor
> (static kind 4, set up by `0x0060A5B0`, image `0x006038C7`, SHELL2 palette
> `0x00603776`); retail captures of `0xE2` show its frames at `(670, 48)`.
> The conclusion below is superseded; see
> `docs/research/shell/2026-09-24-right-panel-statics-evidence.md`.

Resource `0x71C` is class `Static`, rect `447,29,61,33`, style `0x50000007`,
empty title.

Verified facts:

- `FUN_0060F9A0` routes it to `OwnerDraw_Static_006153E0`, not to the button
  callback.
- On `0x497`, the static callback initializes kind/type `0`, text color
  `DAT_00AC18A4`, and no text for an empty title.
- On `WM_PAINT`, if no movie/image/SHP/text state is attached, the owner-draw
  static validates the rect after copying/restoring the background surface. It
  does not call the original Win32 static proc to draw an `SS_BLACKFRAME` or
  `SS_ETCHEDFRAME` primitive.
- `FUN_00531CC0`, `FUN_0052B9B0`, and dialog proc `0x00531F60` never call
  `GetDlgItem(0x71C)` and never send it image/movie/color/custom messages.
- A raw immediate scan of `gamemd.exe` found one `push 0x71C` in code:
  `ToggleMpScoreControls_0046DE20`, a helper that show/hides multiplayer score
  controls (`0x732`, `0x72F`, `0x6D1`, `0x5A8`, `0x468`, etc.) and inverts
  `0x71C` visibility relative to its `param_2`. No direct caller or main-menu
  link was found for that helper in this pass.

Conclusion: in standard `0xE2`, `0x71C` has no verified visible output. It is
not the website tooltip control: `FUN_006040B0` maps `STT:MainButtonYuriWebSite`
to control `0x55F`, not `0x71C`. No `push 0x55F` GetDlgItem/ShowWindow-style
use was found in the binary scan. Control `0x71B` appears in other shell/score
contexts, not in `0xE2` setup.

Confidence: High for no direct `0xE2` setup/use; Medium for absolute invisibility
because an indirect call to the score-control helper cannot be ruled out from
static xrefs alone, though the surrounding IDs make it a non-main-menu helper.

## Button Rendering and Text

> **CORRECTION (2026-05-30):** This section describes the **type-0 `bue/bde` PCX** branch
> of `OwnerDraw_Button_00612B70`, which is the generic owner-draw default but **NOT** what
> dialog `0xE2` actually paints. The five non-Exit `0xE2` buttons are resized to the
> `SDBTNANM.SHP` cell (156×42) by `FUN_0060B000` and painted with SDBTNANM frames 2/3/4
> (the type-1 branch); the button window equals the SDBTNANM frame rect (x=644). The Exit
> button `0x3EE` is special-cased (`FUN_00608CD0` false → not resized, raw ≈162×37 @
> x=638). The `bue/bde` PCX pieces are preloaded but unused on this path. Evidence:
> `decompile_function 0x00608CD0` / `0x0060B000` / `0x00612B70` / `0x0060F9A0`. Full
> analysis: [MAIN_MENU_0XE2_BUTTON_PAINT_AND_REPOSITION_FORK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_0XE2_BUTTON_PAINT_AND_REPOSITION_FORK_GHIDRA_REPORT.md).

All six right-side buttons have class `Button`, style `0x5000000B`. `FUN_0060F9A0`
routes those to `OwnerDraw_Button_00612B70`.

The normal PCX path formats:

| Piece | Format string |
|---|---|
| left | `b%c%c_li%d.pcx` at `0x0083589C` |
| middle | `b%c%c_mi%d.pcx` at `0x0083588C` |
| right | `b%c%c_ri%d.pcx` at `0x0083587C` |

For enabled controls:

- first `%c` is `'u'` for unpressed or `'d'` for pressed;
- second `%c` is literal `'e'`;
- height family is selected from `24` and `30`; `37` px client height selects
  `30`;
- the selected 30 px art strip is vertically centered in the 37 px client;
- pressed state offsets the art and text from the normal rect;
- the middle PCX is tiled by `FUN_006BA3E0`, not stretched;
- missing PCX lookup has no primitive GDI fallback and the button paint path
  dereferences returned surfaces.

Standard main-menu button art:

| State | Left | Middle | Right |
|---|---|---|---|
| Up | `bue_li30.pcx` | `bue_mi30.pcx` | `bue_ri30.pcx` |
| Down | `bde_li30.pcx` | `bde_mi30.pcx` | `bde_ri30.pcx` |

Text:

- owner-draw button paint calls `FUN_00621040(..., color, 5, 0x0C, 0, 0, 0)`;
- align flag `5` means horizontal center plus vertical center in the BitFont
  wrapper;
- color is normally `DAT_00AC18A4 = 0x0000FFFF`, which `FUN_00621040` converts
  as RGB `#FFFF00`;
- disabled controls force up art then apply `AlphaBlendRect(..., 0x80)` black
  blend; disabled text color paths exist through common globals, but no standard
  `0xE2` disabled button was proven in this pass.

Mouse sound:

- `WM_LBUTTONDOWN` (`0x201`) and `WM_LBUTTONDBLCLK` (`0x203`) play the main GUI
  button sound unless the control is disabled;
- prior sound research maps this to `[AudioVisual] GUIMainButtonSound`, default
  `MenuClick`.

## Layout and Resolution Behavior

Dialog/resource layout:

- RT_DIALOG `0xE2` maps to an `800x600` shell client under standard dialog base
  units.
- `FUN_0060C4A0` then moves the parent dialog window to `0,0,g_ScreenWidth,
  g_ScreenHeight` for dialogs included by `FUN_0060C540`, including `0xE2`.
- `CenterChildWindow @ 0x00777080` is called after dialog creation. Because the
  common `WM_INITDIALOG` path has already expanded this dialog to the screen
  client, centering normally resolves back to `(0,0)`.
- Child controls are not uniformly scaled to the full screen, but they are not
  all left at raw DLU positions either. `ResizeShellChildControl_0060C0C0 @
  0x0060C0C0` applies targeted moves and pixel nudges during the fullscreen
  child-enumeration pass.

### Child reposition pass

`FUN_0060C4A0` enumerates children with `ResizeShellChildControl_0060C0C0`.
For dialog `0xE2`, this creates three important corrections to raw
DLU-derived positions:

| Control | Helper | Verified behavior |
|---:|---|---|
| `0x695` | `FUN_0060B550` | Bottom-left status line is anchored at `x = max((screen_w - 800) / 2, 0) + 10`, `y = screen_h - control_h - max((screen_h - 600) / 2, 0) - 1`. |
| `0x71D` | `FUN_0060B610` | Bottom-right status/version line is anchored to the bottom of the right-panel bottom-cap rect (`DAT_00B0FC28` in normal shell mode), not just to its resource DLU `y`. |
| `0x694` | `FUN_0060B950` | Main heading static receives the common heading nudge for dialog `0xE2`: top moves down by `+7` px and height increases by `+1` px in normal shell mode. |

For `0x71D`, `FUN_0060B610` uses `DAT_007F5BF8 = 168` as the
right-panel/sidebar width constant. If the control has no owner-draw override
width slot, it computes an inset `(168 - control_width) / 2`. With the standard
approximately `162` px control width, the inset is `3` px. At `800x600`, the
final x is therefore around `800 - 3 - 162 = 635`, not the raw DLU-derived
`638`.

Static constants read from `0x007F5BE0..`:

| Address | Value | Meaning in these helpers |
|---:|---:|---|
| `0x007F5BE0` | `640` | small shell width |
| `0x007F5BE4` | `800` | standard shell width |
| `0x007F5BE8` | `1024` | high-res threshold peer |
| `0x007F5BEC` | `480` | small shell height |
| `0x007F5BF0` | `600` | standard shell height |
| `0x007F5BF4` | `768` | high-res threshold peer |
| `0x007F5BF8` | `168` | right-panel/sidebar width constant used for `0x71D` centering |

Movie `0x71A` placement:

| Resolution class | `0x71A` position | Movie |
|---|---|---|
| `640x480` | `0,0` | `Ra2ts_s` |
| `800x600` | `0,0` | `Ra2ts_l` |
| `1024x768` | `(1024-800)/2, (768-600)/2` = `112,84` | `Ra2ts_l` |
| custom `801..1023` width | `(w-800)/2` | `Ra2ts_l` |

Common background/right-panel placement:

- `RightPanel__ComputeLayoutRects` uses `local_x = (screen_w - 800) / 2` only
  when `screen_w > 1023`.
- It uses `local_y = (screen_h - 600) / 2` only when `screen_h > 767`.
- Therefore standard `1024x768` centers the common shell block at `112,84`, but
  odd sizes between `801x601` and `1023x767` can center the RA2TS movie earlier
  than the common background/right-panel layout.
- Width and height asset-size choices are independent: width checks compare
  against `640`, height checks compare against `480`. Non-standard pairings can
  mix small-width and large-height dimensions in computed layout rects.

Clipping:

- Parent background and right-panel SHP draws pass flags `0x400` to
  `CC_Draw_Shape`; this is not the center flag `0x200`.
- Owner-draw text sets the BitFont clip rect to the control rect before drawing.
- Static/movie controls validate their own rects after paint; the parent dialog
  validates the full parent rect after `WM_PAINT_Handler`.

## Asset/Load Failure Behavior

### Missing PCX button pieces

The owner-draw PCX loader returns `0` when the file cannot be opened/decoded and
does not insert a cache entry. `OwnerDraw_Button_00612B70` then dereferences the
lookup result for cap width/blit/tile. There is no primitive Win32/GDI button
fallback in the active PCX-button path.

Confidence: High from `FUN_006B9D00`, `FUN_006BA140`, and
`OwnerDraw_Button_00612B70`.

### Missing `Ra2ts_s/l`

The movie resolver tries `.BIK` first and `.VQA` second. If both fail, the
generic movie constructor returns `0`. `OwnerDraw_Static_006153E0` then kills
timer `0x65`, leaves the movie handle slot clear, and the static's normal
`WM_PAINT` path validates without drawing a movie. There is no primitive image
fallback for the RA2TS panel.

Confidence: High for missing-base-name no-handle behavior.

### Corrupt Bink/open failure

`FUN_00432750` logs `Bink Error: %s` and returns `0` if `_BinkOpen` fails.
`VQMovieHandle__Constructor @ 0x005C07D0` can still allocate a Bink wrapper with
its concrete Bink object slot set to `0`. The static code checks only whether
the generic wrapper pointer is non-null before calling vtable methods and using
width/height fields. The decompiler is not clean enough to state the exact crash
or blank outcome for every corrupt-file case, but there is still no primitive
fallback.

Confidence: Medium for exact corrupt-Bink runtime outcome; High that no fallback
art path exists.

## TS-Legacy and Inactive Paths

`GraphicMenu` / `Title.PCX` remains inactive for standard YR initial menu
`0xE2`:

- `FUN_00531CC0` creates dialog `0xE2`, not a GraphicMenu object.
- Visible buttons are Win32 owner-draw button controls.
- The RA2TS panel is static `0x71A` using custom movie messages.
- No `GraphicMenu` constructor or item loop is needed for the `0xE2` paint,
  input, tooltip, text, movie, or return-code path.

Preload-only or non-`0xE2` assets must not be treated as visible main-menu
assets without a direct caller:

- `bud_*` button PCXs are preloaded but not selected by the normal button path;
- `dbak6440.pcx` is the generic fallback for a different common-paint mode, not
  active for `0xE2` mode `1`;
- Skirmish `MnScrnLCoopGameSetup.shp/.PAL` is active for dialog `0x102`, not
  the standard initial menu;
- `STT:MainButtonYuriWebSite` is tooltip mapping for control `0x55F`, not proof
  that `0x71C` draws a website icon.

## Implementation Implications

For pixel-faithful standard YR initial menu rendering:

1. Compose the common shell background/right-panel layer first:
   `MNSCRNS.SHP` at 640, `MNSCRNL.SHP` otherwise, through `SHELL.PAL`, plus the
   right-panel SHP stack and palette split documented above.
2. Draw the RA2TS Bink panel through child `0x71A` after the parent shell paint.
   Use `Ra2ts_s` only at width `640`, `Ra2ts_l` otherwise.
3. Draw right-side buttons as `SDBTNANM.SHP` cells (frames 2/3/4 = default/hover/
   pressed, SDBTNANM.PAL), NOT `bue/bde` PCX (corrected 2026-05-30). The five non-Exit
   buttons are 156×42 windows at x=644 (flush-right in the 168 panel), grid-snapped Y;
   Exit `0x3EE` keeps its raw template ≈162×37 @ x=638. Labels stay yellow `#FFFF00`
   GAME.FNT with mouse-down `GUIMainButtonSound`. See
   [MAIN_MENU_0XE2_BUTTON_PAINT_AND_REPOSITION_FORK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_0XE2_BUTTON_PAINT_AND_REPOSITION_FORK_GHIDRA_REPORT.md).
4. Render `0x694`, `0x695`, and `0x71D` through the same owner-draw static text
   path. `0x695` is dynamic hover/status text; `0x71D` is initialized by the
   dialog proc.
5. Do not add visible output for `0x71C` unless a retail capture or runtime
   watchpoint proves a live main-menu message or show/hide path reaches it.
6. Do not use GraphicMenu, `Title.PCX`, Skirmish `0x102` backgrounds, or
   `MAINBTTN.PAL` for standard initial dialog `0xE2`.

## Open Questions

1. A retail screenshot/video capture is still needed to confirm exact final
   pixels, especially after the verified `0x694`, `0x695`, and `0x71D` child
   reposition helpers, the first visible Bink frame, and any off-by-one Win32
   font metric differences.
2. The exact runtime effect of a corrupt `.BIK` that resolves by filename but
   fails `_BinkOpen` remains only medium-confidence: the binary has no fallback,
   but whether it blanks or crashes depends on wrapper state that the decompiler
   does not expose cleanly.
3. The `SDBTNANM.SHP` frame-10 overlay condition is verified as a record-byte
   branch, but the semantic name and all state transitions of that byte remain
   unresolved.
4. The lone `0x71C` show/hide helper at `0x0046DE20` has no direct caller in the
   current Ghidra xrefs. A runtime watchpoint on `GetDlgItem(hwnd, 0x71C)` during
   first main-menu entry would be the strongest final proof.

## Sources

Fresh Ghidra functions decompiled or created in this pass:

- `FUN_00531CC0`
- `MainMenuDialog0xE2_Proc_00531F60` (created at `0x00531F60`)
- `FUN_0052B9B0`
- `FUN_00622B50`
- `WM_PAINT_Handler @ 0x00621E90`
- `FUN_0060F9A0`
- `FUN_0060CF00`
- `FUN_0060C540`
- `FUN_0060C4A0`
- `ResizeShellChildControl_0060C0C0` (created at `0x0060C0C0`)
- `FUN_0060B550`
- `FUN_0060B610`
- `FUN_0060B950`
- `FUN_00601360`
- `FUN_0060CAF0`
- `FUN_0060C930`
- `FUN_0060CCC0`
- `FUN_0060CDB0`
- `FUN_0072E260`
- `FUN_0072E280`
- `FUN_0072DFB0`
- `RightPanel__Draw @ 0x0072E450`
- `RightPanel__ComputeLayoutRects @ 0x0072EC70`
- `Background_Overlay @ 0x0072E730`
- `FUN_0072ADE0`
- `OwnerDraw_Static_006153E0`
- `OwnerDraw_Button_00612B70`
- `FUN_00621040`
- `FUN_006211D0`
- `FUN_006040B0`
- `CenterChildWindow @ 0x00777080`
- `FUN_0074FAE0`
- `FUN_00735120`
- `VQMovieHandle__Constructor @ 0x005C07D0`
- movie resolver helper `0x005C0640`
- Bink constructor/open path `0x004326C0`, `0x00432750`
- Bink thunk functions created at `0x005C0580`, `0x005C05A0`, `0x005C05F0`
- `ToggleMpScoreControls_0046DE20` (created to inspect the lone `push 0x71C`)

Binary/memory evidence:

- RT_DIALOG `0xE2` from prior resource parse.
- String block `0x00845104..0x00845188` for common shell SHPs.
- Palette string block `0x00845438..` for `SDBTNANM.PAL`, `SHELL2.PAL`,
  `SHELL.PAL`, `MAINBTTN.PAL`, `DIALOG.PAL`.
- Immediate scan of retail `gamemd.exe` for `push 0x71C`, `push 0x55F`,
  `push 0x71B`, `push 0x4E4`, and `push 0x4F0`.

Prior reports referenced:

- [docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md)
- [docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md)
- [docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md)
- [docs/research/MAIN_MENU_DIALOG_0XE2_OWNERDRAW_VISUAL_FOLLOWUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_DIALOG_0XE2_OWNERDRAW_VISUAL_FOLLOWUP_GHIDRA_REPORT.md)
- [docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md)
- `docs/research/SKIRMISH_OWNERDRAW_CALLBACKS_FOLLOWUP_GHIDRA_REPORT.md`
- `docs/research/SKIRMISH_SHELL_RIGHT_PANEL_BACKGROUND_PALETTE_FOLLOWUP_GHIDRA_REPORT.md`

## Related reports (added 2026-05-18 main-menu --area swarm)

Five new reports from the 2026-05-18 main-menu swarm extend or partially
resolve open questions in this doc:

- [SDBTNANM_FRAME10_OVERLAY_CONDITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SDBTNANM_FRAME10_OVERLAY_CONDITION_GHIDRA_REPORT.md) — **partial resolve
  of the SDBTNANM frame-10 deferred item.** Branch at `0x0072E5E6` is a
  binary highlight-vs-default selector (no pulse cadence). Predicate input
  is the byte at WindowExtra record `+0xD8`; reader at `0x00621FEC`,
  writer `FUN_00608440 @ 0x00608440` (4 callers in dialog-proc
  continuations). Clearer `FUN_006084A0` has zero xrefs — bit is
  sticky-on. UX semantic remains unresolved.
- [QUIT_CONFIRM_DIALOG_MAIN_MENU_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/QUIT_CONFIRM_DIALOG_MAIN_MENU_GHIDRA_REPORT.md) — Quit button (`0x3EE`)
  → `Main_Game` case-6 → RT_DIALOG `0x120` via `FUN_005D3490`, CSF
  `GUI:ExitAreYouSure / TXT_OK / GUI:Cancel`. Result codes 0/1/2 written
  through `SetWindowLong(hwnd, 8)`. Clean return-cascade shutdown — no
  `PostQuitMessage`/`ExitProcess`.
- [EVA_WELCOME_BACK_MAIN_MENU_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/EVA_WELCOME_BACK_MAIN_MENU_TRIGGER_GHIDRA_REPORT.md) — verified-negative:
  the only audio on entry through `FUN_00531CC0` is the INTRO music. No
  `VoxClass::PlayEVA` on the shell path.
- `MAIN_MENU_MUSIC_TRACK_AND_LOOP_GHIDRA_REPORT.md` — `Main_Game @
  0x0052D9A0` pushes `"INTRO"` at `0x008263a8` to `Theme::From_Name` →
  `Theme::Play`. Per-theme `Repeat=yes` re-queues via `Theme::AI` poll.
- [SHELL_BUTTON_SLIDE_SOUND_CALL_SITE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_BUTTON_SLIDE_SOUND_CALL_SITE_GHIDRA_REPORT.md) — initial menu
  dialog `0xE2` does NOT play `ShellButtonSlideSound` (common shell proc
  `FUN_00622B50` invokes the slide animation with `DL=0` only). Resolves
  an attribution ambiguity adjacent to this doc's owner-draw analysis.
