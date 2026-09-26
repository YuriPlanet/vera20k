# Shell trackbar `0x0061D950`, native evidence

Bounded evidence for the shell's slider control, `ShellTrackbar__WndProc`
`0x0061D950`, the subclass of every shell `msctls_trackbar32`: Skirmish
`0x102` (Game Speed, Credits, Unit Count), Generate Map `0x105` (Players
`0x3EB`), Options `0xD5` (six sliders), campaign `0x94` (difficulty) and the
in-game B8/BBB. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions; the pointer and thumb arithmetic executed under
Unicorn (`tools/storage_oracle/launcher_trackbar.py`); Ghidra as a navigation
aid. The Ghidra database carries the names and comments cited here.

## Record and arithmetic

- Record fields: `+0xE8` button down, `+0xEC` dragging, `+0xF0` range,
  `+0xF4` position relative to the minimum, `+0xF8` minimum, `+0xFC` stored
  thumb offset, `+0x100` step (`0x4AB`, default 1), `+0x104` plaque flag
  (`0x4AC`, default on), `+0x108` cue off (`0x4AE`).
- `span = client W - reserve - 13`, reserve 50 with the plaque and 0 without.
  The stored offset is `span * pos / range`; the thumb's left edge is
  `1 + offset` for both the paint and the press test.
- A pointer at `x` selects `clamp(x - 6, 1, W - reserve - 12)`, then
  `(t - 1) * (range + 1) / span`, capped at the range and stepped:
  `((v + min) / step) * step - min`. `TBM_GETPOS` (`0x0061E4AD`) returns
  `((min + pos) / step) * step`.
- The first message initializes the record from the common control's own
  range and position (`0x0061DA85..0x0061DB8E`). A zero range would fault at
  the division (`0x0061DB4A`) before the fallback to 100 (`0x0061DB54`).
- Every message replaces a zero step with 1 and turns the plaque on
  (`0x0061DB94..0x0061DBAD`). The first message has computed its span
  without the plaque by then; the owners' setters recompute it.
- `TBM_SETPOS` (`0x0061E486`) ignores a value outside `min..max`.
  `TBM_SETRANGE` (`0x0061E59A`) keeps the old relative position through
  `min(pos, range)`, then `max(pos, min)` against the absolute minimum. Both
  recompute the stored offset. A fresh window set up with a saved value
  outside the range therefore shows `min + max(min(0, range), min)`: retail
  Credits 10000, Unit Count 0, Game Speed position 0, Players 4 (executed:
  77 setup vectors in `launcher_trackbar.py`).

## Press, hold and release

- Every `WM_LBUTTONDOWN` on the window takes the mouse capture (`SetCapture`
  at `0x0061E4DF`) before any test. Only `y > bottom - 18` goes further
  (`0x0061E505..0x0061E512`): a press on the thumb `[left, left + 12)` starts a
  drag without moving it; a press beside it jumps once (`0x0061E545..
  0x0061E598`).
- `WM_LBUTTONUP`, or a move without `MK_LBUTTON`, releases the capture
  (`0x0061E444..0x0061E44E`). While dragging, every message recomputes the
  position from `GetCursorPos` with the current span.
- While a slider holds the capture, Windows sends no `WM_NCHITTEST` to the
  dialog, so its status-help walk (`0x00622CCB`, `ChildWindowFromPointEx`) does
  not run: the status line keeps the slider's help until the release, after a
  rail press as much as after a thumb grab.
- `WM_LBUTTONDBLCLK` takes the press path but releases the capture instead
  (`0x0061E4EF`): beside the thumb it jumps, on the thumb it does nothing.

## Change tail

`0x0061E609`: when the range, minimum or position changed, `InvalidateRect`,
then `WM_HSCROLL MAKELONG(5, min + pos)` to the parent (`0x0061E6AF`), then
GenericClick (Rules `+0x70C` through `0x00750920`, `0x0061E6DD`) unless
`0x4AE` turned the cue off or `0x405`/`0x406` set the value. The setters
still send `WM_HSCROLL`; the Skirmish proc has no case for it.

## Owners' ranges

- Skirmish `SkirmishDialog__OnSetup497`: Game Speed 0..6 at `6 - speed`
  (`0x006AECD8..0x006AECF7`); Credits `TBM_SETRANGE` MinMoney..MaxMoney,
  `TBM_SETPOS` the session money, step MoneyIncrement
  (`0x006AED16..0x006AED59`); Unit Count MinUnitCount..MaxUnitCount
  (`0x006AED72..0x006AED9E`). The ranges pack 16-bit words.
- `RulesClass__ReadMultiplayerDialogSettings` (`0x00671EA0`) reads the keys
  with `ReadInt` over the constructor's values: MinMoney 2500, MaxMoney 10000,
  MoneyIncrement 100, MinUnitCount 1, MaxUnitCount 20
  (`0x00667245..0x00667279`). Retail `rulesmd.ini` sets 5000, 10000, 100, 0
  and 10 (production reader test).
- Generate Map: Players `TBM_SETRANGE` 2..8 (`0x0059722C`), `TBM_SETPOS`
  NumPlayers; no `0x4AB`, `0x4AC` or `0x4AE`, so the plaque shows and every
  change clicks. Its `WM_HSCROLL` case `0x00596B04` disables Use Map and Save.

## Paint

- The first paint copies the `(W + 1) x (H + 1)` screen area under the window
  into the control's surface (`+0x10`), darkened by `AlphaBlendRect`
  `0x00621B80` with the record's `+0xC8`; every paint starts by putting that
  copy back (`0x0061DD12..0x0061DE8F`).
- TROFM (114x24, bright red rows 0 and 23) through
  `Surface__BlitWrappedCentered` `0x006BA3E0` into
  `{x + W - 49, y - 1, 50, 24}`: an opaque copy from the centred source offset
  `max(0, (src - dest) / 2)` that wraps past the source's end, so columns
  32..81. Its red rows land on the frame rows `y - 1` and `y + 22`, under the
  frames.
- TROFL (8x24) at `(x + W - 49, y - 1)`; TROFR (10x24) at
  `(x + W + 1 - 10, y - 1)` (`0x0061DFAE..0x0061DFC8`).
- TRAKGRIP (12x22) by a plain blit into `{x + thumb left, y, 12, H}`: a
  21-high window clips its last row.
- Disabled (`WS_DISABLED`, `0x0061E0B0`): `AlphaBlendRect` darkens the
  thumb's rectangle by `[0xAC4898]` (0x60), and the value text takes
  `[0xAC1CB4]` (0x9F, dark red) instead of `[0xAC18A4]` (0xFFFF, yellow;
  all set in `0x0060F9A0`). The frames get `[0xAC1CA8]` instead of
  `[0xAC4624]`, but a border-2 frame draws its bevel colours whatever the
  colour argument (`0x00620A90`), so they do not change.
- Two border-2 frames `0x006208F0`: the rail `{x, y, W - reserve, H}` and the
  value box `{x + W - reserve + inset, y, reserve - inset, H}`, inset 2 with
  the plaque and 1 without.
- The value text: `sprintf "%d"` of the stepped value (`0x0061E27A`) into
  `{right - 0x31, y, right, bottom}`, centred (`0x0061E2B6..0x0061E2DB`).

## Runtime windows

Retail stills paint the main-menu shell's sliders one pixel wider and taller
than the template conversion: Skirmish and the Options audio sliders 129x22,
the Options plain sliders 181x22, Generate Map's Players 226x22. The executed
relayout (`tools/storage_oracle/shell_relayout.py`) starts from that size and
keeps it; the growth itself is not traced. The thumbs of the six sliders
whose values VERA20k's captures share (Game Speed, Credits, Unit Count,
Detail, Difficulty, Sound) sit one pixel left of the grown span, at the
template width's (Game Speed 4 of 6 at 44, not 45): the setup's
`TBM_SETPOS` stored the offset before the growth, and the next change
recomputes it.

## VERA20k

- `ui::shell::trackbar` owns the rules: `TrackbarRange` (min, max, step;
  `set_up` for an owner's setup, `stepped` for `TBM_GETPOS`),
  `trackbar_press` (miss, hold, drag, jump), `TrackbarHold` (the capture and
  whether it drags), `thumb_left`, `value_text_rect`.
- Skirmish hydrates its saved values through `set_up` and reads Credits back
  stepped (`SkirmishShellState::credits`), for the value text, the game start
  and the saved settings (`SkirmishDialog__OnCommand`
  `0x006AD709..0x006AD79E`).
- Skirmish (`SkirmishTrackbarBounds::range`, `trackbar_hold`), Generate Map
  (`PLAYERS_RANGE`, `RandomMapSetupModalState::press_players`/`drag_players`),
  Options (`capture`) and the campaign (`slider_hold`) keep one hold each. A
  held slider keeps the status line (and the campaign's Back paint), a
  release after it acts on nothing else, and a focus loss drops the hold.
  The release capture `campaign-0x94-slider-held` presses the difficulty
  slider and holds it over Back: Back's pixels match its unlit face and the
  status line keeps the slider's help.
- The painter (`skirmish_shell_render::controls`) blits TROFM through
  `push_entry_wrapped_centered`, places TROFL and TROFR, clips TRAKGRIP, and
  takes one frame entry per window size (`TRACKBAR_FRAMES`).

## Retail comparison

VERA20k release captures at 800x600 with the retail `RA2MD.INI`
(`skirmish-0x102-steady`, `skirmish-0x105-steady`, `options-0xd5-steady`)
against `sk-steady.png`, `rmg-steady.png` and `opt-d5-steady.png`. Each
region is the slider's window plus three pixels around it. Masked: both
thumb positions, VERA20k's software cursor, and the value text where
VERA20k's profile holds another value (Music, Voice). Differences in RGB565
units:

| Slider | Compared | Differing | Largest |
|---|---|---|---|
| Skirmish Game Speed | 3182 | 1242 | 1 |
| Skirmish Credits | 3038 | 1116 | 1 |
| Skirmish Unit Count | 3494 | 1452 | 1 |
| Generate Map Players (thumb included) | 6496 | 2580 | 1 |
| Options Detail | 4950 | 3497 | 1 |
| Options Difficulty | 4950 | 3423 | 1 |
| Options Scroll | 4708 | 1489 | 1 |
| Options Music | 2174 | 1057 | 1 |
| Options Sound | 3494 | 880 | 1 |
| Options Voice | 2306 | 48 | 1 |

The one-unit differences are the darkened background VERA20k does not draw.
The previous build showed a red line inside every plaque.

## Residuals

- **First-paint thumb:** native paints each thumb from the offset its setup
  stored before the window growth, one pixel left of VERA20k's until the
  next change; a press on that column differs the same way.
- **Darkened background:** the slider interior is one unit darker in native
  (`AlphaBlendRect` of the saved background). The movies and credits lists
  already draw that darkened copy; the Skirmish family does not.
- **Disabled paint:** the thumb darkening and the disabled frame and text
  colours are not drawn. VERA20k disables a shell slider only without an
  audio device (Options audio sliders) and during a map generation.
- **Double-click:** if the class delivers `WM_LBUTTONDBLCLK`, native jumps
  without taking the mouse and never drags; VERA20k handles a second press.
- **Focus loss:** native keeps a slider's hold bytes through a lost capture
  until the next move without the button releases them; VERA20k drops the
  hold when the window loses focus.
- **In-game sliders:** the B8 sound and BBB options owners keep their older
  input: a press above the strip or on the rail does not hold the mouse, and
  hover keeps updating during a drag. Their paint follows this chain. In-game
  menus are a later chain.
- **Generate Map combos:** still 225 wide beside the 226-wide Players window
  until the combo chain grows them; their right frame edges sit one pixel left
  of the slider's.
- **16-bit ranges:** native packs MinMoney/MaxMoney and the unit counts into
  16-bit words; a mod beyond 65535 would wrap there, not in VERA20k.
- **Campaign slider:** below 800x600 its window lacks the growth.
- **Timers and RNG:** the control makes no RNG draws and arms no timers.
