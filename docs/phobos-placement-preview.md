# Optional Phobos placement preview

Build with `--features phobos` to draw a building's artwork on the placement
grid. The feature follows Phobos `BuildingTypeExt` placement fields and their
two hooks: `TacticalClass_DrawPlacement_PlacementPreview @ 0x6D528A`
(`src/Ext/BuildingType/Hooks.cpp`) and
`CellClass_Draw_It_SetPlacementGridTranslucency @ 0x47EFAE`.

The preview needs three switches at once: the rules global
`[AudioVisual] PlacementPreview=yes`, the client `[Phobos]
ShowPlacementPreview=yes`, and the building type's own `PlacementPreview=yes`
(default yes).

```ini
[Phobos]                          ; RA2MD.INI, imported into VERA.INI
ShowPlacementPreview=yes          ; source default yes

[AudioVisual]
PlacementPreview=no               ; source default no
PlacementPreview.Translucency=75  ; 0|25|50|75, default 75
PlacementGrid.Translucency=0      ; 0|25|50|75, default 0
PlacementGrid.TranslucencyWithPreview= ; default = PlacementGrid.Translucency

[SOMEBUILDING]
PlacementPreview=yes                    ; default yes
PlacementPreview.Shape=                 ; .shp name, `~~~` takes the theater suffix
PlacementPreview.ShapeFrame=            ; integer; default 0, or the buildup's last frame
PlacementPreview.Offset=0,-15,1         ; X,Y,Z leptons (X/Y are pixels)
PlacementPreview.Remap=yes              ; default yes
PlacementPreview.Palette=               ; palette file name (as written) used when Remap=no
PlacementPreview.Translucency=          ; 0|25|50|75; unset inherits the global
```

The b36 names above replace the older `BuildingPlacementGrid.TranslucentLevel`,
`BuildingPlacementPreview.DefaultTranslucentLevel`,
`PlacementPreview.Show` and `PlacementPreview.TranslucentLevel`. Only the b36
keys are read. `ShowBuildingPlacementPreview` is likewise replaced by
`ShowPlacementPreview`.

## Image and palette

`PlacementPreview.Shape` names an SHP; `~~~` in the name is replaced by the
active theater extension and `.shp` is appended when missing, matching
`TheaterSpecificSHP::Read`. Without a usable Shape the building's
`Buildup=` SHP is used at `Frames / 2 - 1` (the last build-up frame before its
shadow half); without a buildup the body SHP's frame 0 is used.
`PlacementPreview.ShapeFrame=` overrides that frame.

`Remap=yes` (default) draws with the theater unit palette plus the placing
house's colour ramp, like the building drawer. `Remap=no` uses
`PlacementPreview.Palette=` when it loads, otherwise the unit palette. A
missing Shape or Palette falls back exactly as above.

The preview is centered on `Cell2Coord(cell, floor + Offset.Z)` projected to
the screen, then moved by `Offset.X/Y` pixels. The cell is the foundation
origin: the hook's `0x88095C` center plus `0x880960` offset equals the marker
origin recorded by `tools/spatial_oracle/placement_render.py`, which is the
same `BuildingPlacementPreview.rx/ry` VERA already computes. Its translucency
is the per-type value, else `[AudioVisual] PlacementPreview.Translucency`.

## Grid and drawing

`PlacementGrid.Translucency` applies while a building is being placed.
`PlacementGrid.TranslucencyWithPreview` replaces it when both
`PlacementPreview` and `ShowPlacementPreview` are on, even if the current
building sets `PlacementPreview=no` (the native grid hook does not consult the
type). The values use the native 0/25/50/75 mapping (0 = opaque, 25 = 75%
opacity, 50 = 50%, 75 = 25%).

The ghost is submitted after the PLACE cells in the placement pass, through
the existing alpha-blended surface material. The strip cache is per match and
keyed by type, authored Shape/Palette and placing house colour.

## Where VERA20K owns it

- `rules/phobos_placement_preview.rs` — the `Translucency` value (through the
  shared anim translucency table) and `[AudioVisual]` globals.
- `rules/object_type.rs` — the per-BuildingType `PlacementPreview*` fields.
- `rules/ruleset.rs` — immutable projection and config hash.
- `app/persistence/phobos_settings.rs` — the `[Phobos]` client switches.
- `app/presentation/placement_preview.rs` — SHP/palette resolution, cache and
  the screen point. The unit palette comes from the theater owner
  (`map::theater::load_theater_unit_and_iso_palettes`) and the buildup files
  from `render::sprite_atlas::buildup_shp_candidates`.
- `app/presentation/render/build_instances.rs` /
  `draw_passes.rs` — grid alpha and the preview pass.

## Residuals

- The preview uses the theater unit palette for `Remap=yes`; a building whose
  native drawer selects a separate extra building palette is not reproduced.
- Floor height comes from the placement cell's signed level; bridge decks and
  unusual height-corrected cells are not separately queried.
- `ShapeFrame` is clamped to the loaded SHP's last frame; the native clamp
  permits one past the last frame.
- The ghost draws the body/buildup image only; bibs, building animations,
  shadows and embedded shadows are not part of the placement preview.
- The client switch default follows the source (`yes`); the UI document says
  `no`. The rules global still defaults to `no`, so both must be enabled.
- gamemd draws the wall auto-fill markers (`0x6D5730`/`0x6D59D0`/`0x6D5C50`)
  after the `0x6D528A` hook; VERA draws the preview after every PLACE cell,
  which differs only while placing walls.
- No test anchors the preview image, frame or palette choice against retail
  assets or native output yet.
