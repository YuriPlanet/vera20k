# Tiled.Interval / Tiled.AlignToCenter

A `Tiled=yes` animation draws frame 0's picture repeatedly up the screen. The
stock tiler takes the frame height from the SHP, starts half a frame above the
draw point and steps each row by the frame height while its Z falls by half a
frame plus the height. Phobos can replace that height and start the column on
the draw point.

```ini
[ANIM]                             ; AnimType / art section
Tiled=yes
Tiled.Interval=0                   ; integer, default 0 (use the frame height)
Tiled.AlignToCenter=false          ; boolean, default false
```

Readers: `Ext/AnimType/Body.cpp:128-129` (after `Tiled`), constructor 0/false
at `Ext/AnimType/Body.h:83-84,135-136`.

## Native body

- The stock tiled arm calls `SHPStruct::GetFrameBounds @ 0x0069E7E0`
  (`0x42364F`), takes `bounds.Height` (`0x423654`), stores it for the row step
  (`[esp+0x20]`, used at `0x423705..0x42370D`), halves it (`0x423660`,
  `cdq/sub/sar`) for the first row offset (`0x42366B`) and the per-tile Z fall
  (`[esp+0x14]`, `0x423709..0x423715`), then loops until the row leaves the
  view (`0x4236FC`).
- `AnimClass_DrawIt_Tiled_Interval` (`0x423654`, `Ext/Anim/Hooks.cpp:431-446`):
  a positive `Tiled.Interval` replaces `bounds.Height` before the store, the
  halving and every step.
- `AnimClass_DrawIt_Tiled_Center` (`0x423660`, `Hooks.cpp:448-462`): with
  `Tiled.AlignToCenter` the halving term becomes 0, so the first row is the
  draw point itself and both the row and Z steps use the height alone.
- Both hooks sit inside the tiled branch (`0x423636` tests AnimType+0x35B), so
  they do nothing for a non-Tiled animation, and the HasBounce ground shadow
  drawn before that branch keeps the current frame's own bounds.

## VERA

`AnimTypeRuntimeConfig::phobos_tiled_interval` / `phobos_tiled_align_to_center`
read the art section. `anim_draw_pieces` substitutes a positive interval for
the frame-0 pixel height it passes to `tiled_column`, and `tiled_column` takes
`half = 0` when the align key is set instead of `height / 2`; the existing
`rise`/`z` formulas then produce the native rows and Z terms.

## Tests

- `src/app/presentation/instances/anim_draw_tests.rs` —
  `the_tiled_column_rises_by_frame_zero_through_anim_pal_until_a_row_leaves_the_view`
  keeps the stock geometry.
- `tiled_interval_and_align_to_center_override_the_frame_geometry`: interval
  20 over a 10-pixel frame steps rows by 20 from 10 above the point, and
  align-to-center starts on the point and steps by the height alone.

Both key uses were mutated red and restored.

## Residuals

- An empty frame 0 (height 0) still resolves to VERA's single-tile draw; the
  stock infinite loop is documented next to `tiled_column`.
- `Tiled.Interval` is a signed `ReadInt`; only positive values replace the
  height, as the hook tests `> 0`.
