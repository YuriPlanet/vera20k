# MapGen bridge variant draw

`mapgen_range.py` executes original `598030`, `65C780` and `7C5F00` from
`gamemd.exe`.
The 91 cases retain all 250 RNG words, both cursors, disabled byte, padding,
incoming/cached FPU controls, raw draws and rejected candidates. No measured
gameplay callable or returned answer is substituted.

The original CRT precision tail and WinMain rounding/cache block produce
PC53/chop at explicit entry boundaries. This excludes the OS CPU probe and
intervening Windows startup. It does not prove every external callback preserves
that mode. Supplied alternate modes and retained raw-word fixtures are controls,
not claims that stock gameplay generates those states.

There are 137 range requests, 142 original Next calls and 138 state advances;
four calls use disabled streams. A further 645 explicitly declared Next calls
prepare cursor-wrap cases. Seed initialization is outside those counts.

`sim::rng::tests::high_two_bits_match_original_mapgen_range_and_all_retained_words`
compares the 72 `(0,3)` requests entering in PC53/chop against Rust, including
every logical state field after each request. Native padding has no Rust owner.
The production repair caller uses `SimRng::next_high_two_bits`; Seed0's first
three results are `1,1,2`. This corpus tests the range operation, not complete
bridge repair, native map loading or a whole-game RNG timeline.

The former general integer scaling formula is deliberately no longer exposed:
wide intervals can differ due to intermediate native rounding, reversed bounds
are not sorted natively, and the full unsigned span wraps to zero. Disturbed
PC53-nearest with raw `FFFFFFFF` can reject candidate 4 and take another draw
even for `(0,3)`. These controls remain saved without a wider Rust parity claim;
RMG has its own range implementation.

From the repository with the native-oracle Python dependencies and configured
`VERA20K_GAMEMD_EXE` (or `RA2_DIR`):

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python tools/spatial_oracle/mapgen_range.py --check
```

Explicit `--write` regenerates native references; default/check never writes.
The checked-in native JSON is unchanged from the independently generated and
checked corpus (SHA256 `945f06fe8a442f0bff97647641a5c2e48b86ae7f0d77018cc9a095d5159cc323`).
