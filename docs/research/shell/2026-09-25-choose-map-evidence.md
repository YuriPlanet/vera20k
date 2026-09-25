# Skirmish → Choose Map `0x6B`, native evidence

Bounded evidence for the Skirmish map chooser: its route out of and back to
Skirmish `0x102`, the two lists, the buttons and the Use Map checks. Retail
`gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), the slide schedule and the static
kinds executed under Unicorn, Ghidra as a navigation aid. The Ghidra database
carries the names used here.

## Route

- Choose Map (`0x5AA`, `SkirmishDialog__OnCommand` `0x006AD8E7`) saves the
  mode and scenario indices, slides `0x102` out (`0x00608070`: MenuSlideOut,
  no destroy) and hides it; hiding clears its first-paint state
  (`0x0061190E`).
- The runner `0x005E68A0` draws the empty backdrop, loads the
  CustomizeBattle art (`0x0072D120`: MnScrnLCustomizeBattle.shp only at
  exactly 800 wide, and its palette), creates `0x6B` (proc `0x005E6920`) as a
  full-screen family dialog and runs its modal loop `0x007759E0`: no
  `IsDialogMessage`, so Enter and Esc do nothing.
- `0x6B` slides in on first paint. Its slide-in end (`0x4EC`,
  `0x005E6E26`) starts the kind-1 reveals of heading `0x694` and status
  line `0x695`.
- Cancel (`0x5C0`) and Use Map (`0x6C5`) end the dialog through `0x007757E0`:
  `0x6B` slides out and is destroyed; `0x102` is shown again, so it slides
  in again. Its SHOW completion (`0x006230B8`) sends `0x4EE`, which starts
  only statics that never started (`0x00615FDB`): finished ones repaint at
  their final count, and only a changed text restarts (corrected in
  `2026-09-26-skirmish-statics-evidence.md`). Cancel restores
  the saved selection (`0x006AD955`); Use Map commits it (`0x006ADA21`).
- Create Random Map (`0x583`, enabled by the listed mode's
  `RandomMapsAllowed` `0x005D6350`) slides `0x6B` out and hides it, then
  runs the random-map dialog `0x005E8590`. On success the new map is
  selected and Use Map runs (`0x005E6B2F`), eject check included; if Use Map
  declines, the chooser slides back in (`0x005E6B47`).

## Game types

- The mode loader `MPGameOptions__LoadAllGameModes` `0x005D7CE0` reads the
  MPModesMD.ini sections Battle, ManBattle, Siege, Unholy, FreeForAll and
  Cooperative, each through its own factory, so each row's section fixes its
  class (vtables Battle `0x7EE184`, ManBattle `0x7EE50C`, Siege `0x7EE6FC`,
  Unholy `0x7EE814`, FreeForAll `0x7EE424`, Cooperative `0x7EE27C`).
- `MultiplayerGameMode__FillModeList` `0x005D6130` lists, in Skirmish
  (`[0xA8B238] == 5`), only modes whose vtable `+0x40` answers true
  (`0x005D6263`): Battle `0x005C0E20` and FreeForAll `0x005C5E30`; the other
  classes keep the base `0x005D6360` (false). Retail MPModesMD.ini gives
  Battle (1), Free For All (2) and Team Alliance (9, a `[Battle]` row) — the
  three rows of the retail capture. The observer test at `0x005D6239`
  (vtable `+0xBC`) rejects only Cooperative, which `+0x40` already drops.
- Skirmish's init (`0x006AEAC5`) only range-checks the saved mode id, so a
  mode saved by network play can reach the chooser without a row.

## Map list

- The chooser builds its vector by appending (`0x005EEE40`) every scenario
  of the global list `[0xA8B8CC]` the mode accepts; the fill
  `MultiplayerGameMode__FillScenarioList` `0x005D64C0` appends each row with
  `0x4CD`, which the list subclass forwards as `LB_ADDSTRING`
  (`0x0061B4F3`); the templates have no `LBS_SORT`. Rows therefore keep the
  scan order of `0x00699980` (MISSIONSMD.PKT `[MultiMaps]`, then loose
  `*.PKT`, `*.YRO`, `*.YRM`). MISSIONSMD.PKT lists the four-player maps by
  title, which is why the retail list looks alphabetical from DC Uprising on.
- The init selects the current map and scrolls it to the top
  (`0x005E7000..0x005E701B`); the preview follows the highlighted map
  (`0x005E66F0`); a game-type change keeps the same-named map selected
  (`0x005E6CDF..0x005E6D43`); the last clicked game-type row `[0x8316FC]`
  lives across visits (`0x005E6BB7`).

## Lists

Both lists are the owner-draw list `0x00618D40` (paint `0x00619230`):

- **Window:** the 6x13 conversion of the template, one pixel wider and
  taller like every family child: `(116, 127, 196, 344)` and
  `(338, 127, 196, 344)` at 800x600. The paint surface adds one more column
  and row, so rows start at `(x+1, y+1)`, 19 px tall, 18 visible.
- **Frame:** two rings around the window, outer light `0xC5BEA7`
  top/left and dark `0x807A68` bottom/right, inner swapped, corners
  `0xA29C87` — the same paint as the movie list `0x744`.
- **Scrollbar child** (`0x0061C690`): a light line at `bar.x`, a dark line
  at `bar.x + 1`, the parent background inside, arrows at `bar.x + 2`, the
  grip middle tiled from the thumb top under its caps. `bar.x` is 514 on the
  map list.
- **Input:** a press on a row selects it and plays GenericClick
  (`0x0061AA5A`); presses below the last row are ignored; the second click
  of a double-click only notifies `LBN_DBLCLK` (`0x0061A904..0x0061A945`),
  which `0x6B` ignores; the scrollbar captures and repeats (500 ms, then
  25 ms). The subclass has no key or wheel case; that the stock LISTBOX it
  forwards them to (`0x0061C48A`) changes nothing visible is inferred, not
  executed.
- **Status help:** a child's hover message repaints the status line
  (`0x00615EF7`); over the background nothing is sent, so the last text
  stays.

## Use Map

`ChooseMapDialog__UseMap` `0x005E7160`: no map row → nothing. When a
Skirmish AI row at or past the new map's player limit is occupied
(`0x006ACCA0`), `GUI:EjectAIPlayers` asks with OK/Cancel
(`0x005E72D6..0x005E7336`); Cancel keeps the chooser. With no game-type row
the null mode is dereferenced at `0x005E721F` (native crash).

## Retail comparison

Retail stills from the helper at 800x600 (retail RA2MD.INI: Battle, scenario
index 35): `cm-6b-a.png` (entry), `cm-6b-steady.png`, `cm-ffa.png`,
`cm-ret-steady.png`. VERA20k captures `skirmish-0x6b-steady`,
`skirmish-0x6b-entry-tick-17` and `skirmish-0x102-choose-map-return` taken
with the same RA2MD.INI, compared in RGB565 units with
`tools/shell_capture_diff.py`:

- Steady: game types, map rows, selection, scroll position, scrollbar,
  frame, heading, buttons and the status help (`STT:ScenarioListMaps`, the
  pointer resting on the map list) match. Left: 1-px text offsets, the
  one-unit list darkening, and the preview's scaling (the same difference as
  on `0x102`).
- Entry tick 17 against `cm-6b-a.png`: the right column and top panel are
  identical (0 pixels over 2 units); the left side differs only as above.
- Return: `0x102` settles with Battle / DC Uprising (2-4); the retail still's
  combos are mispainted by Wine, so those rows are not compared.

## Residuals

- The empty backdrop presented before `0x6B` exists and at its slide-in end
  (`0x005E68A7`, `0x005E6E2A`) is not drawn; whether either shows a frame is
  unknown.
- The random-map dialog `0x105` is slide-capable (`0x0060C540`,
  `0x0060C5F7`); VERA20k swaps it in and out without slides (random-map
  chain).
- Use Map validates the Cooperative assignment after `0x6B`'s slide-out;
  native validates before `0x007757E0`. Unreachable while Skirmish lists no
  Cooperative mode.

- The list's one-RGB565-unit darkening under its rows is not drawn.
- List and label text sit one pixel lower than retail; button captions one
  pixel higher.
- Use Map with no game-type row: native crashes; VERA20k keeps the current
  mode (reachable only with a network-saved mode).
- The random-map dialog `0x105`'s heading keeps its own rect (not compared).
- Keyboard `0xA3`, Sound and the saved-seed browser lists keep raw-DLU
  rectangles (no retail still to compare the family-child pixel).
- MPModesMD.ini values: native treats any 5th field but `false` as true
  (`0x005D795D..0x005D7974`); VERA20k reads yes/true/1. Retail rows parse identically.
