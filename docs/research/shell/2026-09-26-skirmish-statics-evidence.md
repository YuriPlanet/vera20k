# Skirmish `0x102` kind-1 statics, native evidence

Bounded evidence for the four kind-1 statics of the Skirmish dialog `0x102`:
heading `0x694`, game type `0x6EC`, map name `0x5A8` and status line `0x695`.
Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), the getters and the relayout
executed under Unicorn, Ghidra as a navigation aid. The Ghidra database
carries the names and comments cited here.

## Statics

- **Kind and parameters:** all four are kind 1 (`0x00602490`). The executed
  getters (`tools/storage_oracle/shell_static_timers.py`) give 30 ms, step 1,
  range 8 for `0x694`, `0x6EC` and `0x5A8`, and 15 ms, step 3, range 16 for
  `0x695`.
- **Placement** (executed WM_INITDIALOG relayout `0x0060AAB0`,
  `0x0060C4A0` → `0x0060C0C0` → `0x0060B1D0` / `0x0060B550` → `0x0060B950`,
  `tools/storage_oracle/shell_relayout.py`, which also covers `0x6B`, `0x105`
  and `0x108`):

  | Static | 640x480 | 800x600 | 1024x768 |
  |---|---|---|---|
  | `0x694` | (475, 3, 163, 17) | (635, 3, 163, 17) | (747, 87, 163, 17) |
  | `0x6EC` | (490, 167, 136, 17) | (650, 167, 136, 17) | (762, 251, 136, 17) |
  | `0x5A8` | (490, 189, 136, 34) | (650, 189, 136, 34) | (762, 273, 136, 34) |
  | `0x695` | (10, 458, 616, 21) | (10, 578, 616, 21) | (122, 662, 616, 21) |

  The fixture creates each child at the template conversion one pixel wider
  and taller (the runtime window the retail captures measure; the conversion
  is Windows code, an input here, not executed).
  `ShellDialog__PlaceRightPanelStatic` `0x0060B1D0` insets a right-panel
  static by the record's override `+0xDC` when it is set (`0x0060B32E`),
  else by `(168 - w) / 2`; `0x0060AB71..0x0060AD16` sets 37 for `0x71C` in
  the 30 family dialogs that carry it and 14 for `0x6EC` and `0x5A8` in
  `0xBC 0xBD 0xC2 0xC9 0x102 0xBC6` (`0x0060ACC2`). The fix-up pass moves `0x102`'s heading `+1` y (`0x0060BCE4`); the
  status line takes `0x0060B550` with the 410x12 DLU template. VERA20k drew
  the three right-panel texts in hand-made rects one pixel narrower, so the
  game type and map name sat two pixels left of retail, and the status line
  vertically centred in a 615x20 rect instead of top-left in the window
  (kind-1 paint passes no vertical centring, `0x00615A81`).
- **Texts:** the `0x497` setup `SkirmishDialog__OnSetup497` `0x006AE6E0`
  sends the mode's UI name to `0x6EC` (`MPDialog__SetGameTypeStatic`
  `0x005E2EF0`, `0x006AEC86`) and the session map name `0xA8B322` to `0x5A8`
  (`MPDialog__SetMapNameStatic` `0x005E2F60`, `0x006AEC8D`) before the dialog
  shows; Use Map sends them again (`0x006ADA99`, `0x006ADAA0`).

## Lifecycle

- WM_INITDIALOG creates the statics hidden (`ShellStatic__InitKind`
  `0x0060A5B0`: `+0xA8 = 0`, count 1).
- The SHOW completion (`0x4EC`, common handler `0x006230B8`) enumerates the
  children (`0x0060AA60`) and sends `0x4EE`, which starts a kind-1 static
  (`+0xA8 = 1`, count 1, timer, invalidate) only while `+0xA8` is clear
  (`0x00615FDB`).
- Choose Map slides `0x102` out (`0x00608070`) and hides it (`0x006AD93C`)
  without destroying it; Cancel and Use Map show it again (`0x006AD973`,
  `0x006ADA72`) and it slides in again. Its statics keep `+0xA8`, so
  the new SHOW completion does not restart them: they repaint at their count,
  which after the last timer paint is the target, so the last unit is plain
  yellow instead of the trail step the first show leaves. A `0x4B2` with a
  changed text kills the timer, clears `+0xA8` and sends `0x4EE`
  (`0x00611C72..0x00611CAF`): only a new game type or map restarts.
  The earlier Choose Map note claimed every reveal restarts; corrected there.
- **Hover:** the proc runs the common handler first (`0x006AE40A`), whose
  WM_NCHITTEST writes the help (or an empty text) to `0x695`; the children's
  subclass writes it on WM_MOUSEMOVE. Every write repaints the line
  (`0x00615EF7`).

VERA20k: `ui::skirmish_shell::SkirmishStatics` owns the four statics and the
status line's help for the `0x102` instance: reset by
`App::enter_native_skirmish_from_single_player` (every production entry: from
Single Player, after a game, after the score screen's Continue; the developer
toggle aside), `show` at the entry slide's end, `hover` on every `0x102`
mouse move, committed by the frame loop after present. Before this change the
help text outlived the dialog, so a new `0x102` after a game showed the Start
Game help that retail's new, blank static does not (`sco-cont-steady.png`). The
right-panel rects come from the shared `anchor_rect` with
`LOW_HEADING_ANCHOR` and `MAP_TEXT_ANCHOR` (inset override 14); `0x6B` and
`0x105` take `LOW_HEADING_ANCHOR` for their headings too (the executed `0x6B`
relayout gives the same `(635, 3, 163, 17)`). The wipe-only duplicate reveal
(`ui::skirmish_shell::static_reveal`, `shell_text::Reveal`) is removed.

## Retail comparison

Retail stills from the helper at 800x600, retail RA2MD.INI (Battle, DC
Uprising, one Easy AI): `sk-steady.png` (first `0x102` from Single Player,
no pointer move), `sk-hover-start.png` (pointer on Start Game at
(720, 262)), `sk-hover-short.png` (pointer on Short Game at (110, 294)),
`cm-ret-steady.png` (Choose Map, then Cancel) and `sco-cont-steady.png` (the
new `0x102` after a defeat's score screen). VERA20k release captures with the
same RA2MD.INI, compared in RGB565 units:

| VERA20k checkpoint | Retail still | Heading | Game type + map | Status line |
|---|---|---|---|---|
| `skirmish-0x102-steady` | `sk-steady.png` | 0 | 0 | 0 |
| `skirmish-0x102-hover-start` | `sk-hover-start.png` | 0 | 0 | 0 |
| `skirmish-0x102-hover-short-game` | `sk-hover-short.png` | 0 | 0 | 0 |
| `skirmish-0x102-choose-map-return` | `cm-ret-steady.png` | 0 | 0 | 0 |
| `skirmish-defeat-continue` | `sco-cont-steady.png` | 0 | 0 | 0 |
| `skirmish-after-quit` | `sco-cont-steady.png` | 0 | 0 | 0 |

(Differing pixels in (635, 0)–(800, 32), (640, 160)–(800, 215) and
(0, 572)–(640, 600).) The first show leaves the last unit of each text at
trail step 1, RGB565 (31, 63, 3), in `sk-steady.png` and `sco-cont-steady.png`;
after Choose Map it is (31, 63, 0) in `cm-ret-steady.png`. Before this change
VERA20k drew all three plain yellow in every case, the heading one pixel and
the game type and map name two pixels left of retail.

`skirmish-defeat-continue` plays the production route (a real defeat, the
score screen, Continue) and `skirmish-after-quit` leaves a game through the
in-game abort; both rest the pointer on Start Game before pressing it, so the
old dialog's help was on its status line, and the new `0x102` shows a blank
one like retail. `skirmish-0x6b-steady` against `cm-6b-steady.png`:
the heading at the family rect matches (0 differing pixels).

Rest of the page (whole-frame diff, masking the pointer, the map preview and
the rows Wine mispaints): 7454 pixels differ in the trackbars and the Build
Off Ally label only (see residuals).

## Residuals

- **Map preview:** retail's `0x468` window is 145x113 (executed); VERA20k fits
  the preview into 144x112 and scales it on the GPU. The preview differs in
  6729 pixels (the known preview residual); the scaler is not ported.
- **Choose Map status line:** `0x6B`'s status text sits one pixel below the
  retail still although its executed window has the same top and height as
  `0x102`'s (unchanged from the Choose Map chain).
- **Status after Choose Map:** retail shows an empty status line after the
  return; which message empties it is not traced. VERA20k clears the help
  when Choose Map opens.
- **Trackbars:** the track interiors are one RGB565 unit brighter than
  retail and the value plaques differ; "Build Off Ally ConYards" sits one
  pixel off. Outside this chain.
- **No mode:** without a game mode native leaves `0x6EC` at its template text
  (`GUI:None`, `0x005E2EF3`); VERA20k shows "Battle".
