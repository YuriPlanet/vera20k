# Custom selection box

Phobos adds a `[SelectBoxTypes]` type class and a TechnoType `SelectBox=`
reference, so a selected object can draw a custom SHP box and ground shape in
place of (beside) the stock selection bracket. Sources:
`New/Type/SelectBoxTypeClass.{h,cpp}` (16 fields, constructor defaults, INI
read order), `Ext/TechnoType/Body.cpp:999-1000` (`SelectBox=` /
`HideSelectBox=`), `Ext/Rules/Body.cpp:333-334` (`[AudioVisual]` defaults) and
`Ext/Techno/Body.Visuals.cpp:333-409` (`TechnoExt::DrawSelectBox`), called by
`Ext/Techno/Hooks.Pips.cpp:3-20,74-114` (hooks `0x6D9076`/`0x6D9134` before the
body, `0x6F65D1`/`0x6F683C` from the health-bar phase).

```ini
[SelectBoxTypes]
0=MYBOX

[MYBOX]
Shape=                   ; SHP, .shp appended when absent
Palette=                 ; palette file, PALETTE.PAL when unset
Frames=                  ; nullable X,Y,Z health-band frames
Offset=0,0               ; box offset from the techno point
Translucency=0           ; 0 / 25 / 50 / 75
VisibleToHouses=all      ; owner,allies,enemies,neutral,team,others,all,none
VisibleToHouses.Observer=yes
DrawAboveTechno=yes
GroundShape=
GroundPalette=
GroundFrames=
GroundOffset=0,0
Ground.AlwaysDraw=yes
GroundLine=no
GroundLineColor=0,255,0  ; with .ConditionYellow / .ConditionRed
GroundLine.Dashed=no

[UNIT]                   ; TechnoType
SelectBox=MYBOX          ; nullable type reference
HideSelectBox=no

[AudioVisual]
DefaultInfantrySelectBox=
DefaultUnitSelectBox=
```

## Behavior

- The box resolves `SelectBox=` first, else the infantry default for infantry,
  else the unit default for any non-building. `HideSelectBox=` and the
  `[Phobos] EnableSelectBox=` client switch (default no) gate it.
- `VisibleToHouses=` follows the `CanTargetHouse` ladder against the viewing
  house; the observer reads `VisibleToHouses.Observer=` instead.
- The health band picks `Frames.X` above `[AudioVisual] ConditionYellow=`,
  `Frames.Y` above `ConditionRed=`, else `Frames.Z` (strict `>`; default
  `1,1,1` for infantry and `0,0,0` otherwise). `Translucency=` is exactly
  0/25/50/75, anything else opaque.
- `DrawAboveTechno=yes` draws after the body at the techno point plus the
  class offset plus `PixelSelectionBracketDelta`; `=no` selects the
  before-body pass.
- For a non-building, `GroundShape=` draws with `GroundFrames=` at the ground
  point plus `GroundOffset=` when `Ground.AlwaysDraw=` or the body is airborne.

## Where VERA20K owns it

`rules::select_box_type` owns the catalog (`SelectBoxTypeCatalog::apply_pass`,
the native read order) and `SelectBoxGlobals`; `RuleSet::select_box_index_for`
is the resolution. `ObjectType::from_section` reads `SelectBox=`/
`HideSelectBox=` through `phobos_techno_extras`. The
`[Phobos] EnableSelectBox=` client switch lives in
`app::persistence::phobos_settings`. `render::selection_overlay` loads each
entry's `Shape=`/`Palette=` and `GroundShape=`/`GroundPalette=` once per match.
`app::presentation::select_box` owns the frame/point/visibility decisions and
builds the sprites; `render::build_instances` attaches them to the parent's
Extras strip, and `draw_plan_lowering`/`render::object_extras` draw them with
the same material as the DigitalDisplay custom shapes.

## Residuals

- `DrawAboveTechno=no` (before-body) has no custom-SHP slot in this engine's
  before pass, which carries surface lines only; such a box is not drawn.
- `GroundLine=`/`GroundLineColor=`/`GroundLine.Dashed=` are not drawn; only the
  ground SHP is. The `Damageable` color owner and reader are in place.
- A custom `Palette=`/`GroundPalette=` name is loaded through the same overlay
  SHP path but only the default `PALETTE.PAL` binding is exercised; a missing
  file keeps the entry's box absent.
- The ground point uses the object's projected coordinate; the locomotor
  `Shadow_Point` offset is not modeled.
- `SelectBox=` keeps an unknown name (it resolves at draw time to no box);
  natively an unknown name retains the previous pointer.
