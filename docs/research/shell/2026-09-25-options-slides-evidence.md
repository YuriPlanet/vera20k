# Options `0xD5` from the main menu: native evidence

Bounded evidence for Main Menu → Options → Main Menu: how the launcher
Options dialog `0xD5` enters and leaves as a family page, and its status
help. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), Ghidra as a navigation aid. The
Options controls themselves were ported earlier
(`ui::main_menu_dialogs::options`).

## Route

- `0xE2`'s Options (`0x55C`) writes 5 (`0x00532065`); `0xE2` is torn down
  with its slide-out (`0x00531F0E`).
- `Main__PrepareSession` state 5 (`0x0052DDAB`) sets the next state to 0x12
  and calls `OptionsClass__ShowLauncherDialog` `0x0055FC80`, which creates
  `0xD5` with proc `OptionsLauncherDialog__Proc_D5` `0x0055FDB0` and pumps it.
- Every exit commits the controls (`OptionsClass__ApplyFromLauncherDialog`
  `0x0055FAA0`), then `ShellDialog__Teardown` slides `0xD5` out
  (`0x0055FD06`). Main Menu (`0x686`, result `0x5CB`, `0x0055FE93`) then
  writes the INI (`0x005FAD10`) and state 0x12 builds a new `0xE2`, which
  plays its own entry slide.
- Keyboard (`0x5CE`) runs dialog `0xA3` (`KeyboardOptionsDialog__Show`
  `0x005FBEF0`) and Network (`0x5CD`) dialog `0xD7`; both slide, and both
  return to a new `0xD5`, which slides in again.
- Enter and Esc arrive as IDOK/IDCANCEL; the proc ignores them.

## Family page

Session `+0x30D8` is clear at the main menu, so `0xD5` is a family page:

- **Slides:** `0x0060C540` marks `0xD5` slide-capable (`0x0060C602`). Its
  column: Keyboard and Network are the top buttons (`0x006092CB`; both
  visible), Main Menu the bottom one (`0x0060982C`), no map button or top
  panel. Executed with the other dialogs by
  `tools/storage_oracle/shell_slide_engine.py` (6 more cases).
- **During the entry slide** `0x00606800` suppresses the heading, the status
  line, the monitor and the column buttons; the left-side controls paint.
  The teardown repaint blanks every child.
- **Heading and status line** (`GUI:OptionsMenu`, `0x695`) are the family
  kind-1 statics; their reveals start when the entry slide ends (`0x4EC`,
  `0x006230B8`). Placement: heading `(635, 9, 163, 18)`, status line
  `(10, 578, 456, 21)`, buttons `(644, 199)`, `(644, 241)` and `(644, 535)`
  at 800x600.
- **Slider captions** name the position from the start. The `0x497` init
  sets each trackbar's range (`0x406`) and position (`0x405`); the trackbar
  sends `WM_HSCROLL` whenever its range, minimum or position changes
  (`0x0061E609..0x0061E6AF`), and the proc's handler (`0x0055FF68`) renames
  detail (`TXT_LOW`/`TXT_HIGH`, table `0x0082A248`), difficulty
  (`TXT_EASY`..`TXT_HARD`, `0x0082A254`) and scroll (`TXT_SLOWEST`..
  `TXT_FASTEST`, `0x0082A260`). The template captions (`GUI:HigherDetail`,
  `GUI:Harder`, `GUI:Faster`) never show; retail stills show "High",
  "Normal" and "Slow" on a fresh open.
- **Status help** (`0x00604729..0x00604838`):

  | Control | Key |
  |---|---|
  | Keyboard `0x5CE` / Network `0x5CD` / Main Menu `0x686` | `STT:MainOptButtonKeyboard` / `…Network` / `…Back` |
  | Visual details `0x52B`, difficulty `0x50F`, scroll `0x52A` | `STT:MainOptSliderVisual` / `…Difficulty` / `…Scroll` |
  | Music `0x52F`, sound `0x532`, voice `0x536` | `STT:MainOptSliderMusic` / `…Sound` / `…Voice` |
  | Tooltips `0x602`, target lines `0x601`, hidden `0x604` | `STT:MainOptCBoxTooltips` / `…TargetLines` / `…Hidden` |
  | Resolution `0x6ED` | `STT:MainOptComboModes` |

## VERA20k

- `OPTIONS_PAGE` (`ui::main_menu_dialogs::options::shell`) gives `0xD5` the
  family right-panel places; `shell_status_help_key` names the control under
  the pointer.
- `ShellSlideKind::Options` slides `0xD5` in after `0xE2`'s slide-out; Main
  Menu slides it out (`ShellExitThen::OptionsBack`) and then commits the
  controls and writes the profile.
- The renderer paints the slide column (skirmish chrome) in place of the
  buttons, suppresses the heading, status line and monitor while sliding,
  and blanks every child during the slide-out. Its heading, status line and
  monitor are now the shared family statics; the Options-only heading reveal
  and its present receipt are removed.
- The slider captions start at the position names; the unused template
  captions are removed from the label table. (The earlier design assumed
  they showed until the first thumb move.)

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` with the cursor at (400, 300). Retail
stills: cnc-ddraw windowed at desktop (560, 200), Main Menu → Options
(prefix `Screenshots/`, `yuri's_revenge_2026-09-25_11-31-*`). The two profiles hold different
option values, so the left-side controls `(0, 0, 632, 570)` are masked with
the cursor and the monitor; 114,645 pixels are compared (right panel,
column, heading, status line).

| Checkpoint | Retail still (SHA-256 prefix) | Differing pixels |
|---|---|---|
| `options-0xd5-steady` | `opt-d5-steady.png` (`ff12123b`) | 0 |
| `options-0xd5-entry-tick-17` | `opt-d5.png` (`34829ad3`, the entry slide's tail) | 0 |

The tail differs from the settled page by 16,028 pixels in the column, so it
pins the slide. By eye the left side matches for equal values: the captions
read "High" and "Normal" at the same thumbs; scroll and volumes differ with
the profiles.

## Evidence levels

- **Native behavior established:** route, commit/slide/write order, slide
  suppression, reveal start, status help, keyboard, from instructions.
- **Native execution:** the slide column (`shell_slide_engine.py`).
- **Rust regression tested:** `ui::main_menu_dialogs::options::shell`
  (family places, status help), `ui::shell::slide` (column golden),
  `app::diagnostics::shell_capture` (checkpoints).

## Residuals

- **Keyboard and Network.** `0xD5` does not slide out before Keyboard `0xA3`
  or Network, and `0xA3` does not slide; Network `0xD7` is not implemented
  (VERA20k reopens `0xD5` at once). A new `0xD5` after Keyboard does slide
  in. Each Keyboard/Network visit.
- **Commit timing.** VERA20k commits the controls after the slide-out;
  retail commits before it and writes the INI after. Not visible.
- **Monitor during the entry slide** is left to the panel art (suppressed by
  `0x00606800`); not captured.
- **Resolution combo.** Retail under Wine/cnc-ddraw shows no mode list
  (the proc skips filling it when the mode enumerator `0x004A4900` returns
  nothing, `0x005601C3`), so both retail stills lack the combo; VERA20k
  lists the host's modes. Capture-environment difference, not compared.
