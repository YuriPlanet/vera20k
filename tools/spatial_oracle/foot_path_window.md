# Foot path window

```sh
python -m tools.spatial_oracle.foot_path_window --check
python -m tools.spatial_oracle.track_path_continuation --check
```

`VERA20K_GAMEMD_EXE` or `RA2_DIR` selects the local original executable.
`--write` deliberately regenerates the new corpus and metadata. The second
command checks that the reused continuation owner's existing corpus is unchanged.

The new fixture calls [`track_path_continuation.query`](track_path_continuation.py)
with an optional pre-Process observer. Its ordinary Unit destination setter,
Drive or Ship Process, `Foot::Find_Path` wrapper, native class callback, Mark,
timers, reference-cell store, and caller continuation all execute. The new hook
does not change the existing owner's default calls or expected outputs.

The existing AStar seam at `0x4CBBA0` supplies the complete PathType and direction
buffer. The supplied length is the movement-word count plus a trailing `-1`,
following the original reconstruction layout at `0x42AA90` (node count store
`0x42AAD7`, terminator store `0x42ABB2`). **AStar search and reconstruction do
not execute in this fixture.** The input route geometry is deliberately supplied;
there is no claim that these routes are optimal or produced by a particular map
search. This boundary does not truncate the supplied search output.

The actual wrapper at `0x4D3E82..0x4D3E9F` selects the copy count and performs
`REP MOVSD` into Foot's queue at `+0x5E0`. The observer records the native ECX
count, every native copy write, and all 24 destination words. It does not
calculate expected words from a capacity formula. Controls cover a short route,
23, 24 and 25 movement words, and a 25-word route containing a tube word `8`
inside the copied window. The tube input includes a nonadjacent cell jump for
the Rust queue adapter; native TubeArray lookup and traversal are excluded.

The initial queue has a live `-1` head and distinct stale backing words. A short
native copy leaves backing words after its new terminator unchanged. The corpus
keeps those words as evidence, while `live_words` projects only the prefix before
the first `-1`, or all 24 slots when no terminator was copied. Rust's Vec queue
compares the live prefix and reference cell, not unused native backing storage.

At `0x4D3E98`, immediately before the copy, the observer temporarily places
distinct canaries in the neighboring dwords at Foot `+0x5DC` and `+0x640`.
At `0x4D3EA1` it records their unchanged values and restores their previous
contents before native Mark and timers resume. In particular, `+0x640` is a
real movement timer and is not claimed unchanged after the whole wrapper.
Neither the source buffer, native copy count, nor any copied direction is
modified by this instrumentation.

Three output boundaries remain separate:

- `copy.after`: immediately after native `REP MOVSD`, before Mark/timers.
- `wrapper_return`: after the real wrapper's successful `RET 12`, including
  its `0x4D4003` reference-cell update and timer effects.
- `after_process_boundary`: the existing owner's head-selection stop, before
  head selection and subsequent movement. These rows have not consumed a path
  word at that stop.

All cases use the ordinary zero-prefix caller. Train append, physical map
loading, full pathfinding, movement scheduling, and complete trips remain outside
this native comparison. Original text and selected vtables are checked unchanged,
and both copied-neighbor guards are checked during every replay.

## Rust comparison and continuation

[`installed_path_window_matches_original_find_path_copy`](../../src/sim/movement/foot_path_window_tests.rs)
passes each complete supplied route through the shared `FootPathQueue::install`
owner and compares its live direction words and reference cell with the native
`wrapper_return` observations. This is the direct native comparison; unused
backing storage, the search itself, and later movement are excluded.

In the same file,
`retail_tank_and_infantry_replace_path_windows_and_reach_their_move_destination`
uses retail `MTNK` and `E1` rules on an authored clear 64×64 arena. Each unit
receives one ordinary Move order from `(4,32)` to `(58,32)`, separately with
serial and batched path searches. Production whole-frame ticks must retain the
distant destination, install a bounded first window, replace it as the trip
continues, and reach the requested cell without another order. This is a Rust
continuation regression, not a native full-trip, route, or frame-timing
comparison. The copy limit is 24 direction words; it does not prescribe a new
search after every 24 cells.

## Suggested manual gameplay check

This interactive check has not been run for this contribution.

1. Load a clear 64×64 test map with a stock Allied `MTNK` near `(4,32)` and no
   obstacles or enemies along the route to `(58,32)`.
2. Select the tank and issue one normal Move order to `(58,32)`, 54 cells away.
   Give no intermediate orders. Confirm it continues across the map and reaches
   the destination rather than stopping partway.
3. Repeat separately with a stock `E1`. Keep the route clear of the first unit.

These observations check the visible long-order outcome. The automated tests
above inspect queue replacement and exercise both search scheduling modes.
