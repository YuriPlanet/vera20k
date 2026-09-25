# Shell slide transitions: native evidence

Bounded evidence for how the main-menu family dialogs (`0xE2`, `0x100`,
`0x101`, `0x129`) and Skirmish `0x102` leave the screen, what the slide engine
animates and what the screen shows meanwhile, and for the Exit confirmation that
follows `0xE2`. Retail `gamemd.exe`
SHA-256 `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`;
control flow read from instructions (Capstone), Ghidra as a navigation aid.
The Ghidra database carries the names used here.

## Which dialogs slide

`ShellDialog__ConfigureMode1ForKnownResource` `0x0060C540` sets record `+0xB0 = 1`
(paint mode 1) and `+0xBD = 1` (slide-capable) for a fixed list of dialog ids.
The list includes the main-menu family, Options `0xD5` and its children, Skirmish
`0x102`, Load `0xB7`, Campaign `0x94`, Score `0x108` and the LAN/WOL dialogs.
`ShellDialog__DisableSlides` `0x006083E0` clears `+0xBD`. Its only caller is
LoadDlg (`0x00559435`), around an error message box.

## Leaving a dialog

Every PrepareSession result that ends a family dialog calls
`ShellDialog__Teardown` `0x00622720`:

1. `WWKeyboard__Clear` `0x0054F720` on `[0x0087F770]` drops queued keys.
2. `ShellDialog__SlideOutAndWait` `0x00608070`:
   - It does nothing in a network session (`0x0069BBE0`), for a dialog without a
     record, when `+0xBD` is 0 or `+0xB0 != 1`, or when the window is hidden.
   - Otherwise it plays Rules `+0x19C` at volume 1.0. That field is
     `[AudioVisual] GUIMoveOutSound`, read at `0x006694C7`; retail value
     `MenuSlideOut`.
   - It saves the enable state, disables the dialog and enumerates the children
     with `0x00606800` (lParam 1).
   - It sets `+0xBE = 1`, invalidates the dialog, then pumps messages and the
     main tick until `+0xBE` is cleared or 5000 ms pass.
   - Finally it enumerates the children again (lParam 0) and restores the enable
     state.
3. `DestroyWindow`.

The slide itself runs in the paint path. The common handler `0x00622B50` first
paints the dialog (`0x00621E90`). Then, when `+0xBE` is set (`0x00622C7C`), it:

- sends `0x4E2` to the RA2TS static `0x71A`, which stops the movie;
- runs `ShellDialog__RunSlideAnimation` `0x006071E0` with `DL = 0` (slide-out
  frames);
- clears `+0xBE`.

`ShellDialog__RunPendingSlideOut` `0x00607FD0` does the same for dialogs that
paint in their own procs.

The dialog repaint that precedes the slide-out covers every child. In paint
mode 1, `0x00621E90` draws the right panel (RightPanel__Draw) and the dialog
background (`Background_Overlay`) into the dialog's own surface and blits that
surface over the whole client rectangle (`0x00887310`). The children draw onto
the same screen surface in their own WM_PAINT, and none of them runs before the
dialog is destroyed. So during a slide-out the heading, status line, version
line, monitor, RA2TS movie, list box and prompt are all blank. Each shows the
right panel or background art underneath; the monitor spot carries the right
panel's own baked warning image (the same art the Exit backdrop shows).

## The slide engine animates the whole column

`ShellDialog__RunSlideAnimation` `0x006071E0` animates every right-panel tile
row, not only the buttons:

- **Rows.** The row count is `[0x00B0FA20]` (tile rows from the rect
  initializer `0x0072EC70`): 9 at 800x600 and 1024x768, 6 at 640x480.
- **Counts.** Two child enumerations count the visible top buttons
  (`0x0060A180`, through the `0x00608CD0` list) and the bottom button
  (`0x0060A250`, through the `0x00609730` list: Exit `0x3EE` on `0xE2`, `0x686`
  on the pages and the movie list, `0x5C0` on Skirmish).
- **Schedule.** The array (`0x00607646..0x006076A4`) gives regular row `c` the
  start tick `c + 1`. The first rows are the top buttons (group A: SDBTNANM
  10 → 10..5 → 1 in, 1 → 5..10 → 10 out); the rest are empty tiles (group B:
  10 → 16..11 → 0 in, 0 → 11..16 → 10 out). The bottom button is drawn over the
  last tile row with its own start at `rows`.
- **Loop bound.** The top panel's slot at `rows + 3` is always the maximum, so
  the loop runs `rows + 9` ticks whatever the dialog: 18 at 800x600. The last
  ticks repeat the settled frames: this is the pause retail shows after an
  entry slide before the first full paint.
- **Map button and top panel.** With record `+0xD6` (map button) the regular
  rows start one tile lower and SDMPBTN runs 6 → 0 (in) or 1 → 6 (out) from
  tick 0; a map button closed by a slide-out is drawn under the top panel.
  With `+0xD5` (top panel) the top shows SDTP 0 (in) or 1 (out) until
  `rows + 3`, then SDTP 1 with the warning display SDWRNTMP 5 → 0 (in) or 0 → 5
  (out) over it. Only Skirmish-family dialogs set these; nothing moves
  position.
- **Between ticks.** It calls the audio/theme service `0x00406F70` and
  `Sleep(30)` (`0x00607F11`). It dispatches no messages, so no child window
  paints while it runs.
- **End.** The slide-out ends with `SendMessage(dialog, 0x4ED)` (`0x00607FAE`);
  the slide-in ends with `0x4EC`.

The schedule, the draws per tick and the end message were executed under
Unicorn for each family dialog's counts and flags at 640x480, 800x600 and
1024x768 in both directions (`tools/storage_oracle/shell_slide_engine.py`,
24 cases). The child classifiers were not executed: the counts per dialog are
supplied from the classifier lists above.

## Painting during the entry slide

`ShellDialog__SlideIn` `0x00608260` runs in this order:

1. It checks the same gates as the slide-out.
2. It plays Rules `+0x1A0` (`GUIMoveInSound`, retail `MenuSlideIn`).
3. It disables the dialog.
4. It runs the engine with `DL = 1`.
5. It restores the enable state and invalidates the whole dialog (`0x00608364`).

Both slides bracket the engine with the child enumeration
`ShellDialog__SetSlideChildSuppressProc` `0x00606800`. With
lParam 1 it sets `+0xBC` on each qualifying child's record (`0x006071AD`); with
lParam 0 it clears it. A window whose record has `+0xBC` set only validates in
the common handler (`0x00622C2B`), so the heading and status line are not drawn
while a slide runs. The RA2TS movie starts with the SHOW completion `0x4EC`,
after the slide.

During the entry slide the screen therefore shows:

- the shell backdrop where the movie would be;
- the column opening row by row, buttons without captions;
- no heading and no status line (not started yet);
- whatever the other children painted when the dialog was shown: the version
  line and the monitor's first frame.

Retail stills show this. `to-mm.png` is taken during the `0xE2` entry slide.
`list-back.png` is taken at the end of the `0x101` entry slide, after Back on
`0x129`. Both show a dark map backdrop in the movie area, captionless frames, no
heading and no status line, and on `0xE2` the version line and monitor.

## Exit confirmation (state 6)

Exit is a `0xE2` result, so `0xE2` slides out and is destroyed before state 6
runs (`0x0052DDBA`). State 6 then:

- loads `GUI:ExitAreYouSure`, `TXT_OK` and `GUI:Cancel`;
- paints the empty shell backdrop with `Shell__DrawEmptyBackdrop` `0x0052FEC0`.
  This goes through `0x0072E820` and RightPanel__Draw `0x0072E450` with
  parameter 0 (`0x0052FF52`), which draws SDBTNANM frame 10 on every right-panel
  tile. The function was previously misnamed `BSurface__Constructor`.
- shows the message box `0x005D3490`. With body, OK and Cancel it uses template
  `0x120`: OK at DLU `(207, 155, 83, 15)` and Cancel at `(207, 175, 83, 15)`, so
  OK sits directly above Cancel.

The result decides what follows:

- **OK:** OptionsClass WriteToINI (`0x005FAD10`), then state 7 (quit).
- **Cancel:** backdrop again (`0x0052DE3D`), then state `0x12`, which builds a
  new `0xE2` that slides in.

## Skirmish `0x102`

PrepareSession state `0xB` (`0x0052E10F`) sets game mode 5 and calls the
Skirmish runner `0x006AE2C0` (`0x0052E168`). The runner:

1. creates dialog `0x102` with proc `0x006AE3F0`, stores the address of its
   result in the dialog's user data and shows it;
2. pumps the main loop (`0x00532100`) until the result is `0x617` (Start Game)
   or `0x5C0` (Back), or `0x00623120` reports a quit;
3. tears the dialog down with `ShellDialog__Teardown` (`0x006AE37D`), which
   slides it out like the family dialogs;
4. frees the preview object `[0x00AC1154]` if still present, calls `0x0072CF90`
   and writes the SessionClass fields (`0x006990A0`);
5. on Back only, paints the empty shell backdrop
   (`Shell__DrawEmptyBackdrop`, `0x006AE3CF`);
6. returns whether the result was Start.

On Start, Main_Game falls through to the scenario launch; on Back, state 1
builds the Single Player page, which slides in.

The Start/Back handler `0x006ACEE0` does its work before the result exists:
Start disables its button (`0x006ACF94`), validates (a failure shows a message
box and keeps the dialog), packs the session, destroys the preview and writes
the result (`0x006AD8C7..0x006AD8D5`). Packing completes before the result
is written, and the teardown slide calls no Randomizer (`0x006071E0` calls only
drawing, sound, allocation and the audio service). VERA20k therefore keeps
validation and `close_shell_transaction` (its packing, with the Scenario RNG
draws it performs) at the click, as before this change. The teardown, the
session write and the launch or return run after the slide. Where native
performs the random country and colour draws is not re-verified here; this
change does not move them.

During the slide-out the dialog repaint covers every child: player rows,
combos, checkboxes, sliders, flags, the map preview static `0x468` and the
static texts. What remains is the dialog's own paint (right panel and the
Skirmish background) under the engine's column. `0x102` sets both record
flags, so the map button closes from tick 0, the rows below it close top to
bottom (Start Game and Customize Battle `0x5AA`, the empty tiles, then Back over
the last row), and from tick `rows + 3` the warning display drops into the top
panel.

Both slides also hide the right panel's children on `0x102`:

- `ShellDialog__SetSlideChildSuppressProc` (`0x006069A4..0x00606B8E`) makes the
  heading `0x694`, the preview `0x468`, the map statics `0x6EC` and `0x5A8` and
  (through `0x00601360`) the status line `0x695` validate-only while a slide
  runs.
- Every tick the engine blits the tile column (`0x00607EC8`) and, with `+0xD5`,
  the top panel (`0x00607EF1`) from the dialog's own surface to the screen. The
  preview, the map statics and the three buttons lie inside those rects, so
  whatever they painted before the slide is overdrawn.

The left-side controls are not on the list and lie outside the blits, so
during the entry slide they show what they painted when the dialog was shown.

## VERA20k

- `SlideColumn` in `ui::shell::slide` is the engine's schedule: for each tick,
  the SDBTNANM frame of every tile row and the top-panel art. The row count
  comes from `right_panel_rects` for the running resolution; the counts and
  flags per dialog come from `RENDERED_SHELL_SLIDES`. Both slide directions and
  every renderer use it.
- `render::shell_paint::paint_slide_column` paints the column on the main menu,
  the pages, the movie list and credits. The Skirmish renderer paints the
  column and the top-panel art (SDTP, SDWRNTMP, SDMPBTN) in `chrome.rs`.
- `ShellExit` in `app::frontend::shell_transition` owns the running teardown
  slide: its `ShellFrameWave::new_slide_out` and the result to commit (the
  result names its dialog).
- `App::leave_shell_dialog` starts it when the dialog is showing steady and
  plays `GUIMoveOutSound`. A dialog that is not showing steady commits at once,
  like `0x00608070`'s early returns; a second result while one slides out is
  ignored.
- `App::drive_shell_exit` commits the result after the last tick. It is the
  first step of the frame prelude, before the next dialog is armed and before
  the surface is acquired, so the frame after the last slide-out tick is the
  next screen's first frame (tick 0 of its entry slide, or the movie, credits
  or Exit confirmation). An exit whose dialog stopped showing (another route
  replaced it) is dropped without its result.
- Input is blocked while the slide runs.

Routes that now slide out:

| Dialog | Results |
|---|---|
| `0xE2` | Single Player, Movies & Credits, Options, Exit |
| `0x100` | Main Menu, Skirmish |
| `0x101` | every result |
| `0x129` | Play Movie with a selection, Back |
| `0x102` | Start Game (after validation), Back from the Single Player route |

During either slide, the renderers draw the shell backdrop in the movie
rectangle and the column without captions, and the RA2TS movie clock does not
advance.

- **Entry slide:** no heading or status line, a frozen monitor, and the version
  line and list box as painted. On Skirmish the right panel shows only the
  slide art (no captions, preview, start markers or map statics); the left-side
  controls are drawn.
- **Slide-out:** only the backdrop, the right panel art and the column. No
  heading, status line, version line, monitor frame, list box or prompt; on
  Skirmish no player rows, options, preview or texts.

The Exit confirmation paints the shuttered column (`shuttered_column`: every
row at frame 10) instead of the main menu, and the `0xE2` instance is
invalidated when it opens, so Cancel builds a new one that slides in.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` with the cursor at (400, 300), compared with
`tools/shell_capture_diff.py`. The retail stills are cnc-ddraw PrintScreen
captures of retail YR at 800x600 (SHA-256 prefixes: `to-mm.png` `71ad979c`,
`exit-confirm.png` `41cf16c1`, `mm3.png` `b7449cb1`, `camp-94.png` `1c157743`).

- **`main-menu-0xe2-exit-confirm` vs `exit-confirm.png`:** 0 differing pixels
  outside the message box, including the backdrop and all shuttered tiles.
  Inside the box, the text and both buttons match retail exactly when shifted
  up one pixel. The animated PUDLGBGN lightning is at a different phase.
- **`main-menu-0xe2-slide-out-tick-17` vs `exit-confirm.png`:** the last
  slide-out frame has 0 differing pixels outside the message box (cursor
  masked), plates included: the slide-out closes every row to frame 10, which is
  what state 6's backdrop draws. So VERA20k's last slide-out frame hands over to
  the state 6 screen without a visible change. It is not evidence of what retail
  shows during the slide-out, because state 6 repaints the whole screen
  (`0x0052DE06`, and the message box again at `0x005D3514`); that claim rests on
  instruction reading only.
- **`main-menu-0xe2-entry-sequence` vs `to-mm.png`:** the three settled tail
  ticks (15, 16 and 17) match retail with 0 differing pixels over the whole
  frame (cursor masked), so `to-mm.png` was taken in the engine's tail. The movie
  area matches in every tick; the earlier ticks differ only in the right panel,
  where the column is still opening.
- **`movie-list-0x129-back-first-frame`:** Back on the movie list, read back on
  the first frame after the teardown commits. The harness requires that frame
  to be the recreated `0x101`'s entry slide at tick 0, and it is: every row at
  frame 10, backdrop, no captions.
- **`<dialog>-slide-out-tick-<N>`** holds a production teardown slide at tick
  `N`. Inspected: `main-menu-0xe2` at 17, `movie-list-0x129` at 9, and
  `skirmish-0x102-back` at 0, 6, 13 and 17 (children gone from tick 0, the map
  button closed by tick 6, the warning display dropping in from tick 12).
- **`skirmish-0x102-entry-tick-<N>`** holds the Skirmish entry slide, opened
  from Single Player. Inspected at 0, 6, 13 and 17: the right panel shows only
  the slide art (no captions, preview, map statics or heading), the status line
  is blank, and the left-side controls are drawn.
- There are no retail captures of these Skirmish and slide-out ticks.
- **Steady checkpoints** are unchanged from the status-line change:
  `movies-0x101-steady` 0, `credits-roll-frame-434` 0, `movie-list-0x129-selected`
  25 pixels at one unit (explained in the right-panel statics note).
  `main-menu-0xe2-steady` is now 0: the version line takes the runtime window
  correction (163x17 at `(635, 583)`), the shared cause the heading, monitor and
  status line already carried.

## Evidence levels

- **Native behavior established:** teardown order, gates, sound field, wait loop,
  paint-path slide, the repaint that blanks the children, the slide-capable
  list, the Skirmish runner and handler order, the child classifier lists, and
  the state 6 backdrop, template and results, all from instructions.
- **Native execution:** the slide engine's per-tick draws (shape, frame,
  position), loop bound and end message, for the family and Skirmish counts and
  flags at three resolutions in both directions (Unicorn, 24 cases,
  `tools/storage_oracle/shell_slide_engine.json`).
- **Rust regression tested:** `SlideColumn` against that executed golden, with
  positions from `right_panel_rects` (`ui::shell::slide`); the teardown start
  rule, result ownership and completion (`shell_transition`);
  commit-before-acquire ordering (`app::frame`); column rows at the button
  rects (`main_menu_shell_render`); the capture checkpoints
  (`app::diagnostics`); the retail `[AudioVisual]` cues through the production
  reader (`rules::ruleset`); and the `0x120` layout (`ui::shell::modal`).
- **Parity demonstrated:** bounded to the comparisons above: the entry tail
  ticks and the last slide-out tick at 800x600 on `0xE2`. Other slide frames
  have no retail capture; their draws rest on the executed engine and what
  they leave blank on the paint-path reading.

## Residuals

- **Message box one pixel low.** The `0x120` message box content (text,
  buttons) sits one pixel lower than retail.
- **Classifier counts supplied.** The oracle runs the engine with each dialog's
  button counts supplied from the `0x00608CD0` and `0x00609730` lists; the
  classifiers themselves (and their runtime conditions) were not executed. The
  counts equal the visible owner-draw buttons of each retail dialog template,
  and the `0xE2` counts are also confirmed by the entry-tail capture. A
  miscounted dialog would open the wrong rows as buttons on every slide of it.
- **Skirmish entry left side.** No retail capture shows a `0x102` entry tick.
  VERA20k draws the left-side controls during it, as retail's `0xE2` and `0x94`
  entry captures show for their unsuppressed children (`to-mm.png`,
  `camp-94.png` SHA-256 `1c1577431281d5d2…`).
- **Tall windows.** `right_panel_rects` caps the tile rows at 9; native divides
  without a cap (`0x0072EE92`), so heights 641–767 give 10–13 rows there and a
  longer column. The production shell runs at 800x600 or 640x480, so only
  `--shell-capture` sizes reach it.
- **Other slide-capable dialogs.** `0x0060C540` also marks Options `0xD5`,
  Load `0xB7`, Score `0x108`, Choose Map `0x6B` and the in-game menu dialogs
  (`0xB5`, `0xB6`, `0xB8`, `0xBBA`, `0xBBB`). None of them slides here yet.
- **Skirmish Back backdrop.** After Back's slide-out, retail paints the empty
  backdrop (`0x006AE3CF`) before state 1 shows `0x100`. VERA20k goes straight to
  `0x100`'s entry slide, whose first tick shows the same shuttered rows over the
  same backdrop; only the monitor spot can differ, for about one frame.
- **Developer Skirmish routes.** Back from the developer Skirmish shells (no
  Single Player route) closes at once without a slide.
- **Quit fade image.** After OK, retail destroys the message box without a
  repaint (`0x0052DE21..0x0052DE34`), so its last image most likely stays on
  screen through the state 7 fade. VERA20k shows the empty backdrop alone.
  Once per session, until the screen fades to black.
- **Hidden window.** A minimized window stalls the slide-out until it is shown
  again, then plays the remaining ticks; on Skirmish Start the match launch
  waits with it. Retail gives up after 5000 ms (`0x0060822B`) and destroys the
  dialog.
- **Single Player substitute panel.** Load Saved Game still opens a substitute
  panel over `0x100`, so it does not slide it out. New Campaign slides it out
  into `0x94` ([campaign selection note](2026-09-25-campaign-select-evidence.md)).
- **Unported routes.** Network and Internet have no ported route, so there is no
  teardown there.
- **Keyboard flush.** `WWKeyboard__Clear` has no counterpart: VERA20k blocks
  input during the slide instead of dropping queued keys.
