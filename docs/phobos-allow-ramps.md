# Tiberium ramps

`AllowRamps=` lets a tiberium type grow and spread onto cardinal ramp cells
when its ramp overlay art exists. Stock keeps resources off every slope.

```ini
[Riparius]                    ; [Tiberiums] section
AllowRamps=yes                ; Valueable<bool>, default no
```

Reader: `Ext/Tiberium/Body.cpp:24` (field `Ext/Tiberium/Body.h:26`, default
false). The load also raises `NumSlopes` to eight when it is lower
(`Body.cpp:26`).

## Native body

`CanResourceGerminateOnRamp` (`Ext/Tiberium/Hooks.cpp:46-90`): slope 0 always
germinates; slopes 1..4 only when this type's `AllowRamps` is set and the
overlay at `Image->ArrayIndex + NumFrames` exists and is a Tiberium overlay
(west/north/east/south; corners and steep ramps have no art). The hooks keep
that admission: `0x48399C` (`CanTiberiumGerminate`, which admits an empty
cardinal ramp), `0x483650`/`0x483738` (`CanTiberiumGrow`), `0x4836CF`
(`CanSpreadTiberium`), `0x4837BB` (`SpreadTiberium`), `0x4872A0`/`0x4873A7`
(germinate spread registration), and `0x47D36E` (`RecalcAttributes`), which
clears a sloped resource overlay/index/data whenever the cell cannot
germinate.

## VERA

- `TiberiumType::allow_ramps` reads the key; the processed registry raises the
  retained image state's `slope_count` to 8 for such a type.
- `RuleSet` projects `OverlayTypeFlags::tiberium_allows_ramps` onto every
  overlay slot of an allowing type whose first ramp overlay is Tiberium art
  (`OverlayTypeRegistry::project_tiberium_ramps`). The map layer's recalc then
  clears slopes without needing the tiberium registry
  (`clears_tiberium_on_slope`).
- `tiberium_slope_admits` (`sim/tiberium/mod.rs`) answers the germination
  rule from the tile and registry; the spread admission, the existing-cell
  growth arm and the growth-queue rebuild use it.
- The empty-cell target gate (`admit_new_tiberium_target`) takes the
  germinating type's ramp flag (`0x48399C`), so a keyed type can place a new
  resource on an empty cardinal ramp; the spread processor, spawner, anim
  landing and voxel-anim paths resolve it from their type.

## Tests

- `src/rules/overlay_types.rs` — the key raises `NumSlopes` to 8, the
  projection marks the ramp slots, `allows_ramp` admits slopes 1..4 only for
  the keyed type, and the recalc flag keeps those cells.
- `src/sim/tiberium/mod.rs` — spread onto a slope-4 cell is admitted with the
  key and refused without it.
- The stock recalc oracle tests (`rules/overlay_types.rs`,
  `sim/overlay_grid.rs`, `map/authored_overlay.rs`,
  `map/resolved_terrain/tests.rs`) keep their stock expectations without the
  phobos feature and expect the Phobos clear with it.

## Residuals

- Corners and steep ramps (slope > 4) always clear, matching
  `CanResourceGerminateOnRamp`.
- The ramp-art test reads the single overlay at `Image + NumFrames`, as
  Phobos does; the remaining slope slots are not individually validated.
- Phobos also patches the ramp overlay draw (`0x47F87D`, `0x47FBE5`) and the
  `GetContainingRect` family; those rendering hooks are not ported, so the
  drawn ramp image/rect keeps the stock selection.
- The forced spread's source-side `0x4837BB` admission is unproven here; the
  target-side `0x48399C` gate is what admits the ramp.
