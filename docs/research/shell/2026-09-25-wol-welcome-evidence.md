# Main Menu → Internet: Westwood Online welcome `0x10E`, native evidence

Bounded evidence for Main Menu → Internet on a machine without the Westwood
Online components: the welcome page `0x10E`, its way back, and the
`TXT_APIMISSING` box its WOL actions end in. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), layout arithmetic executed under
Unicorn (research harnesses: rect initializer, `WM_INITDIALOG` relayout,
static setup, group-box paint, slide engine), Ghidra as a navigation aid.
The Ghidra database carries the names used here.

## Route

- `0xE2`'s Internet (`0x684`) writes 2 (`0x005320C2`); `0xE2` slides out.
- `Main__PrepareSession` state 2 (`0x0052DD57`) sets GameMode 4 and state
  0x10 runs `WOL__RunMain` `0x0077B2A0` (WOL_Main).
- Music on entry: with INTRO current, `Queue(-3)` (`0x0077B2FB`); the
  `[WOnline]` settings are read (`0x0077DED0`, `LobMusic` by `ReadInt`,
  default 1); the score shuffle flag `[0x00A83D22]` is saved and forced on
  (`0x0077B30C`); `WOL__ApplyLobbyMusic` `0x0077E110` then, with LobMusic,
  stops INTRO at once and queues -2 when nothing plays or INTRO is still
  current (Theme AI shuffles the next track), else queues -3.
- `SimpleWonlineDialogControl` `0x00798DE0` draws the empty shell backdrop
  and runs `0x10E` (proc `WolWelcomeDialog__Proc_10E` `0x007988C0`); the
  variant `0x11C` needs the WOLAPI `Options` registry bit, absent without WOL.
- Main Menu (result 0) slides `0x10E` out; WOL_Main restores the shuffle
  flag (`0x0077B549`, …) and returns; `0x0052E28E` plays INTRO and state
  0x12 builds a new `0xE2` with its entry slide.
- Quick Match, Quick Co-op, Custom Match, Buddy List and My Information
  (modes 1, 2, 4, 5, 6; `0x00798C39`) slide `0x10E` out, then
  `WOL__EnsureWolapiObject` `0x00786390` cannot create the WOLAPI COM object
  (`0x00785370`) and shows `TXT_APIMISSING` (`0x00785711`) in the message box
  `0xD0` (`0x005E2CD0`: modal, no slide, one OK). OK returns -2, and the game
  goes back to a new `0xE2` the same way.
- Community (`0x55F`) opens `HKLM\Software\Westwood\Yuri's Revenge\URL`
  `Community` in a browser (`0x0077DC90`); without the key nothing happens.
- Enter and Esc arrive as IDOK/IDCANCEL; the proc ignores them. The box's
  loop (`0x007759E0`) dispatches without `IsDialogMessage` (keys inferred to
  do nothing there).

## The page

A family page (Session `+0x30D8` clear):

- **Right panel:** heading `GUI:WOLWelcome`, status line (template 316 DLU
  wide: `(10, 578, 475, 21)` at 800x600), monitor; six top buttons snapped by
  their template tops (rows 199–409 at 800x600), Main Menu on the bottom row.
  Slide tuple (6, 1, 0, 0), executed with the other dialogs by
  `tools/storage_oracle/shell_slide_engine.py` (6 more cases). At 640x480
  the six top buttons fill all six rows, so the engine draws the last row
  twice (Community and Main Menu); the Rust column follows it. Hit tests
  follow the template Z-order, so Main Menu takes that shared row.
- **Background:** MultiplaySelection.shp through MultiplaySelection.pal at the
  dialog origin (`0x0072E730`), loaded only when the screen is exactly 800
  wide (`MultiplaySelectionArt__Load` `0x0072C7E0`).
- **Left side** (template places at every size, one pixel wider and taller
  than the 6x13 conversion like every family child): the ladder stats box and
  the icon glossary box (`BS_GROUPBOX`, `0x0061E700`: the list's two rings with
  the top edge eight pixels down); kind-0 text statics (right-aligned labels,
  `GUI:UnknownStats` values with no nickname, centred glossary header,
  left-aligned descriptions); 13 kind-2 PCX icons, magenta transparent,
  centred only along an axis where the window is larger (`0x00615831`); the
  `0x79F` icon one pixel right (`0x0060C002`).
- **Status help** (`0x006040B0`): the buttons' `STT:WOL…`/`STT:QuickCoop`/
  `STT:MainButtonYuriWebSite` keys, the stats values
  `STT:WOLMyInformation{Wins,Losses,Disconnects,Rank,Points}`, `0x797`
  `STT:WelcomeUpdate`.
- **Box `0xD0`:** the `0xCE` geometry: PUDLGBGN, body static, one MNBTTN OK,
  over the empty backdrop (MNSCRNL, right panel, shut column).

## VERA20k

- `ui::wol_shell` owns `0x10E`: the page spec, actions, layout (family right
  panel, template children, Z-order), status help and the visit state.
- `app::shell_wol` opens it after `0xE2`'s slide-out, drives input through the
  shared controller (GUIMainButtonSound on presses), slides it out for Main
  Menu (`ShellExitThen::WolBack`) and the WOL actions
  (`ShellExitThen::WolApiMissing`), shows the box, and returns to a new
  `0xE2`.
- `app::frontend::wol_welcome_render` paints the page and the box; the
  group box shares the movie list's frame painter; the art is
  `WolWelcomeArt`.
- Music: `ThemeRuntime::enter_wol_lobby` / `leave_wol_lobby` port the entry
  and exit rules; the menu's INTRO upkeep yields to Theme AI while the route
  is WOL. `[WOnline] LobMusic` is read into the options profile (read-only).
- `ui::shell::modal::body_ok_layout` gives the one-button box geometry.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` with the cursor at (400, 300); retail stills
taken windowed at desktop (560, 200) (prefix `Screenshots/`,
`yuri's_revenge_2026-09-25_12-14-*` and `12-15-*`). Masked: the cursor (and the hover
pointer), and the monitor where it shows.

| Checkpoint | Retail still (SHA-256 prefix) | Differing pixels |
|---|---|---|
| `wol-0x10e-steady` | `wol-steady.png` (`b8a29c16`) | 0 |
| `wol-0x10e-entry-tick-17` | `wol-entry.png` (`e3755c01`, entry slide tail) | 0 |
| `wol-0x10e-hover-quick-match` | `wol-hover-quick.png` (`0960a38f`) | 0 |
| `wol-0x10e-back` (movie masked) | `wol2-back-settled.png` (`68491d98`) | 0 |
| `wol-0x10e-api-missing` | `wol-apimissing.png` (`2a77759f`) | 0 outside the box; the box sits 1 px right and 1 px lower (70 of 140,480 box pixels differ once shifted) |

The route logs record Internet → `0x10E` → the press → the teardown's end →
the box, or the new `0xE2` entry slide and settle.

## Evidence levels

- **Native behavior established:** route, results, music rules, the WOLAPI
  failure path, keyboard, status help, from instructions.
- **Native execution:** layout, static setup, group-box draw lists and the
  slide column (research harnesses; `shell_slide_engine.py` in the repo).
- **Parity demonstrated:** the comparisons above.
- **Rust regression tested:** `ui::wol_shell` (right panel, template
  children, icon placement, status help, actions, the 640 Z-order),
  `audio::theme` (WOL entry and exit music), `ui::shell::slide` (column
  golden), `app::diagnostics::shell_capture` (checkpoints).

## Residuals

- **Box position.** The box is 1 px right and 1 px lower than retail: the
  shared message-box centring ignores the one-pixel window grow (the same for
  `0x120` and `0xCE`).
- **Box press sound** is GUIMainButtonSound from the owner-draw press path
  (`0x00613667..0x00613771`); the type-3 button's own path is not traced.
- **Lobby track choice.** Theme AI draws the shuffled track from VERA20k's
  presentation stream (unseeded at the main menu), not `g_MainRng`, so the
  first lobby track is always the same one.
- **640 and 1024.** At 640 retail draws MNSCRNS through the
  MultiplaySelection palette (almost black) and paints Community over Main
  Menu (inferred); at 1024 no shape loads and whatever the empty backdrop
  left shows. VERA20k shows the family backdrop at both; neither is
  captured.
- **Connected WOL.** With WOLAPI installed the actions connect to Westwood
  Online (network only); not ported.
