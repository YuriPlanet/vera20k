# Phase 3 `LastTilesInSet` compatibility translation design

**Status: proposed for fresh read-only design review.**

## Goal

Implement the live YR theater compatibility table and exact IsoMap tile-index
translation proven in
[PHASE3_LAST_TILES_IN_SET_COMPATIBILITY_TRANSLATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_LAST_TILES_IN_SET_COMPATIBILITY_TRANSLATION_GHIDRA_REPORT.md).
Installed stock theaters produce an empty table, so the change must preserve
all stock outputs while closing the active executable path for retail-format
legacy/custom theater data.

## Native contract

`Read_Theater_TileSets_INI @ 0x00545150` builds declaration-ordered
`{legacy_boundary:i32, delta:i32}` records. For each nonterminating tileset
with `T=TilesInSet`, `L=LastTilesInSet`, and a wrapping signed legacy cursor:

```text
if L != -1 and L != T:
    boundary = legacy_cursor wrapping+ L
    delta = T wrapping- L
    append(boundary, delta)
    legacy_cursor = boundary
else:
    legacy_cursor = legacy_cursor wrapping+ T
```

Both keys use native `ReadInt`; exact `TilesInSet=-1` terminates, while other
negative values do not. Missing section/key also produces `-1`, malformed
present text produces zero, and native hexadecimal forms remain accepted.

`CalculateLegacyMapTileIndex @ 0x00544E30` returns positive `65535` unchanged.
Otherwise it walks records in declaration order, stops at the first signed
`boundary > original_raw`, and wrapping-adds every preceding delta. Every
comparison uses the original raw value, never the accumulated result.

The helper is called only by the five IsoMap readers after destination-cell
acceptance and before tile storage. Fill, runtime Mark/direct tile mutation,
and save restoration do not translate. Save data already contains actual
translated IDs and must never be translated again.

## State ownership

Add a private copyable `LegacyTileIndexException` and a private
`legacy_tile_index_exceptions: Vec<_>` to `TilesetLookup`. This is immutable
theater definition state, not simulation state: it is reconstructed from the
active theater INI, is not serialized in snapshots, and does not enter the
world hash. `load_theater` rebuilds a value-equivalent table on every map load
from the active immutable theater bytes. The native vector's allocation
address, capacity-growth identity, and ownership bytes are not gameplay state,
and same-theater external file mutation while the process is running is outside
the installed-retail domain. No process-global Rust table is introduced.

Expose one pure `pub(crate)` method on `TilesetLookup`:

```rust
translate_legacy_map_tile_index(raw: i32) -> i32
```

It implements the exact sentinel, signed original-input comparisons,
declaration-order early stop, and wrapping accumulation. It performs no range
validation, sorting, clamping, or registry lookup.

## Parser changes

Replace the fixed `0..10000`/section-presence loop in `parse_tileset_ini` with
an ordinal loop that treats an absent section as native `TilesInSet=-1` and
stops only on exact `-1`. Use `IniSection::read_int` for both numeric keys.

Maintain a separate signed wrapping `legacy_cursor`. Build the compatibility
record before converting the current positive tile count into safe Rust
registry slots. `TilesInSet=0` and values below zero other than `-1` create a
zero-count bounds/metadata row but still update legacy arithmetic. A positive
count creates exactly that many registry slots even when files are blank or
missing.

Correct `SetName`'s already-verified native missing-key default to `"No Name"`;
`FileName` remains empty. This is the only adjacent parser correction admitted
by the slice. Other tileset fields retain their current owners.

Rust's tile identity is `u16`, so a positive registry total that cannot be
represented must return a dedicated `MapError` rather than truncate, allocate
unboundedly, or emulate native memory corruption/OOM. Likewise, ordinal
overflow is a safe invalid-input rejection. These are explicit invalid-domain
exclusions and cannot affect the six installed retail theaters.

The safe representable limits are explicit. Positive tile ID `65535` is the
IsoMap no-tile sentinel, so the registry may contain at most 65,535 usable
slots, IDs `0..=65534`; the first total of 65,536 errors before allocation or
`TilesetBounds` narrowing. Tileset ordinals `0..=65535` fit the existing
`u16`-owned metadata and lookup APIs; attempting to continue at ordinal 65,536
returns the ordinal-overflow error. Removing the arbitrary 10,000-section cap
therefore makes `TileSet10000` load normally without weakening either real
representation boundary. A missing `SetName` is stored as native default
`"No Name"`.

## Map-ingress integration

`MapFile.cells` has two existing ingress meanings: decoded IsoMapPack cells are
raw compatibility-space IDs, while RMG and manually constructed maps already
contain actual registry IDs. Preserve that distinction using the existing
load-only provenance: a nonempty `iso_map_pack_lookups` vector means the
represented explicit cells came through an IsoMap decoder. A sentinel-only
pack yields no explicit cells, so its empty vector has no tile to translate.

In the production `materialize_map_load_cells` path, pass the active
`TilesetLookup` separately from the variant selector. Only when that IsoMap
provenance is present, clone each coordinate-accepted explicit record,
translate only `tile_index`, and then replace the prefilled cell. RMG/manual
actual-ID cells remain unchanged. Invalid/null Pack records remain dummy-only
and never translate. Fill cells retain the cached actual Clear/Water IDs.

The selector-free `ResolvedTerrainGrid` constructor is a synthetic/test path.
When it is supplied `TheaterData`, it translates explicit cells only if the
same IsoMap provenance vector is nonempty; RMG/manual actual-ID fixtures and
theater-less fixtures remain raw. This path remains distinct from snapshot
restore. `Simulation.resolved_terrain` is skipped by serde: the persistence
owner clones the retained, already-translated terrain template into
`rebuild_caches_after_load`, then
`restore_map_authority_after_snapshot_load` reapplies serialized
`dynamic_terrain_cells`, whose tile IDs are also actual IDs. Neither stage may
call the legacy transform.

Do not add translation to LAT, variant selection, `IsometricTileClass::Mark`,
cliff/bridge mutations, snapshot load, or any lookup method. Those consumers
must receive the already-translated actual ID.

## Player-experience and subsystem ledger

- Tile image, variant chain, TMP metadata, LAT/slope inputs, land type, bridge
  classification, radar, and world rendering all consume the translated cell
  ID through the existing materialized terrain pipeline.
- Coordinate, subtile, level, ice-growth, stream order, and dummy-cell behavior
  remain unchanged.
- Empty compatibility vectors are identity and must preserve all installed
  stock map output, RNG, source lineage, and state hashes.
- Pack1-4 are compiled live callers but absent from all 386 installed map
  payloads. The API is format-neutral, while this builder changes only the
  represented Pack5 ingress.

## Acceptance tests

1. Parser builds `{5,+3}` and `{11,-2}` from the verified three-set example.
2. Translation with a nonempty table asserts `4->4`, `5->8`, `10->13`,
   `11->12`, positive `65535->65535`, and separately proves that signed `-1`
   is translated rather than mistaken for that sentinel.
3. An accumulated result crossing a later boundary proves comparisons still
   use original raw; a negative boundary proves signed comparison; a
   nonmonotonic table proves first-greater early stop and no sorting. Separate
   `i32::MAX/MIN` cases prove wrapping transform accumulation.
4. Empty-vector identity covers representative positive, negative,
   `i32::MIN/MAX`, `-1`, and positive `65535` inputs.
5. Missing/malformed/hex values, exact `TilesInSet=-1` termination with later
   sections, direct `LastTilesInSet=-1` suppression, direct
   `LastTilesInSet==TilesInSet` suppression, zero, nonterminating negative
   counts, and parser-side wrapping cursor/delta arithmetic are pinned.
6. Positive missing/blank files still consume actual registry slots. Exactly
   65,535 slots succeeds and the 65,536th errors; ordinal 65,535 is accepted
   and ordinal 65,536 errors through a focused checked-ordinal helper test.
   `TileSet10000` succeeds after cap removal, and a missing `SetName` yields
   `"No Name"`.
7. Production-shaped Pack5 materialization proves a coordinate-accepted
   explicit record reaches translated `source_tile_index`, variant/TMP lookup,
   LAT input, and final tile state.
8. Fill at an applicable numeric boundary remains unchanged; invalid Pack5
   coordinates only update the dummy and do not materialize a translated cell.
9. The same boundary-valued explicit ID translates in a Pack5-provenance map
   through both production and selector-free constructors, but remains
   unchanged in RMG/manual actual-ID maps and theater-less fixtures.
10. Runtime actual-ID mutation plus the full persistence
    prepare/serde/rebuild/restore route retains an ID that would change if the
    transform ran again, proving both retained-template and dynamic-cell
    authority avoid double translation.
11. All six active retail theater INIs build empty vectors and retain existing
    registry starts/counts and map expectations.

Focused validation uses only `cargo test -p vera20k --lib <filter>` after
confirming Cargo/rustc are idle. The phase-wide full suite remains deferred
until every Phase 3 row closes.

## Exclusions

- Do not emulate native allocator failure, null-record dereference,
  negative-index memory access, or unbounded allocation.
- Do not add Pack1-4 decoders in this slice.
- Do not implement Marble Madness, theater conversion, TMP extension fallback,
  RMG generation, or editor behavior. Preserving existing RMG actual-ID cells
  at shared materialization ingress is required, not an RMG expansion.
- Do not update older stale research documents in the implementation commit;
  their corrections are already recorded in the new verified report.

## Decision

Proceed only after a fresh read-only design critic returns PASS with zero
findings. Any ambiguous parser, ingress, no-double-translation, or stock-
preservation behavior keeps this mechanism open.
