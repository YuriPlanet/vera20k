# ToolTip.Background.*

Phobos lets a side tint and fade the tooltip's backing rectangle instead of
the stock opaque black fill.

```ini
[AudioVisual]
ToolTip.Background.Color=0,0,0     ; ColorStruct, default black
ToolTip.Background.Opacity=100     ; int 0..100, default 100
ToolTip.Background.BlurSize=0.0    ; float, default 0

[Side]                             ; per-side overrides, all nullable
ToolTip.Background.Color=10,20,30
ToolTip.Background.Opacity=40
ToolTip.Background.BlurSize=2.5
```

Readers: the `[AudioVisual]` trio at `Ext/Rules/Body.cpp:169-171` (constructors
`Body.h:722-724`); the `[Side]` `Nullable` trio at `Ext/Side/Body.cpp:48-50`
(constructors `Body.h:44-46,75-77`). All six use the Phobos parsers: a
three-byte colour (`$`/`h`/`%hu`, every component 0..255 else retain), an int
and a strtod float, each retaining the current value on unreadable text.

## Native body

`CCToolTip_Draw2_FillRect @ 0x478FDC` (`Misc/PhobosToolTip.cpp:420-443`)
resolves each field from the player side's `SideExt`, else the rules globals,
and calls `FillRectTrans(rect, color, opacity)`. `FillRectTrans` clamps the
opacity with an unsigned compare against 100 (`0x4BB94A..0x4BB958`), so a
negative int reads as full opacity. When the `[Phobos] ToolTipBlur` client
switch is on it then calls `BlurRect(rect, blurSize)`. With the constructor
defaults the fill is opaque black — the stock tooltip backing.

## VERA

`PhobosGlobals::tooltip_background_color/opacity/blur_size` read the
`[AudioVisual]` trio with the Phobos readers;
`rules::phobos_tooltip_background::TooltipBackgroundSideSettings` owns the
three nullable `[Side]` overrides, read per registered side in `[Sides]`
order, and `RuleSet::side_tooltip_background` resolves them (Phobos builds
only). `tooltips::tooltip_background` turns the result into the fill quad's
tint and 0..1 alpha (`opacity / 100` after the same unsigned 0..100 clamp),
and `tooltip_quads` paints it behind the text; with no overrides the draw is
the stock opaque black, and non-Phobos builds keep it unconditionally.

## Tests

- `src/rules/phobos_globals_tests.rs` —
  `the_tooltip_background_globals_default_and_read` (defaults, overrides,
  junk-retains, above-100 stored).
- `src/rules/ares_side_defaults_tests.rs` —
  `phobos_side_tooltip_background_reads_explicit_overrides` (explicit values,
  unset side, `<none>` clear).
- `src/app/input/tooltips.rs` —
  `tooltip_background_resolves_globals_and_side_overrides` (globals, side
  fallback, opacity 0, the unsigned clamp for 101 and -5) and
  `a_configured_tooltip_background_keeps_its_tint_and_alpha`.

The opacity percent conversion and the unsigned clamp were each mutated red
and restored.

## Residuals

- The `ToolTipBlur` client switch's blur pass (`CCToolTip::BlurRect`) is not
  implemented; `BlurSize` is read for parity only. Trigger: the switch is on.
  Frequency: none by default (the switch constructs off), and the constructor
  blur is 0.
- `FillRectTrans` quantizes alpha as `opacity * 255 / 100` and blends with a
  `>> 8` shift, so even 100 is 254/255 coverage; VERA draws exact alpha
  (a one-level difference, invisible).
