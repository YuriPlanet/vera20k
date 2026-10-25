# Optional Phobos sidebar positions

Build with `--features phobos` to choose the hardcoded sidebar coordinate
table per side, independently of the SIDE MIX artwork.

```ini
[SOMESIDE]                    ; side section in rulesmd.ini
Sidebar.GDIPositions=         ; boolean
```

The first side in `[Sides]` defaults to `yes` (GDI positions); every other
side defaults to `no` (NOD positions), matching Phobos `SideExt`'s constructor
`ArrayIndex == 0`. The key changes the tab, cameo, repair/sell, top-button,
scroll and power-bar coordinates only; the side's SIDE MIX art and chrome
heights still follow the side theme.

Source: `SideExt::Sidebar_GDIPositions` and `SideExt::Initialize`
(`src/Ext/Side/Body.cpp`); hooks `0x534FA7` (`Prep_For_Side`), `0x652EAB`
(radar), `0x6A5090`/`0x6A51E9` (sidebar positions/GUI) and `0x63FB5D`/`0x6403DF`
(power bar) in `src/Ext/Side/Hooks.SidebarGDIPositions.cpp`.

## Where VERA20K owns it

- `rules/ruleset.rs` — `side_gdi_positions`, read from each side's section in
  `[Sides]` order, with the first-side default and configuration hash.
- `ui/sidebar/layout_spec.rs` — `for_theme_and_positions`, which keeps the art
  theme separate from the GDI/NOD coordinate choice.
- `app/presentation/sidebar_render.rs` — resolves the active side through the
  existing theme/side owner and installs the chosen layout.

## Residuals

- The radar aperture remains the fixed 140x108 rectangle; the native radar
  position hook is not separately modelled.
- The style follows the configured side; changing sides mid-match rebuilds the
  sidebar on the existing theme transition.
