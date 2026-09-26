# Random map `0x105`, native evidence

Bounded evidence for Choose Map's random-map dialog `0x105` as a family page:
its slides, heading and status line, hover help, the chooser's statics around
it, the seed browser over it, and the disabled button paint it shows. Retail
`gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone); the slide schedule, the static
getters, the relayout and the help lookup executed under Unicorn; Ghidra as a
navigation aid. The Ghidra database carries the names and comments cited here.

## Route

- Create Random Map (`0x583`) slides `0x6B` out (`0x005E6A03`) and hides it
  (`0x005E6A0B`) without destroying it, then runs
  `ChooseMap__RunRandomMapDialog` `0x005E8590`, which picks the setup callback
  by session type and runs `RandomMapSetupDialog__Run` `0x00595BC0`.
- `0x105` is a family page. It slides in on first paint with four top buttons
  (Use Map, Load Map, Save Map, Delete Map), the bottom button (Cancel), no map
  button and the top panel; the executed engine
  (`tools/storage_oracle/shell_slide_engine.py`) now covers `0x105` in both
  directions at three sizes.
- Use Map (`0x6C5`) and Cancel (`0x5C0`) end the dialog through its teardown
  slide (`0x00595C42` → `0x00622720`).
  - Result 1: the runner loads `RandMap.Sed` (`0x005E85D1`) and returns the
    new map's index. The chooser selects it and runs its own Use Map while it
    stays hidden (`0x005E6A16..0x005E6B2F`).
  - Cancel (-1), or a declined Use Map: `0x005E6B47` calls
    `ShellDialog__SlideIn` on the still-hidden `0x6B`, which returns at once
    (`IsWindowVisible`, `0x006082E3..0x006082F4`). `ShowWindow` follows
    (`0x005E6B51`), and the first paint slides the chooser in.
- Load, Save and Delete Map (`0x0059693F`, `0x005968CC`, `0x005969BF`) store the
  options (`0x00596C70`) and run `LoadSaveDialog__RunModalLoop` `0x00558DD0`
  directly. The browser creates `0xB7`, `0x2B4` or `0x2B5` (`0x00622650`) and
  shows it (`0x00622800`) over `0x105`, which is neither hidden nor slid out, so
  closing the browser uncovers `0x105` without a slide.

## Statics

- **Parameters:** `0x105`'s heading `0x694` (`GUI:GenerateMap`) and status
  line `0x695` are kind 1: 30 ms, step 1, range 8 and 15 ms, step 3, range 16.
  These come from the executed getters (`tools/storage_oracle/shell_static_timers.py`,
  which now includes `0x6B` and `0x105`).
- **Placement** (executed relayout, `tools/storage_oracle/shell_relayout.py`):
  - heading `(635, 3, 163, 17)`, with the fix-up's +1 y;
  - status line `(10, 578, 456, 21)`, from the 303x12 DLU template through
    `0x0060B550`;
  - preview `0x468` `(644, 37, 145, 113)`.
- **Hover:** `RandomMapSetupDialog__Proc` `0x00596300` runs the common handler
  first (`0x0059631A`). Every mouse move writes the help of the control under
  the pointer to `0x695`, or an empty text over the background, and every write
  repaints the line (`0x00615EF7`).

  The labels come from the table lookup `0x006040B0`, executed for every
  control of 15 offline shell dialogs (`tools/storage_oracle/shell_help_keys.py`):

  | Control | Help |
  |---|---|
  | Terrain Type, Time of Day, Climate, Map Size, Resources | `STT:GenerateCBoxEnvironment`, `…Time`, `…Theater`, `…MapSize`, `…Resources` |
  | Players | `STT:GenerateSliderNumPlayers` |
  | Surprise Me, Generate Map | `STT:GenerateButtonSurprise`, `STT:GenerateButtonPreview` |
  | Use Map, Load, Save, Delete Map, Cancel | `STT:GenerateButtonUseMap`, `…LoadMap`, `…SaveMap`, `…DeleteMap`, `…Cancel` |

  VERA20k drew no status line on `0x105`.
- **Lifetime:** `0x6B` keeps its statics while `0x105` runs, because it is
  hidden, not destroyed.
  - Back from `0x105`, its heading repaints at the final count: the last letter
    is plain (31, 63, 0) in `rmg-cancel-b.png` and `rmg-cancel-steady.png`,
    where the first show left trail step (31, 63, 3) (`rmg-cm.png`).
  - Its status line shows the help it held.
  - `0x105` keeps its own statics under the seed browser for the same reason.
- **During slides:** `ShellDialog__SetSlideChildSuppressProc` `0x00606800` makes
  `0x694` and, through `ShellDialog__HasRevealStatusLine` `0x00601360`, `0x695`
  validate-only while a slide runs. This holds for `0x6B`, `0x105` and `0x102`.
  - `rmg-cancel.png` shows `0x6B`'s last entry tick after Cancel, with the
    column still captionless. Its status line shows the help it held ("Create a
    random battlefield."), while the heading area shows the top-panel art.
  - So the status line, outside the engine's blits, keeps what it painted when
    the dialog was shown; the heading lies under the top-panel blit.
  - The paint order between `ShowWindow` and the first-paint slide is not
    traced; the capture fixes the result.

## Disabled buttons

Owner-draw buttons record their type at `+0xB0`: 1 for the family column's
SDBTNANM buttons.

- **Art:** the disabled paint blends black over the art only for type 0
  (`0x006135F3..0x0061361B`), so a disabled column button keeps its art.
- **Caption:** it takes the colour `[0x00B0FA94]` (`0x00612F5F` →
  `0x00613072`), which `0x0072A8C0` sets to (0x48, 0, 0) outside a suspended
  game.
- **Retail:** `rmg-steady.png` shows the disabled Use Map, Save Map and Delete
  Map at full art, with captions at RGB565 red 9.

VERA20k dimmed disabled art to half on the menu pages and `0x105`, and drew the
captions `#9F0000`. The fix applies to every family page, for example Single
Player's Load Saved Game when there is no save.

## VERA20k

- **Page statics:** `ui::shell::static_reveal::DialogStatics` owns the heading
  and status line of a dialog that another dialog hides or covers. It lives in
  `ChooseMapModalState` (`0x6B`) and `RandomMapSetupModalState` (`0x105`):
  - `show` runs at the SHOW completion (start once, else repaint);
  - `hover` runs on every mouse move;
  - `shown_again` runs when the dialog shows again or is uncovered.

  `SkirmishStatics` (`0x102`) is built on it. The shared per-page statics
  cannot serve these dialogs, because every slide-target change resets them.
- **Slides:** `ShellSlideKind::RandomMap` and the exits
  `ShellExitThen::RandomMapUse` and `RandomMapCancel`. Both Use Map paths (the
  immediate one and the one deferred behind a generation) and both Cancel paths
  end through the teardown slide.
- **Which dialog shows:** `SkirmishShellState::top_dialog` is the one decision
  for which dialog of the Skirmish stack shows. It serves the slide target and
  the renderer. The seed browser leaves `0x105` as the slide target, so closing
  the browser repaints `0x105`'s statics and starts no slide.
- **Help keys:** `status_help_key_for_random_map_setup`, pinned to the executed
  table.
- **Disabled buttons:** `ButtonPolicy::disabled_dim` and
  `BUTTON_DISABLED_ALPHA` are removed; `SHELL_TEXT_RGB_DISABLED` is the
  native caption colour.

## Retail comparison

The retail stills come from the helper at 800x600 with retail RA2MD.INI. The
VERA20k release captures use the same INI. Differing RGB565 pixels in the
heading (635, 0)–(800, 32), the status line (0, 572)–(640, 600) and the right
panel (632, 0)–(800, 600):

| VERA20k checkpoint | Retail still | Heading | Status line | Right panel |
|---|---|---|---|---|
| `skirmish-0x105-steady` | `rmg-steady.png` | 0 | 0 | 0 |
| `skirmish-0x105-entry-tick-17` | `rmg-a.png` | 0 | 0 | 0 |
| `skirmish-0x105-hover-surprise` | `rmg-hover-surprise.png` | 0 | 0 | 0 |
| `skirmish-0x105-hover-load` | `rmg-hover-load.png` | 0 | 0 | 186, the pointer only |
| `skirmish-0x105-cancel-tick-17` | `rmg-cancel.png` | 0 | 324, `0x6B`'s text 1 px low | 0 |
| `skirmish-0x105-cancel-steady` | `rmg-cancel-steady.png` | 0 | 432, the same | 9733, preview scaling and `0x6B`'s 1-px captions |

`skirmish-0x105-use-map` plays Generate Map, then Use Map. The dialog slides
out, the chooser commits the map and `0x102` settles with it.
`skirmish-0x105-seed-browser-return` plays Generate Map and Save Map, then the
browser's Back. `0x105` settles with no entry slide (the route fails if one
starts), its heading intact and the Save Map help on its status line; there is
no retail still for this route.

The left side of `rmg-steady.png` differs in 13207 pixels (pointer and
preview masked):

- the Players trackbar;
- the combo boxes, which Wine mispaints in the retail stills;
- captions and labels one pixel off.

## Residuals

- **Labels:** the left labels and the Surprise Me and Generate Map captions sit
  one pixel off; those two buttons lack the family's +1 window.
- **Trackbar:** the Players trackbar's value plaque (a visible red bar) and
  track differ. This is the trackbar chain, shared with `0x102`.
- **Map preview:** the native window is 145x113; VERA20k scales the preview
  into 144x112.
- **`0x6B` residuals:** the status text one pixel low, the captions one pixel
  off, and the list darkening (see the Choose Map note).
- **Seed browser:** `0xB7`, `0x2B4` and `0x2B5` do not slide yet and draw their
  heading without a reveal. This is the next chain.
- **Timers and RNG:** the route arms the kind-1 timers of `0x694` and `0x695`
  (started by `0x4EE`, killed at the target count) and the slide engine's
  30 ms tick. It makes no RNG draws or detach calls of its own; the map
  generator's draws are unchanged.
