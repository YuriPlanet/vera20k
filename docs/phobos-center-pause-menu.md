# Optional Phobos centred pause-menu background

Build with `--features phobos` and set `[Sidebar] CenterPauseMenuBackground=yes`
in `uimd.ini` to place the active-scenario modal background at its own size at
the centre of the battlefield. Default is `no`, where VERA stretches the art over
the whole battlefield instead, since VERA's frames take any size
(`InGameShellLayout::with_battlefield_background`).

```ini
[Sidebar]
CenterPauseMenuBackground=false  ; boolean, default no
```

`0x0072FC60` sizes the background from the resolution and publishes it to
`0x00B0FC30` as `(0, 0, canvas)`. The switch moves that rectangle's corner to
the centre of the battlefield: the frame less the 168-pixel right column and the
32-pixel bottom strip. Retail `BKGDSM/MD/LG[Y].SHP` fills that area exactly at
640x480, 800x600 and 1024x768, so the stock sizes neither move nor stretch;
the switch matters at other frame sizes or with art sized differently from the
frame, where the canvas keeps its size and is cropped, or framed by black,
evenly on both sides.

Source: the `[Sidebar]` read in `src/Phobos.INI.cpp` (default `false`) and the
`InitSideRectangles_CenterBackground` hook at `0x72FCB5`
(`src/Ext/Sidebar/Hooks.cpp`). That address is the `mov [0x00B0FC30], eax` which
stores the rectangle `0x0072FC60` just filled; the hook rewrites the X/Y of the
rectangle `eax` points to before the store runs.

## Where VERA20K owns it

- `rules/phobos_pause_menu.rs` — reads the key from the loaded `UIMD.INI`.
- `ui/shell/in_game_shell.rs` — `InGameShellLayout::with_centered_background`,
  the hook's arithmetic, applied to the layout `0x0072FC60` produces.
- `app/loading/init_helpers.rs` — reads the switch next to the superweapon
  sidebar settings from the same file.
- `app/frontend/skirmish_shell_render/in_game_shell.rs` — applies it when an
  active-game shell lays out, so the pause menu, Game Controls, Abort and
  Diplomacy all share the placement `0x72F540` paints.

## Residuals

- The background still clips to the whole frame, as `0x72F540` draws it: art
  wider than the battlefield runs under the right column, which paints after it,
  rather than being cut at the column's edge.
