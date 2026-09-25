# Campaign selection `0x94`: native evidence

Bounded evidence for Single Player → New Campaign: the dialog `0x94`, what it
shows and does, and how it leaves. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), Ghidra as a navigation aid. The Ghidra
database carries the names used here.

## Route

- Single Player's New Campaign (`0x688`) writes result 8: `0x100` is torn down
  (its slide-out) and `Main__PrepareSession` runs state 8 (`0x0052DF05`).
- State 8 loads the art (`CampaignArt__Load` `0x0072D9A0`), creates `0x94`
  with proc `0x0052EC00` (`0x0052DF65`) and loops until the proc writes its
  result (`0x0052DFA2`). The loop also plays the queued hover voice once it is
  due (`0x0052DFAF`).
- It then tears `0x94` down (`ShellDialog__Teardown`, `0x0052E031`: the
  slide-out), waits up to 3000 ms while the campaign voice (handle
  `0x00A8F300`) still plays (`0x0052E036..0x0052E089`), and frees the art
  (`CampaignArt__Release` `0x0072DAA0`).
- Back (`0x686`) writes -1: state 1 recreates Single Player (`0x0052E0AF`).
- A campaign writes its index. The slider position picks Scenario `+0x610`
  and `+0x60C` (switch `0x0052E527`, table `0x0052EBE4`), and
  `ScenarioClass__Start_Scenario` `0x00683AB0` starts the campaign's first
  scenario (`0x0052E718`).

## The dialog

Template `0x94` (533x369 DLU): Back `0x686`, heading `0x694`
(`GUI:CampaignMenu`), status line `0x695`, monitor `0x71C`, emblems `0x6EA` /
`0x6EC`, captions `0x7A7` / `0x7A8`, the trackbar `0x50F`, labels `0x71E`
(`GUI:Difficulty`) and `0x670`, and a list `0x455` with Load `0x40E` that stay
hidden while `[0x00A8F7AD]` is clear (`0x0052F05C`). No writer of that byte
was found.

- **Right panel:** heading, monitor, status line and Back follow the family
  page rules (heading `(635, 9, 163, 18)`, status `(10, 578, 456, 21)`,
  monitor `(670, 47, 93, 55)`, Back `(644, 535, 156, 42)` at 800x600).
- **Column:** the slide engine's top-button count sees no visible top button
  (Load is the only one on the `0x00608CD0` list for `0x94`, and `0x0060A180`
  counts visible windows only), so the column is 0 buttons plus Back. It is
  executed with the other dialogs by
  `tools/storage_oracle/shell_slide_engine.py` (6 more cases).
- **Control rectangles:** from 800 pixels wide,
  `ShellDialog__GetControlOverrideRect` `0x00608500` gives the left-side
  controls fixed pixel rectangles, which `0x0060AF50` applies with the
  half-margins beyond 800x600 (none while a game is suspended):

  | Control | Rect at 800x600 |
  |---|---|
  | `0x6EA` Allied emblem | (141, 58, 348, 136) |
  | `0x6EC` Soviet emblem | (141, 364, 348, 136) |
  | `0x50F` trackbar | (177, 265, 272, 22) |
  | `0x71E` "Difficulty" | (177, 291, 141, 20) |
  | `0x670` difficulty name | (309, 291, 141, 20) |
  | `0x7A7` Allied caption | (10, 198, 610, 20) |
  | `0x7A8` Soviet caption | (10, 505, 610, 20) |

  640-wide screens keep the template rectangles.
- **Background:** `0x0060CF68` gives `0x94` the FSBKGDLG background and
  palette instead of MNSCRNL/SHELL.PAL. `Background_Overlay` draws it at the
  origin, centred only from 1024x768 up (`0x0072EC70`).
- **Emblems:** kind-4 statics. FSALG.SHP and FSSLG.SHP (260x136, 5 frames each)
  draw through FSALG.PAL and FSSLG.PAL (`0x0072DAC0`, `0x0072DAD0`). They are
  centred horizontally in their windows: (185, 58) and (185, 364).
- **Captions and labels:** plain yellow text statics without a reveal. The
  captions show `STT:AlliedCampaignIcon` / `STT:SovietCampaignIcon`, set on
  `0x497`, and are centred. `0x71E` is left-aligned and `0x670` right-aligned.
- **Trackbar** (`0x0061D950`): the init turns its value plaque off (`0x4AC`
  with 0) and sets range 0..2 and position OptionsClass `Difficulty`
  (`[0x00A8EB64]`; Options lives at `0xA8EB60`). Its paint:
  1. restores the area it saved under itself on its first paint, one RGB565
     unit darker (`0x00621B80`), over (w+1)x(h+1);
  2. draws the TRAKGRIP.PCX thumb at `x + 1 + 259·pos/2` (178, 307, 437);
  3. draws two bevel boxes (`0x006208F0`): the rail and a second box
     `reserve − inset` wide, the inset being 1 + (plaque on) (`0x0061E22A`).
     With no plaque that box is one pixel wide negative and adds three columns
     at the rail's right end.

  The bevel colours are `0x00C5BEA7` / `0x00807A68` (`0x00BBGGRR`).

## Behaviour

- **Hover** (`WM_NCHITTEST`, `0x0052ED60`): with no press captured, a change
  of the child under the cursor stops the old emblem (`0x4D5` frame 0,
  `0x4D4`) and restarts the 500 ms voice delay. A newly hovered emblem
  animates (`0x4D3`: 100 ms a frame, from `ShellStatic__AnimationInterval`
  `0x006033F0`) and queues its voice (AlliedCampaignSelect or
  SovietCampaignSelect). Each paint reports its next frame to the dialog
  (`0x4D8`, `0x00615A34`); the wrap to 0 stops the timer. The counter is then
  0, but nothing repaints the static, so the last painted frame stays up until
  the cursor leaves. A captured thumb or a pressed Back holds the mouse, so
  hover does not change while either is held.
- **Selecting:** a press on an emblem captures the mouse and plays
  `GUIMainButtonSound` (`0x0052EF52`). Releasing on the same emblem writes the
  campaign found by name ("all1", "sov1": `CampaignClass__IndexByName`
  `0x0046CC90`), stores the slider in OptionsClass `Difficulty` and plays the
  queued voice at once; the emblem stops but stays hovered
  (`0x0052F276..0x0052F2A3`), so moving on it queues no second voice. Any
  other release the dialog receives (its own capture, or over the background
  and the emblems) restarts the emblem under the cursor from frame 0 or clears
  the highlight (`0x0052F307..0x0052F3C0`).
- **Slider:** a press counts only below `y = h − 18`. On the thumb it
  captures; beside it, it jumps once. Positions are `x < 94` → 0, `< 180` → 1,
  else 2. Every change sends `WM_HSCROLL` with the position in `HIWORD`
  (`0x0061E672`), which names it in `0x670` (`TXT_EASY` / `TXT_NORMAL` /
  `TXT_HARD`, table `0x00822774`), and a mouse change plays GenericClick. The
  range change posted at init also sends it, so the name shows from the
  start.
- **Status help** (`0x006040B0`): emblems → their caption keys; slider →
  `STT:CampaignSliderDifficulty`; Back → `STT:CampaignButtonBack`.
- **Keyboard:** Esc and Enter arrive as IDCANCEL/IDOK and nothing handles them;
  no control has `WS_TABSTOP`.

## VERA20k

- `ui::campaign_shell` owns the dialog: layout, the hover, press, voice,
  emblem-animation and slider state (`CampaignShellState`), the difficulty
  mapping, and the right-panel spec (`CAMPAIGN_PAGE`).
- `app::shell_campaign` opens it after `0x100`'s teardown, routes input, plays
  the voices on one owner handle (a new voice stops the previous one), and
  commits Back to Single Player.
- `ShellExit` keeps a finished campaign teardown's last frame on screen while
  that voice plays, up to 3000 ms.
- `app::frontend::campaign_shell_render` paints it; the art (emblems, background,
  thumb, trackbar frame, darkened background) is `CampaignShellArt`.
- The trackbar rules (`thumb_left`, `trackbar_position_from_x`,
  `trackbar_press`) move to `ui::shell::trackbar`, shared by the launcher and
  in-game Options, the sound sliders and this dialog.
- The shared bevel (`render::skirmish_shell_chrome`) had its colours in the
  wrong channel order, and plaque-less trackbar frames skipped their second
  box. Both are fixed and pinned to executed `0x006208F0` draw lists
  (`tools/storage_oracle/shell_bevel.py`). The colour order changes every
  bevel consumer: the Skirmish, launcher Options and in-game Options trackbar
  frames (`launcher_trackbar_plain_180` and `in_game_trackbar_plain_192` also
  gain the second box) and the combo faces (`skirmish_combo_face_*`,
  `launcher_combo_face_180`, `keyboard_combo_face_207`). Retail stills of those
  screens (2026-09-24, 45 frames) show the blue-grey `(0xA7, 0xBE, 0xC5)` face
  and none of the old beige order; no per-screen diff was re-run for them.
- The egui campaign placeholder is removed.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` with the cursor at (400, 300), compared with
`tools/shell_capture_diff.py`. Masked: the cursor, the monitor (animation
phase) and the status line. The retail stills were taken windowed, where the
shell's help hit test lands on Back, so their status line shows Back's help
(`mm3.png`, taken from another window position, shows none).

| Checkpoint | Retail still (SHA-256 prefix) | Differing pixels |
|---|---|---|
| `campaign-0x94-steady` | `camp-94-steady.png` (`b25bf4af`) | 0 |
| `campaign-0x94-slider-left` (Easy) | `camp-94-slider-left.png` (`1ca59b67`) | 0 |
| `campaign-0x94-slider-right` (Hard) | `camp-94-slider-right.png` (`bf63950e`) | 0 |
| `campaign-0x94-entry-tick-17` | `camp-94.png` (`1c157743`, the entry slide's tail) | 0 |

## Evidence levels

- **Native behavior established:** the route, the proc's messages, the
  override rectangles, the art and palettes, the trackbar paint and mouse
  rules, the status help table and the keyboard behaviour, from instructions.
- **Native execution:** the slide column for `0x94`
  (`shell_slide_engine.py`); the trackbar frame bevels (`shell_bevel.py`);
  the thumb positions and mouse partitions (the launcher trackbar oracle the
  shared helpers already pin).
- **Rust regression tested:** `ui::campaign_shell` (layout, hover and voice
  timing, animation stepping, selection, slider presses);
  `render::skirmish_shell_chrome` (frames equal the executed bevel);
  `ui::shell::slide` (column golden); `app::diagnostics` (checkpoints).
- **Parity demonstrated:** the four comparisons above at 800x600.

## Residuals

- **Campaign start.** VERA20k's scenario loading is noncampaign-only. A release
  on an emblem stores the difficulty and plays the voice, but the dialog stays
  up; retail tears it down, waits for the voice and starts the first scenario
  (`0x00683AB0`). Every campaign start; the whole campaign mission start is a
  separate mechanism. The select path also clears `[0x00ABCE08]`
  (`0x0052F28B`, in-game state read by `Main_Tick` and the save loader),
  inert in the shell; the scenario difficulties from Options `Difficulty`
  (switch `0x0052E527`, table `0x0052EBE4`) belong to that start.
- **Hover animation and voice timing** come from instructions and were not
  executed: the 100 ms frame step, the 500 ms voice delay (played once the
  time is past it), the 3000 ms post-teardown voice wait, and the
  draw-then-advance order that leaves the last frame up. The retail helper
  cannot hover this dialog (its pointer recentres through the help hit test),
  so no capture shows them. Missed timer periods coalesce into one step, as
  `WM_TIMER` does.
- **640x480 and in-between sizes** are not captured. Screens 801–1023 wide
  shift the controls but not the background in retail; VERA20k follows the
  same rules but has no comparison.
- **Load list.** `0x455` and Load `0x40E` stay hidden (no writer of
  `[0x00A8F7AD]` was found); an indirect writer is not ruled out.
