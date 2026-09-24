# Shell slide transitions: native evidence

Bounded evidence for how the main-menu family dialogs (`0xE2`, `0x100`,
`0x101`, `0x129`) leave the screen and what they paint while their buttons
slide, and for the Exit confirmation that follows `0xE2`. Retail `gamemd.exe`
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

The slide engine uses the same stagger and loop bound in both directions:
30 ms ticks, slot `s` starts on tick `s + 1`, `N + 9` ticks for `N` slots. The
two directions differ only in the frame ramp: out ramps count up to the empty
slot, frame 10. The engine draws only SDBTNANM button frames. Between ticks it
calls the audio/theme service `0x00406F70` and `Sleep(30)` (`0x00607F11`). It
dispatches no messages, so no child window paints while it runs. The slide-out
ends with `SendMessage(dialog, 0x4ED)` (`0x00607FAE`); the slide-in ends with
`0x4EC`.

The dialog repaint that precedes the slide-out covers every child. In paint
mode 1, `0x00621E90` draws the right panel (RightPanel__Draw) and the dialog
background (`Background_Overlay`) into the dialog's own surface and blits that
surface over the whole client rectangle (`0x00887310`). The children draw onto
the same screen surface in their own WM_PAINT, and none of them runs before the
dialog is destroyed. So during a slide-out the heading, status line, version
line, monitor, RA2TS movie, list box and prompt are all blank. Each shows the
right panel or background art underneath; the monitor spot carries the right
panel's own baked warning image (the same art the Exit backdrop shows).

## Painting during the entry slide

`ShellDialog__SlideIn` `0x00608260` runs in this order:

1. It checks the same gates as the slide-out.
2. It plays Rules `+0x1A0` (`GUIMoveInSound`, retail `MenuSlideIn`).
3. It disables the dialog.
4. It runs the engine with `DL = 1`.
5. It restores the enable state and invalidates the whole dialog (`0x00608364`).

Both slides bracket the engine with the child enumeration `0x00606800`. With
lParam 1 it sets `+0xBC` on each qualifying child's record (`0x006071AD`); with
lParam 0 it clears it. A window whose record has `+0xBC` set only validates in
the common handler (`0x00622C2B`), so the heading and status line are not drawn
while a slide runs. The RA2TS movie starts with the SHOW completion `0x4EC`,
after the slide.

During the entry slide the screen therefore shows:

- the shell backdrop where the movie would be;
- the button frames without captions;
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

## VERA20k

- `ShellExit` in `app::frontend::shell_transition` owns the running teardown
  slide: kind, `ShellFrameWave::new_slide_out` and the result to commit.
- `App::leave_shell_dialog` starts it when the dialog is showing steady and
  plays `GUIMoveOutSound`. Otherwise it commits at once, like `0x00608070`'s
  early returns.
- `App::drive_shell_exit` commits the result after the last tick.
- Input is blocked while the slide runs.

Routes that now slide out:

| Dialog | Results |
|---|---|
| `0xE2` | Single Player, Movies & Credits, Options, Exit |
| `0x100` | Main Menu, Skirmish |
| `0x101` | every result |
| `0x129` | Play Movie with a selection, Back |

During either slide, the renderers draw the shell backdrop in the movie
rectangle and frames without captions, and the RA2TS movie clock does not
advance.

- **Entry slide:** no heading or status line, a frozen monitor, and the version
  line and list box as painted.
- **Slide-out:** only the backdrop, the right panel art and the frames. No
  heading, status line, version line, monitor frame, list box or prompt.

The Exit confirmation paints the shuttered backdrop (`render::shell_paint::
paint_shuttered_tiles`) instead of the main menu, and the `0xE2` instance is
invalidated when it opens, so Cancel builds a new one that slides in.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` with the cursor at (400, 300), compared with
`tools/shell_capture_diff.py`:

- **`main-menu-0xe2-exit-confirm` vs `exit-confirm.png`:** 0 differing pixels
  outside the message box, including the backdrop and all shuttered tiles.
  Inside the box, the text and both buttons match retail exactly when shifted
  up one pixel. The animated PUDLGBGN lightning is at a different phase.
- **`main-menu-0xe2-slide-out-tick-13` vs `exit-confirm.png`:** the last
  slide-out frame before state 6 has 0 differing pixels outside the message box
  (cursor masked), except the three plate tiles between Movies & Credits and Exit
  (18,900 pixels). That exception is expected: the dialog repaint draws plates,
  and state 6 then redraws every tile shuttered (RightPanel__Draw parameter 0).
  The five emptied slots, the blank heading, status and version line, the
  monitor art and the backdrop all match.
- **`main-menu-0xe2-slide-out-tick-<N>` and `movie-list-0x129-slide-out-tick-<N>`**
  hold the production teardown slide at tick `N`; they were inspected at ticks 0,
  6 and 13 (`0xE2`) and 0, 5 and 10 (`0x129`).
- **`main-menu-0xe2-entry-sequence` vs `to-mm.png`:** the movie area matches
  within one unit in every frame (cursor excepted). The final slide frame's
  right panel is within 9 units: 3651 pixels, mostly 1 unit, in the plates
  between buttons. Retail's own slide and steady captures differ by the same
  amount there. The version line differs as in steady state.
- **Steady checkpoints** are unchanged from the status-line change:
  `movies-0x101-steady` 0, `credits-roll-frame-434` 0, `movie-list-0x129-selected`
  25 pixels at one unit (explained in the right-panel statics note), and
  `main-menu-0xe2-steady` only the version line.

## Evidence levels

- **Native behavior established:** teardown order, gates, sound field, wait loop,
  paint-path slide, the repaint that blanks the children, the slide-capable
  list, and the state 6 backdrop, template and results, all from instructions.
- **Rust regression tested:** slide-out ramp and `0xE2` schedule
  (`ui::shell::slide`), teardown completion (`shell_transition`), the retail
  `[AudioVisual]` cues through the production reader (`rules::ruleset`), and the
  `0x120` layout (`ui::shell::modal`).
- **Parity demonstrated:** bounded to the comparisons above. Mid-slide-out
  frames have no retail capture. Their schedule is the tested entry schedule,
  and what they leave blank rests on the paint-path reading above, which the
  last `0xE2` frame matches against the retail backdrop.

## Residuals

- **Message box one pixel low.** The `0x120` message box content (text,
  buttons) sits one pixel lower than retail.
- **Post-slide lull.** After an entry slide, retail can show the frame-1 slots
  without captions or movie for a moment before the first full paint. The
  trigger is not traced.
- **Other slide-capable dialogs.** Skirmish `0x102` and Options `0xD5` are
  slide-capable in retail. VERA20k slides `0x102` in but not out, and slides
  `0xD5` neither way.
- **Single Player substitute panels.** New Campaign and Load Saved Game still
  open substitute panels over `0x100`, so they do not slide it out.
- **Unported routes.** Network and Internet have no ported route, so there is no
  teardown there.
- **Keyboard flush.** `WWKeyboard__Clear` has no counterpart: VERA20k blocks
  input during the slide instead of dropping queued keys.
