# Paradrop candidate coordinates

`python -m tools.spatial_oracle.paradrop_coordinates --check` replays the pinned
retail executable through the shared [native runner](../native_oracle.md).
`--write` deliberately regenerates its JSON and provenance sidecar.

## Native boundary and ownership

Aircraft's overfly mission calls `DropPayload415C60` at `4159FB`. This corpus
starts at `415C7D`, after a passenger was removed, and stops at `415D78`, before
the candidate cell lookup. It executes the original payload decrement,
Aircraft virtual GetCoords, FacingClass Current, both trig lookups and both
ftol conversions. No instruction, callee or arithmetic result is substituted.

- `415CA6..415CF7` uses the post-decrement payload parity to wrap the **full
  heading word** by minus/plus `0x3FFF` (left/right).
- `415CF7..415D13` sign-extends that word, subtracts `0x3FFF` and forms the
  same angle used by Walk and Fly, using the original `7E2810` constant.
- `415D29..415D59` reads the retail sine/cosine table, multiplies by the
  original radius `128.0` at `7E2808`, adds/subtracts from the **world
  coordinate**, then truncates through `7C5F00`.

Rust's `drop_world_xy` owns only DropPayload's parity/heading selection.
`util::native_trig::facing_step_world_xy` owns shared numeric evaluation;
`movement::ground_pose` owns reading and splitting Location. The obsolete
byte-facing `v_offset` and its only-use conversion helper `sim_to_i32` are
removed. That helper claimed truncation toward zero while actually flooring;
changing it alone would still round at the wrong point in the drop expression.
`SimFixed` remains the simulation's default fractional type.

## Coverage

The corpus executes **393,216 coordinate prefixes**: every 16-bit heading,
both payload parities, and origins `(12928,5248)`, `(0,0)` and
`(131071,130816)`. Each sweep hashes signed little-endian i32 XY pairs in
ascending heading order and retains 26 explicit samples. Input Location Z is
1040 and must survive every row. Current() receives a retained heading and
zero turn rate; it is not a new FacingClass interpolation comparison.

`paradrop_world_coordinates_match_all_native_headings` compares production
coordinate math against every native digest and the explicit samples.
`paradrop_landing_cell_matches_original_coordinate_prefix` runs all 52 samples
at the first origin through `try_drop`, including infantry subcell placement,
Reveal and cargo departure. `paradrop_uses_native_cell_for_occupancy_admission`
checks both admission and retry when either of the two adjacent cells is full.

For example, heading `0x007F`, even post-count, at the centre of cell `(50,20)`
produces native XY `(13055,5249)`, still in `(50,20)`. The former byte-facing
path chose `(51,20)`. This affects placement/occupancy decisions, not just a
small drawing offset. A focused production test failed on this cell-selection
defect before the implementation changed (initial fixture translated to the
centre of `(0,0)`).

## Limits and retained effects

The native comparison ends before map admission, infantry subcell selection,
parachute construction, Reveal, success/retry and mission scheduling. Those
downstream Rust paths have regression coverage, not a new whole-paradrop
native comparison. The prefix executes no RNG draw, timer write or detach;
payload count is decremented once and the original code's Z is preserved.
Downstream subcell RNG, cargo-head removal/restoration, canopy attachment,
the successful rearm timer restart and team removal keep their existing owners.
A corrected cell can intentionally change whether those downstream effects run.

The three coordinate origins do not certify every i32 coordinate or the whole
simulation's platform determinism. Negative results from the zero origin exercise the
numeric boundary, not proof that the mission admits an off-map carrier. Stock
rules do not supply any value to this numeric prefix: its radius is native data.

The adjacent shared-helper audit also found a separate Rocket distance concern:
`int_distance_to_sim` could stop on the upper member of an integer Newton
two-cycle. The native Rocket locomotor port (`sim::movement::rocket_movement`)
replaced its only consumer and the helper is deleted
([issue #1027](https://github.com/YuriPlanet/vera20k/issues/1027)).
No ordinary-map overflow was demonstrated in the other inspected shared helpers;
the documentation now states their result/intermediate bounds explicitly.
