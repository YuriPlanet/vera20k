# TIBTRE native executable comparison

`tibtre.py` executes the pinned retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c` with the
shared checked Unicorn runner. The saved JSON is native output, not a Python or
Rust implementation of the mechanism. Its sidecar records the environment,
declared inputs, service sinks and payload hash.

From a checkout with retail `ini/RULESMD.INI` and `ini/ARTMD.INI`:

```sh
export VERA20K_GAMEMD_EXE='/path/to/retail/gamemd.exe'
python -m tools.spatial_oracle.tibtre --check
# Explicit publication only after reviewing native fixture changes:
python -m tools.spatial_oracle.tibtre --write
```

`VERA20K_TIBTRE_INI` can select another directory containing the two physical
INIs. The executable's neighboring `ra2.mix` supplies unchanged stock SHP bytes.
Archive/index decoding prepares fixture inputs through the existing
`tools.sidebar_oracle.stock` owner; it is not native archive loading. `--help`
and imports perform no native work, reads or writes. `--check` never repairs a
reference.

## Executed mechanism

Original Terrain AI `0x0071C730` calls the original Object AI `0x005F3E70`,
tests `IsAnimated` and the retained active rate, calls Scenario Random
`0x0065C780`, executes signed absolute-value/modulo and the x87 probability
comparison, then writes the animation stage/timer. Its timer query
`0x00426630` and stage advance execute unchanged. On the stock 22-frame SHP's
midpoint (stage 11), AI first resets stage/rate/timer and invokes original
Cell SpreadTiberium `0x00483780` with `force=1`.

Spread selects the source overlay's native Tiberium index, falling back to
Riparius index 0 only for index -1. Original `RandomRanged(0,7)` chooses the
start direction; the original ordered neighbor loop and
CanGerminate `0x004838E0` choose the first admitted empty cell. Existing ore and
gems are rejected by the overlay -1 gate. Original PlaceTiberium
`0x00487190` receives amount 3, selects the variant with
`RandomRanged(0,11)`, constructs/marks an original Overlay `0x005FC380` /
`0x005FC570`, recalculates the cell with `0x0047D2B0`, inserts the original
growth queue entry `0x007235A0`, then stores overlay data 3 and dispatches
dirty effects. The queue insert occurs before the data write.

Ordinary Terrain objects pass CanGerminate but independently block the Overlay
constructor's `Cell47C550` any-Terrain query. In that case Place still returns
true, retains the growth entry and writes data 3, while the overlay stays -1
and Land stays unchanged. With seed 1 at the explicit direct-call frame 200,
both successful placement and this constructor refusal retain priority bits
`0x43500000` (208.0), RNG cursors `[4,107]` and four following recorded draws.
This is state at return; later growth-queue processing is not certified.

Each RNG event includes its return value and both native cursor pairs, so
retries inside RandomRanged are visible. Whole native RNG state is hashed
after each AI visit, and every timeline/direct-spread row records four
following original draws.
Growth entries retain their original float priority bits.

## Reader and asset inputs

The physical RULESMD TIBTRE01 section establishes `SpawnsTiberium=yes`,
`IsAnimated=yes`, `AnimationRate=3`, and `AnimationProbability=.003`. ARTMD
supplies `Theater=yes` and `Foundation=1x1`; the four measured animation/spawn
keys belong to RULES. Original TerrainType constructor `0x0071DA80` and the
selected RULES reader/store blocks execute, including ReadDouble
`0x005283D0` and FSTP dword `0x0071E073`. Original constructor defaults are
rate/probability 0 and both flags false. The physical lexical strings are
prepared by the existing bridge_landing_inputs helper with original signed
CRC indexes; native physical INI loading and complete layer processing are
excluded. The payload pins both file hashes and selected physical keys.

The original reader/store produces `.003` float bits `0x3B449BA6`, rather than
a Python-chosen float cast. Executed controls accept sample 3000 and reject
3001. Probability .5 accepts sample 500000 and rejects 500001 in the declared
x87 environment. Zero probability rejects sample 0 but accepts the signed
INT_MIN raw draw, whose original absolute-value wrap/modulo leaves a negative
sample. These boundaries are original executable observations.

The payload pins TIBTRE01–03 `.TEM` and `.SNO` SHA/length/header/frame-format
inputs from physical retail MIX bytes. All six have SHP header `[0,84,56,22]`
and frame format 3 throughout. Pixel decoding, native shape drawing and GPU
output are excluded. Drawing arguments have a separate comparison in
[`terrain_render.md`](terrain_render.md).

Original instance constructor member/reset blocks
`0x0071BB9E..0x0071BBD1` and `0x0071BC86..0x0071BCA5` establish stage 0,
changed false, timer start at current frame, duration/rate 0 and step 1.
Fresh fixture instances execute these blocks. They exclude full construction,
placement, registration and its optional light producer. The timer's unused
middle dword copies a caller local; fixtures explicitly zero that padding.

## Coverage and limits

The corpus includes:

- Eight selected original type-read cases and four instance constructor
  controls, including signed frame extremes.
- 39 probability controls for stock .003, zero and .5, including negative raw,
  signed INT_MIN, modulo wrap and both probability boundaries. Each changes
  one supplied RNG state word so the original XOR draw produces `next_raw`;
  no RNG function or return value is replaced.
- Fourteen retained timelines (660 AI visits): stock seed 4 naturally starts
  at frame 32 and emits at frame 65; stock seed 1 remains idle during its
  42-frame window. Further rows cover rate 3/0/-1, zero probability, omitted
  keys, disabled flags, already-playing state, all blocked neighbors and two
  original spawners scheduled in both actor orders.
- Twenty-four direct forced-spread rows covering every start direction 0–7,
  overlay/land/slope/bridge/spawner admission, existing ore and gems, a gem
  source, blocked neighbors and disabled global natural spreading. Additional
  object/tile controls cover Units, ordinary Terrain constructor refusal,
  live/dead Buildings, Invisible and InvisibleInGame fields, and
  AllowTiberium=false. The two Building visibility flags are independently
  supplied retained fields, rather than full Building reader comparisons;
  native InvisibleInGame's post-read also forces Invisible=true and
  RadarVisible=false.

The shared refinery_dock map/Scenario fixture and harvest_field
overlay/Tiberium registry/queue fixture supply map width/height 16, known
Riparius/Gem ordinals, MaxStage 12 and growth/spread percentages .1. These
registry values are supplied inputs, not a claim that their full readers
execute here. The original Cell Recalc uses the existing terrain_recalc
synthetic resident 1x1 TMP and actual overlay Land 5, with supplied spare map
cache storage; physical TMP loading is outside the comparison.

Only successful bounded allocation/free storage and presentation dirty
services are replaced in the measured mechanism. Cell rectangle getters
return empty rectangles; Tactical/Radar calls record dispatch and return.
Original admission, variant selection, Overlay writes, Recalc, density,
queues, timers and RNG execute. Original Object AI receives silent objects
with no other active effects. Whole Logic scheduling, full Scenario growth
processing, save/load readers, game rendering and full TIBTRE parity are not
certified by these fixtures. Rust production tests must independently replay
the relevant rows and verify their connection through the live object turn.

## Rust integration and release observations

The validation receipt (`tools/spatial_oracle/tibtre.validation.json`, in git at 1b1a8b43e) records the exact candidate,
retained build manifests/binary hashes, native payload hashes and production
observations. `sim::world::tibtre_oracle_tests` replays all 660 original AI visits
and all 24 direct spread rows, comparing stage, rate, timer, cell bytes, growth
heap priority bits, complete RNG-state hashes and following draws. The terrain
owner separately compares all 39 probability and four constructor controls.
The production rules reader compares eight selected native type inputs.

Separate normal-frame composition tests follow actual emitted ore through the
existing growth scheduler, player `HarvestCell`, `MinerReturn` and refinery
credit deposit. Active animation survives the existing snapshot restore policy;
limbo clears its animation index. The renderer compares all 126 original draw
argument rows; the explicit retail atlas check decodes all 18 theater/type
bindings against the 44 original decoded-source hashes and binds all 11
body/shadow pairs. The shared read-only GPU blitter comparison and production
Ground lowering/overlap checks passed on Apple M4/Metal. These checks establish
their named boundaries, rather than full native game or image parity.

Final library validation passed **9,412 tests, 0 failures, 226 ignored**, with
mandatory retail INIs and archives. Clippy completed with 728 warnings and no
errors; the Python suite passed 436 tests with four optional cases skipped.
Leaf formatting, diff checks and the simulation-field ratchet passed.

The normal release loader/render path observed retail AnyTown for 800 steps
using [`map_observation.tibtre.example.json`](../map_observation.tibtre.example.json).
Both stock TIBTRE02 objects visited every body stage 0–10; their six complete
cycles took exactly 33 frames from hit to midpoint reset. All eight neighbors
already held ore, so forced spreading correctly placed nothing.

A separate authored input preserves that physical map's bytes except removal
of its complete `OverlayPack` and `OverlayDataPack` sections. It starts with no
ore; the live producer created density-3 cells at frames 331, 455 and 680, each
33 frames after its animation started. This exposed a required presentation
prerequisite: the atlas preloaded resource art, but the runtime draw-name index
registered only map-authored ore identities. A regression failed before the
shared registration owner was extended to include every parsed resource type.
The corrected release visibly draws the three new cells. All recorded terrain
observations and initial/final fingerprints match the preceding release; only
presentation pixels changed (146 pixels in the cleared-field endpoint).
The raw retail/derived maps and BGRA files remain local, not in the repository.

To reproduce the authored input, extract `XMP03T4.MAP` from `multimd.mix` using
the existing `tools.sidebar_oracle.stock::{mix,mix_hash}` input owner. Require
source SHA-256 `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`.
Preserve each physical line and its line ending, omitting only the two complete
sections named above. Set a copy of the example profile's `selected_map_file`
to that new absolute path, retaining all other fields, then run the ordinary
`tools.map_observation` command described in its guide. The receipt records the
derived bytes, input hashes, cycle/placement observations and both release
identities. This preparation changes authored input; it does not inject
simulation state or create a parity golden.

One fresh read-only critic independently inspected the original instructions and
production consumers, finding no blocking stock-chain defect. The two minor
documentation findings were corrected. The branch then integrated main
`4ab3a4f5f67750527fd424e7ae26118200499222` without conflicts and passed the final
library suite and clippy again. Release label `tibtre-native-integrated-20261001`
pins code commit `dc5d208e7c3cba3d1a35013cf7e28ad453c6c248` and binary SHA-256
`0a263e642f8a9d7f0aaf4c3d24951a0dc2a26290445551b029eb4ad4d88955d3`.
Both refreshed 800-step observations validate and exactly match the preceding
release's recorded observations, fingerprints and frame bytes. The receipt keeps
both validation stages and all three release identities; subsequent changes only
update documentation and evidence receipts.

The OQ-38 enqueue-side growth/spread array-counter rebuild was a residual at
this TIBTRE validation. The subsequent [ore-queue comparison](ore_queue.md)
implements and checks those thresholds, receiver reseeding and connected callers
against native execution. Its bounded coverage is separate from the TIBTRE
producer/render comparisons above; neither certifies a whole long-session history.
