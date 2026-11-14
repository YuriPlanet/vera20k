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
  replaces the pip width/height used as the X/Y step with
  `AmmoPipSize.Get(size)`. VERA has no `[General] Pips_Ammo_Size` or
  `Pips_Ammo_Buildings_Size` globals, so `None` keeps the stock `4` /
  (building ? `2` : `0`) step of the shared `pip()` closure.
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

Readers use the native pairing helpers in `rules/ini_value.rs`:
`read_phobos_int_pair` for the `Valueable<Point2D>` offset and
`read_phobos_nullable_int_pair` for the `Nullable<Point2D>` size (only a full
two-integer read sets it, and `<default>`/`none` clear it). Each read is
gated with `#[cfg(feature = "phobos")]` and falls back to the constructor
value in a stock build, so the three feature sets keep identical drawing.

## Residuals

- `ScalePipDraw.frame` is a `usize`; a negative authored `AmmoPipFrame`
  falls back to the stock 13, and a negative `AmmoPipWrapStartFrame` draws
  frame 0, instead of a negative frame index.
- No `[General] Pips_Ammo_Size` / `Pips_Ammo_Buildings_Size` globals are
  ported, so `AmmoPipSize` overrides VERA's built-in step rather than the
  native global fallback.
- `AmmoPipSize` changes only the Ammo-scale arm's step; the shared `pip()`
  closure keeps its stock step for every other PipScale.

## Tests

`src/app/presentation/ammo_pip_tests.rs`, all through the production
`scale_pip_draws` reader/draw path and `#[cfg(feature = "phobos")]`:

- `an_authored_ammo_pip_frame_replaces_the_stock_thirteen`
- `the_ammo_pip_wrap_start_frame_reads_its_authoring_and_constructor`
- `the_ammo_pip_size_changes_the_strip_step_only_when_authored`
- `an_authored_ammo_pip_offset_translates_every_pip_in_the_strip`
- `the_pip_wrap_arm_counts_the_wrap_levels`

The existing `an_authored_empty_ammo_frame_continues_the_strip_to_capacity`
and the retail Harrier replay are unchanged.
