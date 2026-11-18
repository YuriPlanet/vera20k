# Packed translucent object composition

`blitter_a.py --translucent` executes original selector, pixel-leaf and producer
instructions over supplied leaf inputs; it is not a whole-scene comparison.
`--write` stores the JSON one record per line.

```sh
python -m tools.procedural_drawing_oracle.blitter_a --translucent --check
```

- `selectors`: SHP `Convert490E50` and voxel `Convert490B90` for bits 2/4/6/10/12,
  with and without `+4000`. Unit `73B140` composites its source cache once through
  `490B90`; SHP `705E00` reaches `490E50` through `4AED70`.
- `cases`: N1/N27/N53 palette and LUT profiles, A, indices, brightness and
  destination boundaries, with final colors and prior and final depths. Bit8
  warps 1/2/3 (`displacement`) read `[destination+warp*2]` at `495641`, a word
  the call has not stored yet. Bit_Blit `4373B0` seeds the warp from the row,
  relative to the tactical top, and the clipped left x
  (`4374A3..4374B7`, XOR 3 per row at `4378F9`); the Unit `73B140` value
  `70BE50` computes is never read. SHP character4 does not add bit8
  (`705E45..705E56`); Unit and voxel do.
- `replay_controls`: three successive calls of one leaf on a retained destination
  and depth array.
- `cloak_transition`: `Foot4DA4E0 -> Techno703860` and Unit selector
  `73B21F..73B259` over stage and progress controls, including invalid stages.

`src/render/draw_state.rs` and `src/sim/game_entity.rs` test the cloak rows. The
ignored GPU tests in `src/app/presentation/render/packed_material_gpu_tests.rs`
read `cases` and `replay_controls`, placing each warp case where the row and
column parities give its warp. The committed JSON is trimmed to the fields a
Rust source names ([capture files](../README.md#capture-files)). Allied and sensor visibility branches are not
executed.

Rust's visual-character scaling truncates in integers. For positive i32 progress
`p` and nonzero i32 stages `s`, `256*p` is below `2^39`. Under the fixture's native
`0E7F` control word (53-bit precision, truncate) the scaled division error stays
below `2^(-13)/abs(s)`, while a noninteger `256*p/s` is at least `1/abs(s)` from an
integer, and an integer result is exact because `p/s` is then representable. So
truncation cannot cross an integer boundary; the signed64 result wraps to low32.
Stage zero follows the executed masked-invalid result, whose low32 is zero.

Shadow callers `Unit73C5C4 -> Foot4DB0D0 -> Techno706BD0` require raw cloak state
`+220 == 0` (`706BDD`) and NoShadow false (`706BF3`). `StartCloaking703799` with
`+224 == 0` still returns visual character zero, so the visual character cannot
replace the raw-state gate. This is instruction reading, not an executed shadow
lifecycle.
