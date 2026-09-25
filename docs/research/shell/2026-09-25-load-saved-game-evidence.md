# Load Saved Game `0xB7` from Single Player: native evidence

Bounded evidence for Single Player → Load Saved Game: the dialog `0xB7` as
the main menu shows it, what it does, and how it leaves. Retail `gamemd.exe`
SHA-256 `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`;
control flow read from instructions (Capstone), Ghidra as a navigation aid.
The Ghidra database carries the names used here.

## Route

- Single Player's Load Saved Game (`0x689`) writes 9 (`0x0052D750`). `0x100`
  is torn down with its slide-out (`0x0060D380` → `ShellDialog__Teardown`
  `0x00622720`).
- `Main__PrepareSession` state 9 (`0x0052E0BE`) rescans `BATTLEMD*.INI`
  (`0x0052CB90`, not UI), builds the saved-game receiver (`0x00558740`) and
  runs it in Load mode (`LoadOptionsClass__RunLoadDialog` `0x005587F0` →
  shared loop `0x00558DD0`). Loaded → `0x0052E5DC` (game start); otherwise
  state 1 recreates Single Player (`0x0052E0F9`).
- The loop creates `0xB7` with proc `LoadDialog__LoadModeProc_B7`
  `0x00558A30` (`0x00558F2E..0x00558F39`).

## One flag decides the shape

`SessionClass__IsGameSuspended` `0x0069BBE0` returns Session `+0x30D8`, set
only while a game is suspended behind an in-game menu (`0x0069BAB0`) and
cleared by `0x0069BB40`. It is clear at the main menu, so `0xB7` there is a
family page, unlike the in-game `0xB7` VERA20k already paints:

- **Background:** `0x0060CF00` has no case for `0xB7`: the default
  `0x0060D20B` (MNSCRNL, MNSCRNS at 640). Paint `0x00621FB1..0x00621FFE`
  draws the right panel (`0x0072E450`) and the backdrop.
- **Right panel:** heading `0x694` `GUI:LoadMissionMenu` at the family place
  (`0x0060B1D0` + `0x0060B950`); status line `0x695` a kind-1 reveal
  (`0x00602AB8`). The template has no monitor `0x71C` and no movie `0x71A`,
  so the monitor window shows the panel's own art.
- **Prompt:** `0x40C` is hidden (`0x00558F7C..0x00558F99`).
- **List:** `0x0060B7A0` returns at once (`0x0060B7BC`), leaving the
  template rectangle.
- **Slides:** `0x0060C540` marks `0xB7` slide-capable; the entry slide starts
  on the first child paint (`0x00612690` → `0x00608260`). The column: Load
  `0x40F` is the one top button (`0x006091A5`; `0x0060A180` counts visible
  owner-draw buttons, disabled or not), Back `0x686` the bottom one
  (`0x00609844`), no map button or top panel. Executed with the other
  dialogs by `tools/storage_oracle/shell_slide_engine.py` (6 more cases).

At 800x600: heading `(635, 9, 163, 18)`, status line `(10, 578, 456, 21)`,
Load `(644, 199, 156, 42)` (DLU top 122 snapped like the first family
button), Back `(644, 535, 156, 42)`, list window `(119, 127, 399, 304)`.

## Behaviour

- **List** (`LoadSaveDialog__RebuildEntryList` `0x005596A0`): every `*.SAV`
  the reader accepts, newest first; columns at x 2 / 255 / 315 (`0x00558B3A`):
  description, short date, time. The first compatible row is selected
  (`0x00559BB5`).
- **Load** is enabled when the list has rows (`0x00558FC8..0x00558FE5`), with
  or without a selection. A double-click on a row loads it (`0x00558AC0`).
- **Buttons** play GUIMainButtonSound on the press
  (`0x00613667..0x00613771`).
- **Status help** (`0x00604261..0x0060429A`): list `STT:LoadList`, Load
  `STT:LoadButtonLoad`, Back `STT:LoadButtonBack`.
- **Keyboard:** Enter and Esc arrive as IDOK/IDCANCEL; the proc ignores them.
- **Back** writes 2 (`0x00558AB0`): the teardown slides out, then state 1
  recreates Single Player with its entry slide.
- **Load** (`0x00559051`): the dialog hides, the Loading panel `0xF0` shows,
  and `Load_Game` `0x0067E440` runs; success enters the game with no shell
  slide-out. On failure the `TXT_ERROR_LOADING_GAME` box shows, then `0xB7`
  is hidden again and nothing shows it (`0x00559594`).
- **Single Player gate** (`LoadOptionsClass__HasLoadableSave` `0x00559C20`):
  Load Saved Game `0x689` is enabled for the first save the same reader
  accepts (minus `SAVEGAME.NET` and masked attributes).

## VERA20k

- `ui::shell::saved_games` owns `0xB7`: `LOAD_SAVED_GAME_PAGE` and
  `main_menu_saved_game_layout` beside the in-game layout.
- `app::shell_load_saved_game` opens it after `0x100`'s teardown, routes
  input through the shared browser (`ui::shell::saved_file_input`), plays
  GUIMainButtonSound, and commits Back to Single Player. The saved-game rows
  come from one builder shared with the in-game browser.
- `app::frontend::load_saved_game_render` paints it. The family list
  (darkened backdrop, frame rings, scrollbar, selection) is shared with the
  movie list `0x129` (`movies_credits_render::push_family_list`).
- Single Player enables Load Saved Game from the same repository reader the
  list uses.
- A disabled Load takes no press (shared by every saved browser).
- Saved-list dates and times now format on macOS and Linux too (local time,
  the user's locale short date and time); they were blank there before.
- The egui save/load panel no longer opens from Single Player.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` with the cursor at (400, 300) and a
repository holding one save, compared with `tools/shell_capture_diff.py`.
Retail stills: cnc-ddraw windowed at desktop (560, 200), Single Player →
Load Saved Game with one retail save (prefix `Screenshots/`,
`yuri's_revenge_2026-09-25_10-48-*`). Masked: the cursor
`(398, 298, 40, 40)`, the monitor `(670, 47, 93, 55)` and the one row's text
`(120, 128, 398, 18)`, whose save differs.

| Checkpoint | Retail still (SHA-256 prefix) | Differing pixels |
|---|---|---|
| `load-saved-game-0xb7-steady` | `lsg-b7-steady.png` (`71014cca`) | 71, status line only (below) |
| `load-saved-game-0xb7-entry-tick-17` (status line masked) | `lsg-b7.png` (`377b11ae`, the entry slide's tail) | 0 |

The tail still differs from the settled page by 14,502 pixels in the right
panel, so it pins the slide. The status line shows the list help in both. Its
last two glyphs are 1–5 units brighter in VERA20k (71 pixels): a one-line
residual. Retail Back returned to Single Player (`lsg-back-settled.png`,
`53c31dff`).

## Evidence levels

- **Native behavior established:** route, flag-0 layout and paint, list,
  enable rules, sounds, status help, keyboard, Back and the load/failure
  path, from instructions.
- **Native execution:** the slide column (`shell_slide_engine.py`).
- **Parity demonstrated:** the two comparisons above at 800x600.
- **Rust regression tested:** `ui::shell::saved_games` (family rects),
  `ui::shell::saved_file_input` (disabled Load), `ui::shell::slide` (column
  golden), `util::native_file_time` (Unix formatting),
  `app::diagnostics::shell_capture` (checkpoints).

## Residuals

- **Starting a saved game from the main menu.** VERA20k restores saves only
  into a running scenario (`PreparedLoad` needs the live runtime), so Load
  here shows `TXT_ERROR_LOADING_GAME` and keeps the dialog up. Retail hides
  the dialog, shows `0xF0` and enters the game; on a real failure it hides
  the dialog for good (a soft-lock VERA20k does not copy). Every main-menu
  load; a cold load that builds the match from the save is a separate
  mechanism.
- **Save format.** VERA20k lists its own `.bin` saves, not retail `.SAV`;
  older VERA20k saves without the current envelope are skipped, which keeps
  Load Saved Game disabled when only those exist.
- **Date and time strings** follow the platform locale (`strftime_l`
  `%x`/`%X` off Windows) rather than Win32's locale data.
- **Status line tail:** the last two glyphs of the steady help text are 1–5
  RGB565 units brighter than retail (71 pixels, not visible).
- **640x480** and the Back slide-out frames are not captured.
