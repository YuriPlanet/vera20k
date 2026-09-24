# Right-panel statics: native evidence

Bounded evidence for two statics every right-panel shell dialog carries: the
animated warning monitor `0x71C` and the heading `0x694`. It covers the
main-menu family (`0xE2`, `0x100`, `0x101`, `0x129`) and the monitor of
Options `0xD5`. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow and constants read from instructions (Capstone), Ghidra as a navigation
aid.

## Monitor `0x71C`

A scan of all 98 RT_DIALOG templates finds control `0x71C` (Static, style
`0x50000007`, DLU size 61×33 at y 29) in 30 dialogs, exactly the set that the
static-kind code tests: `0x94 0xBB 0xD5 0xD6 0xD7 0xD8 0xE2 0xE6 0xE7 0xF3 0xF4
0xF5 0xFE 0x100 0x101 0x108 0x10E 0x10F 0x112 0x114 0x116 0x117 0x11C 0x11D
0x122 0x125 0x129 0x2BC 0xBC6 0xBC7`. No dialog `0xE2` control `0x71B` exists
(`0x71B` appears only in `0x73` and `0x122`), so the main menu has no website
static.

- Image: `0x006038C7..0x00603A00` returns SDWRNANM.SHP for `0x71C` in those
  dialogs; the palette path `0x00603776` routes it through `0x0072E2A0`
  (SHELL2).
- Initialization `0x0060A982..0x0060AA0B`: static kind 4 (`+0x70`), frame
  `+0x98 = 0`, `+0x9C = GetTickCount()`, `+0xA0 = 0x00603240(...) = 1`, then
  `SetTimer(hwnd, 0, 1)` and `+0xA8 = 1`.
- WM_TIMER `0x00615C49..0x00615D0F`: when `GetTickCount() − +0x9C > +0xA0`
  it invalidates the static; the first time (`+0x94 == 0`) it kills the timer,
  stores the SHP frame count (`word [shp+6]`, 91) in `+0x94` and re-arms the
  timer at that many milliseconds.
- WM_PAINT `0x006159EB..0x00615A17`: draws frame `+0x98`, then advances it
  (`+1`, wrapping to 0 at `+0x94`); while `+0x94` is still 0 the advance
  wraps immediately, so frame 0 is drawn until the first timer.
- Placement: kind-4 paint centers the shape in the static's window only along
  an axis where the window is larger (`0x006157B5..0x006157E0` stores the
  window size, `0x0061595E..0x0061597E` compares it). The runtime window is
  one pixel wider and taller than the 6×13 resource conversion (the same
  compatibility correction the `0xE2` heading already carried), and `0x71C`
  takes the inset override 37 (`0x0060AC99..0x0060AD16`, read by
  `0x0060B1D0`): `x = 800 − 37 − 93 = 670` and the 92×53 frame centers at
  `(670, 48)` in the `(670, 47, 93, 55)` window. (The `(w+1)×(h+1)` buffer
  at `0x0061560F` only saves and restores the background.)
- The first-paint slide runs inside the dialog's first WM_PAINT
  (`0x00612690 → 0x00608260 → 0x006071E0`: `AudioSystem__Pump(); Sleep(0x1E)`
  per step, no message dispatch), so the static gets no WM_TIMER until the
  slide ends. It shows frame 0 during the slide and its pending startup timer
  fires right after it; the animation clock starts at the slide end.

Retail captures (cnc-ddraw, 800×600, listed in the Movies & Credits evidence
note plus `mm-now.png`, `sp-0x100.png`, `sp-0x100-settled.png`): every frame's
monitor region equals an SDWRNANM frame rendered through SHELL2.PAL at
`(670, 48)`; the 90×51 interior matches exactly in all ten captures (the
outer ring differs only because the asset renderer draws a marker there).
Entry frames (`mc-0x101.png`, `sp-0x100.png`, `list-0x129.png`, `to-mm.png`)
show frame 0 during the slide; `credits-esc.png` and `sneak-esc.png` show the
`0x101` heading at reveal count 9 with the monitor at frame 2, a clock that
started at the slide end; settled frames show frames 38–69, consistent with
one frame per 91 ms.

## Heading `0x694`

- Kind 1 through `0x00602490` for the right-panel dialogs; timer interval
  30 ms (`0x00600CA0`, `0x00600F08`), step 1 (`0x006015E0` default), highlight
  range 8 (`0x006023DA`). Executed under Unicorn for `0xE2 0x100 0x101 0x129
  0xD5 0x102 0xB7` by `tools/storage_oracle/shell_static_timers.py`, which also
  returns the `0x71C` kind-4 startup timer 1 (`0x00603240`); Rust tests read its
  reference file.
- Fix-up pass `0x0060B950`: for these dialogs the heading moves `+7` y and grows
  `+1` h (`0x0060BD17..0x0060BD31`) unless a network session is active
  (`0x0069BBE0`); only `0xBC 0xBD 0x102 0xC2 0xC9 0xBC6 0x105 0x6B 0x113` take
  `+1` y instead (`0x0060BCE4`).
- The text is drawn in the window, which carries the same one-pixel
  compatibility correction, so the effective rectangle is `(635, 9, 163, 18)`
  at 800×600 for all four dialogs, the rectangle `0xE2` already used. The
  reveal starts when the dialog's first-paint slide completes.
- Placement `0x0060B1D0` (no network session): `x = W − inset − w − dx`,
  `y = resource y + dy`, with `dx = max(0, (W − 800) / 2)` and
  `dy = max(0, (H − 600) / 2)`; the panel art only moves from height 768, so
  at heights 601..767 these statics sit below their art as in retail.

## Heading colours

The reveal colours are the encoded COLORREF interpolation of the BITFONT
Path-A print (`src/render/shell_text_reveal.rs`; yellow toward white over the
8-unit trail). The shell batch shader
multiplies a UI tint into the *encoded* texel (`palette_light` in
`src/render/palette_light.wgsl`), so the Path-A glyph tint is the encoded byte
over 255. Before this change the tint was additionally linearized, which darkened
every trail step (terminal step blue 30 reached the surface as 3, i.e. RGB565
unit 0 instead of 3).

The final paint is the one before the count reaches `len + range + 1`, so the
last unit keeps trail step 1 (`0xFFFF1E`, RGB565 `(31, 63, 3)`): retail
`mm3.png`, `sp-0x100-settled.png` and `options-0xd5-settled.png` show it. A later
repaint without a timer (for example after the window is re-activated) paints
count `len + range + 1` and turns that unit fully yellow, as in `mm-now.png`;
VERA20k keeps the last timer paint.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture` at the neutral cursor, against the retail
captures (cnc-ddraw), with `tools/shell_capture_diff.py`:

| Checkpoint | Native | Masks | Differing pixels |
|---|---|---|---|
| `movies-0x101-steady` | `mc-0x101-settled.png` | cursor, RA2TS movie, `0x71C` window | 0 |
| `movie-list-0x129-steady` | `list-0x129-settled.png` | cursor, `0x71C` window, status | 0 |
| `movie-list-0x129-selected` | `list-row-click.png` | cursor, `0x71C` window, status | 0 |
| `main-menu-0xe2-steady` | `mm3.png` | cursor, RA2TS movie, `0x71C` window | 416, all in the version line `0x71D` (x 669..761, y 586..596) |

The masked `0x71C` window of every capture equals one SDWRNANM frame (rendered
through SHELL2.PAL by the asset tool) at `(670, 48)` over the 90x51 interior:
frames 5, 7, 2 and 2 for the four captures, counted from each dialog's slide
end (before the slide freeze they were 9, 11, 5 and 5). Retail
`mc-0x101-settled.png` and `options-0xd5-settled.png` show frames 46 and 50 at
the same origin.

Not covered: Options `0xD5` has no production capture checkpoint (its monitor
takes the same window correction); heights other than 480/600/768 have unit
tests only; the version line's one-pixel offset.

Follow-ups outside this change: the RA2TS static `0x71A` timer (`0x65`) is
equally frozen during a slide but VERA20k steps the movie; returning from
Options rebuilds `0xE2` (state 5 → `0x12`, `0x0052DDAB`) with a new slide,
heading and monitor while VERA20k keeps the old instance; score dialog `0x108`
carries `0x71C` but draws none; `0xD5` is slide-eligible (`0x0060C540`) but has
no first-paint slide in VERA20k.

## Status line `0x695` (recorded for the next change)

- Kind 1 for dialogs accepted by `0x00601360` while no network session is
  active (`0x00602AA5..0x00602AC7`); interval 15 ms (`0x0060134E`), step 3
  (`0x00601D02..0x00601D14`), range 16 (`0x006023C8..0x006023D8`).
- Placement `0x0060B550`: `(Δx + 10, H − h − Δy − 1, w, h)` with the
  DLU-converted window size.
- Kind-1 paint `0x00615A81..0x00615AE8` passes print flags `0x10`/`0x11`/`0x12`
  only (left/center/right from the window style); `0x00621040` centers
  vertically only for flag 4, so the text is top-aligned.
- Hover text reaches the static as message `0x4B2` from the dialog handler
  (`0x00622CCB..0x00622E83`); the shared subclass stores it and, when it
  changed after the reveal started, stops the timer and sends `0x4EE`
  (`0x00611BC1..0x00611CAF`), restarting the reveal.
