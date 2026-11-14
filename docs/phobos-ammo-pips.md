# Phobos Ammo pip INI keys

Four `TechnoType` keys read next to the already-ported `EmptyAmmoPipFrame`
(`Ext/TechnoType/Body.cpp:1003`, constructor `-1` at `Body.h:667`). All four
sit in the same `TechnoTypeExt` record and feed `TechnoClass_DrawPips` in
`Ext/Techno/Hooks.Pips.cpp`; VERA's owners are `src/rules/object_type.rs`
and the Ammo arm of `app/presentation/ui_overlays.rs::scale_pip_draws`.

| Key | Phobos read | Constructor (`Body.h:666-670`) | Consumer | VERA |
|---|---|---|---|---|
| `AmmoPipFrame` | `Body.cpp:1002` | `13` | `Hooks.Pips.cpp:286` (`ammoFrame`) | filled frame replaces the hardcoded 13 |
| `AmmoPipWrapStartFrame` | `Body.cpp:1004` | `14` | `Hooks.Pips.cpp:249` (`startFrame`) | passed to the wrap and unwrapped arms |
| `AmmoPipSize` | `Body.cpp:1005` | empty `Nullable<Point2D>` | `Hooks.Pips.cpp:172` (`size`) | `Some([x,y])` overrides the strip step |
| `AmmoPipOffset` | `Body.cpp:1006` | `{0,0}` | `Hooks.Pips.cpp:241` (`pipOffset`) | translates every pip |

## Behaviour

- The unwrapped Ammo strip (`0x70A36E`, `Hooks.Pips.cpp:280-305`) draws
  `GetPipMax` slots when `EmptyAmmoPipFrame >= 0`, else stops at the fill
  count. Filled slots use `AmmoPipFrame`; the rest use `EmptyAmmoPipFrame`.
- `TechnoClass_DrawPips_Sizes` (`0x709B2E`, `Hooks.Pips.cpp:156-180`)
  chooses one size pair for the whole DrawPips call from the type's
  `PipScale` and the drawn class: `Pips.Ammo[.Buildings].Size` for
  `PipScale=Ammo`, `Pips.Generic[.Buildings].Size` otherwise. The Ammo arm
  then replaces it with the type's `AmmoPipSize` when that nullable is set.
  The constructor defaults reproduce the stock `4` / (building ? `2` : `0`)
  step, so an unset `[AudioVisual]` keeps the old drawing.
- `AmmoPipOffset` is added once to the strip's draw origin
  (`Hooks.Pips.cpp:241`); in VERA's replayed `ScalePipDraw` that is the same
  pair on every pip's `offset_pixels`, applied after the slot step.
- `PipWrap > 0` takes the wrap arm (`0x70A421..4E6`,
  `Hooks.Pips.cpp:245-279`): `PipWrap` slots, each showing the wrap level
  counter above `AmmoPipWrapStartFrame` (start-frame default 14). A slot whose
  counter is still at or above the raw `Ammo` count steps the level down until
  it is below; a level below zero keeps the bare start frame. The Phobos arm
  advances the origin before each draw, so the wrapped strip starts one step
  in. In a Phobos build zero or negative `PipWrap` keeps the unwrapped strip;
  a stock build enters on any nonzero wrap and draws nothing when negative,
  as vanilla `0x70A374`/`0x70A42C` do.

## Global size pairs

Four `[AudioVisual]` keys (`Ext/Rules/Body.cpp:160-163`, constructor
`Body.h:620-623`) supply the fallback step; each is a `Valueable<Point2D>`
read with `read_phobos_int_pair` (short reads fill only the leading slot, a
failed conversion keeps the whole pair):

| Key | Default |
|---|---|
| `Pips.Generic.Size` | `4,0` |
| `Pips.Generic.Buildings.Size` | `4,2` |
| `Pips.Ammo.Size` | `4,0` |
| `Pips.Ammo.Buildings.Size` | `4,2` |

The generic pair also sizes the spawn-pip row (when `SpawnsPipSize` is unset)
and the occupant/passenger rows, which is why the pair is selected once per
call rather than per arm. In a stock build `pip()` keeps the built-in
`4`/(building ? `2` : `0`) step and the Ammo arm keeps its own
`AmmoPipSize` override.

Readers use the native pairing helpers in `rules/ini_value.rs`:
`read_phobos_int_pair` for the `Valueable<Point2D>` offset and globals,
`read_phobos_nullable_int_pair` for the `Nullable<Point2D>` size (only a full
two-integer read sets it, and `<default>`/`none` clear it). Each read is
gated with `#[cfg(feature = "phobos")]` and falls back to the constructor
value in a stock build, so the three feature sets keep identical drawing.

## Residuals

- `ScalePipDraw.frame` is a `usize`; a negative authored `AmmoPipFrame`
  falls back to the stock 13, and a negative `AmmoPipWrapStartFrame` draws
  frame 0, instead of a negative frame index.
- The global pairs are plain `[i32; 2]` in `PhobosGlobals`; a partial read
  writes the leading slot and keeps the default elsewhere, matching
  `Parser<int, 2>` rather than re-deriving a Point2D.

## Tests

`src/app/presentation/ammo_pip_tests.rs`, all through the production
`scale_pip_draws` reader/draw path and `#[cfg(feature = "phobos")]`:

- `an_authored_ammo_pip_frame_replaces_the_stock_thirteen`
- `the_ammo_pip_wrap_start_frame_reads_its_authoring_and_constructor`
- `the_ammo_pip_size_changes_the_strip_step_only_when_authored`
- `an_authored_ammo_pip_offset_translates_every_pip_in_the_strip`
- `the_pip_wrap_arm_counts_the_wrap_levels`
- `the_global_ammo_size_drives_the_strip_step`
- `the_building_ammo_size_global_drives_the_building_strip`

Global readers/defaults: `the_pip_size_globals_default_and_read_from_audio_visual`
in `src/rules/phobos_globals_tests.rs`. Generic-pair consumers:
`spawn_pips_fall_back_to_the_global_generic_size`,
`the_global_generic_size_drives_the_passenger_strip`.

The existing `an_authored_empty_ammo_frame_continues_the_strip_to_capacity`
and the retail Harrier replay are unchanged.
