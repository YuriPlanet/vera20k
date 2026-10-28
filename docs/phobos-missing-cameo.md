# Optional Phobos missing cameo

Build with `--features phobos` to replace the placeholder the sidebar shows for
a type whose cameo art is missing.

```ini
[AudioVisual]                 ; rulesmd.ini
MissingCameo=XXICON.SHP       ; filename, including the .shp or .pcx extension
```

The default is the stock `XXICON.SHP`, which the game loads for a type whose
own cameo does not load. `none` or `<none>` clears the name, which leaves the
stock file. Phobos's two hooks then decide by the lowercased name:

| Name | Path |
| --- | --- |
| not `XXICON.SHP`, containing `.shp` | the configured SHP replaces the loaded `XXICON.SHP` handle when that file loads (`Replace_XXICON_With_New`: `0x715A4D`, `0x716D13`, `0x6CEE31`, `0x6CE8AA`) |
| containing `.pcx` | over the stock `XXICON.SHP`, `StripClass_Draw_DrawMissing` (`0x6A99F3`) blits the PCX into the 60x48 cell when that PCX loads |
| anything else, or a file that does not load | the stock `XXICON.SHP` |

The PCX arm draws over the stock handle, so it needs `XXICON.SHP` to load;
without it the type draws nothing. A name carrying both extensions tries the
`.shp` arm first, then the `.pcx` arm.

The key applies to TechnoType cameos and to SuperWeaponType icons; Phobos hooks
both `ReadINI`/`Load` pairs. A type that does have its own cameo art is not
affected.

## Where VERA20K owns it

- `rules/native_processing.rs`: each rules pass reads `MissingCameo` before its
  type bodies, as `RulesExt::ExtData::LoadBeforeTypeData` (`0x679A15`) does,
  through the 32-byte `PhobosFixedString` buffer. A later pass whose value is
  empty keeps the earlier one, and a scenario reset keeps the value.
- `rules/ruleset.rs`: `GeneralRules::missing_cameo`, published from that
  retained scalar and defaulting to `DEFAULT_MISSING_CAMEO`. Exposed as
  `RuleSet::missing_cameo`.
- `render/sidebar_cameo_atlas.rs`: `render_cameo` tries the declared,
  image-derived and legacy SHP candidates first and only then
  `render_missing_cameo`; `render_superweapon_cameo` does the same for an icon
  with no `SidebarImage=`. `render_missing_cameo` applies the two arms above,
  and `render_pcx_cameo` is shared by Ares `CameoPCX` and this key.

## Residuals

- A build without the capability keeps the previous behaviour: a type with no
  cameo art has no atlas entry, and the cell stays background. The stock
  `XXICON.SHP` fallback itself is not modelled outside the Phobos build.
- The SHP arm runs when a type's cameo loads, and gamemd keeps that handle
  (`+0x6F0`) through later passes. A `MissingCameo=` that only a later layer
  (the mode INI or the map) sets can therefore miss types whose cameo loaded
  earlier. VERA20K draws the final value for every type.
- A type whose own `Cameo=` names `XXICON` loads the stock handle too, so
  Phobos's draw arm also puts a configured `.pcx` over it. VERA20K draws that
  type's `XXICON.SHP`.
- `AltCameo=` keeps its own lookup with no fallback, matching the native
  `+0x1F8` field, which the hook does not touch.
- Every type without art gets its own copy of the placeholder in the cameo
  atlas.
- The PCX arm draws on Ares's 60x48 canvas convention rather than re-deriving a
  destination from the surface, because VERA composites the sidebar atlas
  instead of blitting into the native `SidebarSurface`.
