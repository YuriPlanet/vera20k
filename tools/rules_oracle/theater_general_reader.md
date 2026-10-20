# Theater General integer reader

[The harness](theater_general_reader.py) executes original gamemd instructions
for all 56 `[General]` integer reads inside `Read_Theater_TileSets_INI`
(`545150`), preserving the caller's defaults, read order and result stores.
[Native outputs](theater_general_reader.json) contain 34 synthetic input cases,
six physical theater files and seven ordinal-publication controls.
[Provenance](theater_general_reader.meta.json) records the Unicorn version, execution boundaries and fixture substitutions.

Explicit generation followed by an independent check passed on 2026-09-27.
This is bounded native evidence; Rust replay and production theater loading are
separate validation obligations.

## Reproduction

Use Python 3.10+, Unicorn 2.1.4 and Capstone, and configure the original
executable through `VERA20K_GAMEMD_EXE` or `RA2_DIR` as described in the
[native comparison guide](../native_oracle.md). From the repository root:

```sh
PYTHONPATH=. python tools/rules_oracle/theater_general_reader.py --check
```

Retail files default to `ini/`. Set `VERA20K_THEATER_GENERAL_ASSETS` when they
are elsewhere. The directory must contain `temperatmd.ini`, `snowmd.ini`,
`urbanmd.ini`, `lunarmd.ini`, `desertmd.ini` and `urbannmd.ini`. Output retains
each full physical file's byte count and SHA256, plus its exact extracted
`[General]` text. All six measured files have a unique General section and
unique keys within it. No RA2 base theater INI supplies these values.

The default invocation checks without writing; only `--write` replaces the
goldens. No original executable instructions are patched. Repository helpers
supply the original image and bounded runtime/INI-cache fixture.

## Native contract and order

The contiguous block starts at `545535`, where native code sets ESI to −1,
and stops at `545C3F`, after the last bridge-piece result store. Each call is
the original `CCINIClass::ReadInt` at `5276D0`. The JSON `read_contract` observes
the exact key, section, default, call address, result-store address and storage
location for every read. Its array order is the actual native call order:

```text
RampBase, RampSmooth, MMRampBase, ClearTile, RoughTile, SandTile,
GreenTile, PaveTile, MiscPaveTile, ClearToRoughLat, ClearToSandLat,
ClearToGreenLat, ClearToPaveLat, HeightBase, BlackTile, BridgeSet,
WoodBridgeSet, CliffSet, ShorePieces, WaterSet, SlopeSetPieces,
SlopeSetPieces2, MonorailSlopes, Tunnels, TrackTunnels, DirtTunnels,
DirtTrackTunnels, WaterfallEast, WaterfallWest, WaterfallNorth,
WaterfallSouth, CliffRamps, PavedRoads, PavedRoadEnds, Medians,
RoughGround, DirtRoadJunction, DirtRoadCurve, DirtRoadStraight,
DestroyableCliffs, WaterCaves, WaterCliffs, PavedRoadSlopes,
DirtRoadSlopes, Rocks, WaterBridge,
BridgeTopLeft1, BridgeTopLeft2, BridgeBottomRight1, BridgeBottomRight2,
BridgeTopRight1, BridgeTopRight2, BridgeBottomLeft1, BridgeBottomLeft2,
BridgeMiddle1, BridgeMiddle2
```

Every default is signed −1 except **DestroyableCliffs**, whose call at
`545978` explicitly passes −2. The preceding 46 keys are signed TileSet
ordinals stored in the caller frame; the ten bridge-piece keys are signed
relative tile identities stored directly in globals. They do not pass through
a `u16` conversion in this native block.

| Selected key | Native call | Default |
| --- | --- | --- |
| BridgeSet | `5456EF` | −1 |
| WoodBridgeSet | `54570A` | −1 |
| SlopeSetPieces | `545776` | −1 |
| SlopeSetPieces2 | `545791` | −1 |
| Tunnels | `5457C7` | −1 |
| TrackTunnels | `5457E2` | −1 |
| DirtTunnels | `5457FD` | −1 |
| DirtTrackTunnels | `545818` | −1 |
| DestroyableCliffs | `545978` | −2 |
| Ten bridge-piece reads | `545B4D` through `545C2E` | −1 |

The original publication block `545CEF..545FA3` compares each retained
ordinal with the supplied loop ordinal and publishes the cumulative tile base
only on equality. These controls execute that block after the real reads:

- `BridgeSet=65537` does not alias ordinal1; it remains unresolved at ordinal1.
- At supplied ordinal1, `CliffSet=1` publishes the full supplied base70000.
- At supplied ordinal65537, `BridgeSet=65537` publishes base123456.
- Negative roles retain their original reset values; missing/negative
  DestroyableCliffs retains its distinct −2 sentinel.

Four appended controls stay within the current production `u16` tile-index
projection. `all_ordinals_hex_19` supplies `$13` for all 46 ordinal keys and
executes ordinal19/base1234: every original role global publishes1234.
The `mixed_ordinal_0`, `mixed_ordinal_1` and `mixed_ordinal_19` controls supply
`BridgeSet=$13`, `WoodBridgeSet=1`, `CliffSet=bogus` and
`DestroyableCliffs=$nothex`. The actual reads produce19, 1, 0 and −2;
the respective ordinal/base pairs0/0, 1/7 and19/1234 publish only CliffSet=0,
WoodBridgeSet=7 and BridgeSet=1234. Other roles retain their original reset
values. These controls run independently after fresh General reads and resets.

The large ordinal/base controls establish DWORD comparison/storage behavior,
not reachability of such a full theater. TMP construction, cumulative count
generation, loop termination and later lunar global zeroing are excluded.

## Parser outcomes that consolidation must preserve

| Supplied text or condition | Original signed result |
| --- | --- |
| Missing section/key, empty or whitespace-only physical value | Caller default |
| Lowercase `general` or lowercase keys | Caller default |
| Exact `BridgeSet` alongside lowercase `bridgeset` | Exact key's value |
| `19`, `$13`, `13h`, `13H`, `+19` | 19 |
| `19tail`, `19.75` | 19 |
| `0x13`, ordinary nonnumeric `bogus` | 0 |
| Failed hex scan `$nothex` | Caller default |
| `-7`, `65537`, signed i32 extrema | Exact signed value |
| `4294967295`, `$FFFFFFFF` | −1 |
| Two identical keys, or two General sections, in the sampled layouts | First retained value |
| Empty first duplicate value, then `19` | 19 |

The previous ad-hoc theater scanner used case-insensitive names and Rust
`str::parse`, which produced different results for malformed text, native hex
forms and numeric suffixes. Removing it is therefore both consolidation and
a parser correction. Missing values must not become tile0: native defaults
are −1/−2, while a *present* malformed ordinary value can deliberately parse0.

Keep consumer projections explicit. Checked `u16` conversion remains suitable
for the existing presentation-facing optional fields; negative and oversized
values become absent there. Signed ordinal resolution must reject negative
values before indexing and avoid wrapping oversized values. The existing
`HighBridgeRimTiles::from_ini` already uses signed `read_int(key, -1)` for all
ten pieces and must retain that behavior. This corpus does not justify
replacing its signed identities with the narrower presentation fields.

The native `DestroyableCliffs=-2` default is distinct even though the current
optional resolved cliff-range projection maps either negative sentinel to
absence. Preserve −2 in the reader owner so another signed consumer does not
inherit a changed default. Likewise, later lunar zeroing remains a separate
post-read operation; `lunarmd.ini` itself still supplies ordinary integer keys.

## Physical theater results

| File | BridgeSet | WoodBridgeSet | SlopeSetPieces / 2 | Tunnels | DestroyableCliffs |
| --- | ---: | ---: | --- | ---: | ---: |
| temperatmd.ini | 19 | 80 | 25 / 26 | 53 | 56 |
| snowmd.ini | 19 | 73 | 25 / 26 | 47 | 61 |
| urbanmd.ini | 19 | 101 | 25 / 26 | 53 | 56 |
| lunarmd.ini | 19 | 80 | 25 / 26 | 53 | 56 |
| desertmd.ini | 19 | 80 | 25 / 26 | 53 | 56 |
| urbannmd.ini | 19 | 101 | 115 / 116 | 53 | 56 |

All six read `BridgeMiddle1=7`, `BridgeMiddle2=12` and `CliffSet=10`.
The JSON preserves all 56 results per file, not just this subset.

## Fixture and claim boundaries

The prepared cache retains lexical strings and their source order. It omits
empty values/sections and strips entry comments, matching the declared
physical-parser boundary; it never parses an integer or preselects a duplicate
winner. Native CRC indexes begin unsorted, so original native sort/search
chooses among retained duplicates. The five duplicate controls cover only
their stated two-entry/two-section layouts. They do not close the existing
arbitrary duplicate-name CRC/qsort residual or execute `INIClass525A60` file
loading. The ordinary six-theater inputs have no such duplicates.

The supplied loader frame begins after archive loading and ends before the
wall-clock/progress-service and TMP loop. No RNG draw, timer cadence, rendering,
map admission or full theater-load parity is claimed. Code-span hashes and
unchanged-byte assertions bound the executed reader/publication instructions.

The JSON binding surface is:

- `read_contract`: ordered `{key, section, default, call, store, storage,
  location}` records; frame values additionally identify `resolved_global`.
- `cases`: `{name, input_text, values}`; each `values` map contains all 56
  native signed results and input text can be passed directly to `IniFile`.
- `physical`: `{file, bytes, sha256, general_text, values}` for six retail files.
- `projection_cases`: `{name, input_text, values, ordinal_projection}`;
  projection supplies its ordinal/base and records all 46 original globals.
