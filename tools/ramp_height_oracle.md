# Checked ramp-height oracle

Run from the repository root with the [native setup](native_oracle.md):

```sh
python -m tools.ramp_height_oracle --check
python -O -m tools.ramp_height_oracle --check
python -m tools.ramp_height_oracle --write --output .local/ramp-height-candidate.json
```

Default/`--check` is read-only. Imports and help do not resolve retail files or run
native code. `--write` explicitly publishes a candidate and provenance sidecar;
review the candidate before changing the recorded reference. The shared
`tools.native_oracle` owns executable identity, PE mapping, completion, required
callee checks and publication. Fixture-local code owns supplied map/object state
and case enumeration. No original instruction bytes or call results are replaced.

The 158 cases execute original ground lookup `578080`, slope evaluation `47B3A0`,
Object height setter/getter `5F5FA0/5F5F40`, and Unit/Infantry placement leaves
`747EB0/5247D0`. Every setter must reach both original lookup and evaluation.
The case order, register clearing, memory initialization, live/cached x87 words,
104/416-lepton startup constants and dummy-cell behavior are preserved.
Early stops, instruction/time limits, faults and missing required callees fail
without publishing vectors. Placement-vtable checks remain active under `-O`.

The checked-in [golden](ramp_height_vectors.json) remains byte-identical. The
shared writer normalizes JSON key order in a newly exported candidate; all payload
fields compare structurally. Its [sidecar](ramp_height_vectors.meta.json) records
scope, binary identity and canonical payload hash separately.
[Validation](ramp_height.validation.json) records native replay and Rust coverage.

```sh
python -m tools.cargo_run -- test -p vera20k-core --lib live_surface_and_coordinate_setter_match_all_native_ramp_vectors
python -m tools.cargo_run -- test -p vera20k-core --lib live_raw_bridge_height_preserves_native_signed_deck_byte_and_dummy_identity
python -m tools.cargo_run -- test -p vera20k-core --lib moving_ramp_snapshot_continues_residual_bridge_crossing_through_paid_points
```

The first Rust test directly compares all 158 ground lookups. Its coordinate-height
comparison uses `native_raw.wrapping_sub(requested_height)` to test production
movement's zero-height request; it does not prove arbitrary requested-height setter
behavior. The bridge test compares six slope-normalized deck-byte encodings and
shared dummy identity, not full signed world-coordinate integration. The final
check covers movement save/restore continuation. Getter and
Unit/Infantry placement outputs retained in the native fixture are not all compared
by that Rust test. These supplied-state samples do not establish whole locomotor,
world-entry callback, occupation or rendering parity. RNG, timer and detach behavior
is outside this oracle's scope. Portable failure/lifecycle tests run in
`python -m tools.run_tests`; native replay requires the pinned retail executable.
