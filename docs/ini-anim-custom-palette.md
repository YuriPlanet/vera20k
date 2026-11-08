# AnimType `CustomPalette=`

An ART `[ANIM]` section can name its own palette file. The name is a *file*,
extension and all, and it is looked up without a theatre suffix.

```ini
[MCVMARK]                     ; an [Animations] type's ART section
CustomPalette=markpal.pal
```

The file replaces both the owner's selected ColorScheme and the global
`anim.pal` for every frame of that animation. A missing or unreadable file
leaves the ordinary arm in place rather than drawing nothing. An empty value is
an omitted key.

The animation is a plain convert: gamemd builds no ColorScheme for it, so the
SHP's index band 16..31 keeps the custom file's own colours instead of taking
the owner's house ramp. Mental Omega's `MCVMARK` — the marker over a
deployable MCV — depends on that, because its green corner lights are indices
16 and 22. Mental Omega writes 541 `CustomPalette=` values, most of them
`unittem.pal` or `handshld.pal`.

## Native sites

- The AnimType's `CustomPalette` buffer is 25 bytes, filled by the ART read; the
  name is not resolved until the draw asks for a convert.
- `DrawAll`'s animation arm hands that convert to `CC_Draw_Shape` and skips both
  the selected scheme and `ANIM.PAL`; a tiled or fogged draw picks its own
  palette literal and keeps it.

## Where VERA20K owns it

- `rules/art_data.rs`: `AnimTypeRuntimeConfig::custom_palette` keeps the name
  verbatim.
- `render/sprite_atlas.rs`: the atlas decodes each named file once, then
  `sprite_palette_for_key` answers `SpritePaletteChoice::ExtraBuilding` for a
  type that names one, and the house remap is skipped for that choice.

## Residuals

- The hardware-cursor and placement-preview paths keep their own palette
  choices; neither draws an AnimType's `CustomPalette=`.
- A `CustomPalette=` on a type that also sets `Tiled=yes` yields the tiled
  `anim.pal` arm, as the native draw reads its literal first.
