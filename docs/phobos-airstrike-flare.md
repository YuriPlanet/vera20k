# Optional Phobos airstrike flare

Build with `--features phobos` to colour an airstrike's target line, its dot
and the target's tint per TechnoType. The line also gets Phobos's depth fix.

```ini
[AudioVisual]                 ; rulesmd.ini
AirstrikeLineColor=255,0,0    ; R,G,B, default 255,0,0
AirstrikeLineZAdjust=0        ; integer, default 0

[SOMETECHNO]                  ; a TechnoType
LaserTargetColor=             ; [ColorAdd] index, defaults to [AudioVisual] LaserTargetColor
AirstrikeLineColor=           ; R,G,B, defaults to [AudioVisual]
```

A colour needs three comma-separated channels from 0 to 255; any other text
leaves the previous value. The TechnoType keys are Phobos `Nullable`s: an
empty, `<default>` or `none` value leaves them to `[AudioVisual]`.

## Native sites

- `TechnoClass::DrawAirstrikeFlare @ 0x00705860` takes its red-to-orange shade
  from a Main `RandomRanged(190,270)` at `0x00705905`. Phobos
  `TechnoClass_DrawAirstrikeFlare_LineColor` (`0x007058F6`) resumes at
  `0x00705976`, past that draw. It draws `RandomRanged(745,1000)` from the
  same stream instead, scales the drawing techno type's `AirstrikeLineColor`
  by it for the lines and keeps the unscaled colour for the dot. It also gives
  the whole line the smaller of its two end depths plus `AirstrikeLineZAdjust`,
  so a line to a lower target no longer clips.
- Retail draws the dot only when the target end is the higher one
  (`0x0070597E`). Phobos `TechnoClass_DrawAirstrikeFlare_DotColor`
  (`0x0070597A`) always draws it.
- A building an airstrike targets ORs the strike owner type's
  `TintColorAirstrike` into its colour word (`BuildingClass_Draw_TintColor`,
  `0x0043D386`). Phobos converts the resolved `LaserTargetColor` with
  `GeneralUtils::GetColorFromColorAdd`, which keeps only the RGB565 case of
  retail's fall-through switch: `31,0,0` gives `0xF800`, not retail's
  `0xFC00`.

Source: `src/Ext/Techno/Hooks.Airstrike.cpp`, `src/Ext/TechnoType/Body.cpp`,
`src/Ext/Rules/Body.cpp` and `src/Utilities/GeneralUtils.cpp`, by Starkku.

## Where VERA20K owns it

- `rules/object_type.rs` and `rules/ruleset.rs`: the keys.
- `sim/airstrike.rs`: `Simulation::airstrike_line_color` and
  `Simulation::airstrike_laser_target_color`, which fall back from the type to
  `[AudioVisual]`.
- `app/presentation/airstrike_lines.rs`: the line and the dot.
- `app/presentation/lighting.rs`: the target's tint word, through
  `ColorAddTable::phobos_word` (`rules/color_add.rs`).

## Residuals

- Phobos's building tint hooks (`src/Ext/Techno/Hooks.Tint.cpp`, from
  `0x0043D386`) rebuild the whole colour word, including its Iron Curtain and
  Force Shield parts. VERA20K keeps retail's word for those and ports only the
  airstrike part.
- `[AudioVisual] ColorAddUse8BitRGB=` (default no), which makes
  `GetColorFromColorAdd` read 8-bit RGB, is not read.
