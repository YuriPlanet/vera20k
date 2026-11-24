# Animation selection-bracket draw offsets

Phobos lets an animation's draw point follow its owner's selection bracket
(`Ext/AnimType/Body.cpp:82-90`, `Ext/AnimType/Body.h:37-45`, consumer
`AnimClass_DrawIt_DrawOffset` at `0x423122` with the RING1 alternate at
`0x422CD8`, `Ext/Anim/Hooks.cpp:278-345`).

```ini
[SOMEANIM]                            ; AnimationType
XDrawOffset=0                         ; int, default 0 (plain draw offset)
XDrawOffset.ApplyBracketWidth=false   ; bool, default false
XDrawOffset.InvertBracketShift=false  ; bool, default false
XDrawOffset.BracketAdjust=0           ; int, default 0
XDrawOffset.BracketAdjust.Buildings=  ; Nullable<int>, unset falls back to BracketAdjust
YDrawOffset.ApplyBracketHeight=false  ; bool, default false
YDrawOffset.InvertBracketShift=false  ; bool, default false
YDrawOffset.BracketAdjust=0           ; int, default 0
YDrawOffset.BracketAdjust.Buildings=  ; Nullable<int>, unset falls back to BracketAdjust
```

`XDrawOffset` and `YDrawOffset` themselves are the existing art offsets the
sprite atlas already bakes into every animation frame; the plain `XDrawOffset`
is read here only because the bracket X overwrite discards it natively.

## Behavior

The hook runs inside `AnimClass::DrawIt` before its body and shadow draws. It
first adds `XDrawOffset` to the location. When the animation's `OwnerObject` is
a Techno and either `ApplyBracket*` key is set, it then replaces the location's
axes from that Techno's selection bracket:

- a Building uses `GetBuildingSelectBracketPosition(..., Top)`. Y follows only
  when the ART `Height` sign matches `YDrawOffset.InvertBracketShift`
  (non-negative height with the default, negative with invert), shifted by
  `YDrawOffset.BracketAdjust.Buildings` or `YDrawOffset.BracketAdjust`. X
  always follows `ApplyBracketWidth`: the Top anchor plus
  `int(60 * (foundationWidth / 2.0) * ±1)` (right unless inverted), the same
  nullable adjust, minus the plain `XDrawOffset`;
- every other Techno (Infantry, Unit, Aircraft) uses
  `GetFootSelectBracketPosition(Anchor(invert ? Left : Right, Top), infantry)`.
  Y follows only when `PixelSelectionBracketDelta`'s sign matches the invert
  flag, shifted by `PixelSelectionBracketDelta + YDrawOffset.BracketAdjust`. X
  is the anchor plus `XDrawOffset.BracketAdjust`, minus `XDrawOffset`.

When `ApplyBracketHeight` is set and the owner has a shield whose bar draws
(`!IsBrokenAndNonRespawning && (HealthRatio > 0 || !Pips.HideIfNoStrength)`),
the location's Y also takes `BracketDelta - 3` while the shield's
`BracketDelta` sign matches the invert flag.

The attach-effect `Animation.DrawOffsetN` accumulation is added after these
overwrites, so the two features compose in native order.

## VERA shape

`rules::phobos_anim_draw_offset::PhobosAnimDrawOffset` reads the keys and hangs
off `AnimTypeRuntimeConfig`. The free-anim Display caller
(`app/presentation/instances/overlays.rs`) resolves the overwrite through
`anim_draw::phobos_draw_offset_point` before adding the attach-effect offset;
`anim_draw::phobos_bracket_draw_point` holds the gates and arithmetic. The
bracket anchor and its projection reuse the existing owners:
`digital_display::building_bracket` (`GetBuildingSelectBracketPosition`),
`digital_display::screen_point` and `ui_overlays::foot_select_bracket`.

Building slot animations (`BuildingClass::CreateAnimForSlot`, `0x451890`) store
absolute coordinates and have no `OwnerObject` (the native capture's stored
location is the absolute slot position), so the hook does not move them.

## Residuals

- The type keys read through the project's Phobos `Valueable<int>` reader while
  the sprite atlas bakes art `XDrawOffset` through the native `ReadInt`; the
  two parsers only differ on `$`/`h`-prefixed text in that art key, so a
  bracket X write could subtract a different value than the atlas added.
  Trigger: such a token in an animation's art `XDrawOffset` with
  `XDrawOffset.ApplyBracketWidth=yes`. Effect: a small X error on the owned
  animation. Frequency: custom art only.
- The Building branch's production dispatch (`Structure` -> Building anchor,
  ART `Height` sign, shield delta) is exercised only at the
  `phobos_bracket_draw_point` unit level, not through the Display caller; the
  foot branch has the integration test.

Tests: `rules::phobos_anim_draw_offset::tests` (reads, defaults, Nullable
fallback) and `anim_draw::tests` (both owner arms, invert gates, building
adjust, shield delta, and one attached animation through the production
Display caller).
