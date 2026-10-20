# Native Anytown concrete repair cursor

Eight bounded original `Map587410` queries cover both authored huts, (85,58)
and (89,51), in healthy, first-damaged, collapsed and repaired states. The
unmodified stock map and resident fixtures come from the existing
`tools/spatial_oracle/anytown_damage` package. No bridge-cache representation,
cursor predicate or native lookup answer is supplied by Rust or by this harness.

The complete original mapped `.text` remains SHA256
`4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc`
after every measured call. No native instruction is patched. The canonical
payload SHA256 is
`2e605f8e8155de13d1407498daf4dede82d473b8374093685307194248bec7fb`.

## Executed input and result

The shared Resident owner reads physical `XMP03T4.MAP`, SHA256
`7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`,
and exact theater/overlay/land values, supplies its 180 physical cells and
pristine TMP/zone storage, and performs the existing original initial Recalc.
First damage and collapse are one or two actual `57CCF0` calls at (87,54).
Repaired cases additionally execute actual `573540` from the selected hut.
All preparatory calls and their effects are retained. Damaged overlays are
never manufactured. Original seed-0 Main/Scenario/MapGen streams and frame
1000 are supplied at initialization; preparatory repair advances only MapGen.

| Hut | Stage | Last selected coordinate / overlay | Query AL | Map lookup calls |
| --- | --- | --- | ---: | ---: |
| (85,58) | Healthy | (87,57) / 229 | 0 | 60 |
| (85,58) | First damage | (87,57) / 229 | 0 | 60 |
| (85,58) | Collapsed | (87,57) / 229 | 1 | 56 |
| (85,58) | Repaired | (87,57) / 229 | 0 | 60 |
| (89,51) | Healthy | (88,53) / 216 | 0 | 60 |
| (89,51) | First damage | (88,53) / 218 | 0 | 60 |
| (89,51) | Collapsed | (88,53) / 221 | 1 | 52 |
| (89,51) | Repaired | (88,53) / 215 | 0 | 60 |

Every query scans all 25 candidate cells in Y-major/X-minor order and retains
the **last** matching family. Its native tile gates precede the overlay gates;
the selected stock cases take the concrete overlay path with tile-family flag
0 and concrete flag 1. They scan first +Y, then -Y if required. For collapsed
hut (85,58), +Y reaches off-band (87,58); the negative pass finds terminal 232
at (87,54). For collapsed hut (89,51), the positive pass reaches 232 immediately
at (88,54). First-damage overlays 218/219/220 do not make the cursor true.

For all eight queries, the entire resident Cell-memory hash, 180 interpreted
Cell snapshots, three RNG streams and frame remain unchanged. False returns
are EAX `0xFFFFFF00` and AL 0; true returns are EAX/AL 1. Only AL is the Boolean
contract: `587C44` clears AL without clearing EAX's upper bits.

## Native source and limits

The frozen external `native_587410.asm` preserves the original full function;
[`hut_cursor_native_bytes.json`](hut_cursor_native_bytes.json) pins the original body
and selected instruction bytes. No external Python or disassembly is a replay dependency. Selection runs
`587433..5876B4`, with last selection observed at `5876BA`. Tile-family gates
use WoodBridgeSet (`ABAD1C`) first and BridgeSet (`AA0E28`) second, each spanning
16 tiles; overlay gates then test 74..101 followed by 205..232. The concrete
X-oriented group at `587812..587833` walks -X then +X. The other concrete
orientation at `58793D` walks +Y then -Y. Either concrete path accepts terminal
overlay 231 or 232 as the true result. Only the physical Y-axis path is executed
by this packet; mixed-family precedence and other-axis behavior are instruction
evidence, not additional native fixtures.

The selected `587410` path executes fully with original lookup and no replaced
dependencies. Reaching its structural-record branch at `5876C6` fails closed;
no synthetic structural records are supplied. The broader structural bridge
record path remains a separate residual. The fixture does not run the Mouse/UI
caller, a click, an Engineer command/lifetime, hut destruction or native scenario
loading, and makes no Rust or production comparison claim.

Preparatory damage/repair keeps the existing Resident boundaries: empty bridge
occupants, supplied pristine TMP heads and initial zone planes, successful
bounded allocation, and recording connectivity/hierarchy/presentation callbacks.
Actual selected Recalc and ordinary damage/repair bodies still run. Those
prerequisite seams are not cursor-result substitutions.

[`hut_cursor.json.gz`](hut_cursor.json.gz) includes every preparatory and query trace, all scan candidates
and evolving selections, directed lookup requests/results, cell writes and full
before/after states, return registers, frame and complete RNG states.
[`hut_cursor.meta.json`](hut_cursor.meta.json) pins every imported repository Python source and records
the exact boundaries. [`hut_cursor_promotion.json`](hut_cursor_promotion.json) pins every original artifact.
[`hut_cursor_receipt.json`](hut_cursor_receipt.json) records the promoted replay;
[`hut_cursor_summary.json.gz`](hut_cursor_summary.json.gz) is the original result projection.
The earlier six-case `anytown-hut-native` packet remains byte-for-byte unchanged.

## Replay

From the repository root, use the native-oracle Python/Unicorn environment,
the matching original executable and the two local asset roots described in
`tools/spatial_oracle/anytown_damage/retail_manifest.json`. LANGRULE is absent.

```sh
export VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe
export VERA20K_SHRAPNEL_INPUTS=/path/to/bootstrap-assets
export VERA20K_ANYTOWN_INPUTS=/path/to/anytown-assets
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.anytown_damage.hut_cursor --check
```

`--write` republishes the hash-pinned original values through the existing shared
compressed-reference owner; changed native output is rejected. A separate
`--check` re-executes all eight cases and compares the full payload and provenance
without writing. All native fields are preserved: this publication projection
is identity before canonical JSON/gzip encoding. No proprietary MAP/TMP/INI body is copied into this packet.
