# Ares custom foundations

`Foundation` in `src/rules/foundation.rs` is the resolved geometry owner. The
existing BuildingType processing and `TypeFacts` cache carry that same value;
there is no parallel placement, navigation or renderer geometry registry.
Stock tokens retain the original fixed 22-entry table and row-major cell order.
With `ares`, the fixed ART effective Image section installs Custom geometry once:
`Foundation`, `Foundation.X/Y` (integer default 0), occupied `Foundation.i`, and
`FoundationOutline.Length` (default 0, minimum 10) / `FoundationOutline.i`.
The original cached INI readers and integer scanner are reused. Custom occupied
points sort signed X then Y and deduplicate; outline backing preserves authored
indices, missing-slot sentinels and its final sentinel. Direct factory slot 10
can therefore remain meaningful after the terminated dock list has ended.

The existing consumers now use the resolved owner: construction/cache and saved
state, cell occupation and hidden diagonals, navigation, picking, build admission,
whole-rectangle visibility, proximity's actual occupied providers, dock/factory
exit lists, selection/placement drawing, zshape dimensions, and related size
queries. Simulation configuration hashing includes ART geometry. Unsupported
sizes outside 0..65535, signed product overflow and outline lengths over 1048576
are rejected during rule validation; values are not clamped into another shape.
Custom dimensions and actual-list bounds remain distinct. Cursor4A94F0 returns
only the span:4A8C9E centers that span without subtracting a negative list minimum.
The existing rendering zshape calculation receives correct declared dimensions;
no depth offset is fitted to an image.

## Executed evidence

`ares_foundation.py` extends the existing EmpProbe/Probe VM and
`bridge_anim_inputs.Reader.make_ini` cache builder; it does not fork the PE, CRT,
INI parser, RNG or map algorithms. `ares_foundation.json` contains two synthetic
Map histories (rectangle2x4 and sparse holes/duplicates) plus one additional
reader/cursor history. The latter records full outline backing, index10 behind
missing slots, native bounds, offset and the published marker argument. The first
two histories were not rerun to append it. Metadata records Ares binary identity;
retail executable identity is retained in the local execution receipt under the
shared publication policy. Inspected relevant instructions match both supported
retail variants; the executed variant is pinned privately.

Executed bodies include extension constructor1000CAE0,
reader1000DE0F..1000E11A, getters10015310/100152D0/10010120/10010150,
actual-list bounds10011980, Map list10015490, hidden helper10011B00 and
Add/Remove10015420/100154B0, and cursor4A94F0/4A8C9E..4A8D11.
All three full RNG streams and code guards remain unchanged. This path has no
timer or detach writes. Reader reentry retains installed geometry and makes zero
geometry reads. Mark fixtures supply interior origin20,20 and depth2; real
counter writes, sorting and deduplication execute. Allocation, lexical cache,
CRT formatting/shutdown and hook/world shells are explicit service boundaries.
The cursor stops before Mark4A95A0. Whole BuildingType bootstrap, whole world
Mark/Unlimbo, pathfinding and GPU rendering are not native-executed by this probe.

Reproduce with externally supplied binaries (no assets are loaded):

```sh
python -m tools.rules_oracle.ares_foundation --ares /path/to/Ares.dll --gamemd-exe /path/to/gamemd.exe --check
```

Production-reader comparisons:
`rules::foundation::custom_tests::` (reader retention, independent dimensions/list,
outline backing, direct exit slot, cursor, ART hash and unsupported-size admission).
Production lifecycle regression:
`sim::world::custom_foundation_tests::` (construct/Unlimbo/Mark, actual occupied
cells, navigation, hidden counters, snapshot restore and UnInit cleanup).
Feature-off protection: `rules::foundation::tests::`.
These are Rust regressions, not a claim of whole-game rendered parity.

## Bounds and residuals

The installed final geometry is covered; the full original multi-pass ReadINI
bootstrap and its transient selector are not executed. Signed/packed off-map
cell policy remains an existing owner boundary outside the interior witnesses.
Independent RMG stock metadata and terrain ART metadata are not Ares gameplay
foundation authorities. Pending placement marker bit4 and extreme/zero-sized
render behavior remain existing separate boundaries. Existing native-z rules
(including maximum supported width) remain unchanged; a rendered capture is
needed to establish the selected building's final visual outcome. Ordinary
positive custom shapes are the selected loaded-building chain, not universal
support for pathological native allocations or every generator path.

## Persistence and feature-off impact

The String/u8 to typed Foundation migration changes ProcessedType cache and
TypeFacts snapshot serialization even with Ares disabled. No legacy-byte
serializer is installed. The existing automatic snapshot layout digest rejects
old layouts before body decode; regenerate processed caches and start a new game
with this candidate. Configuration hashes also intentionally include resolved
stock/custom ART geometry. This is not an assertion of feature-off byte-layout
compatibility. Feature-off still excludes Custom variants/readers.

The proximity correction also applies to stock shapes: providers must occupy an
actual ground cell and be the first Building returned by that cell's existing
list owner. Run `sim::production::production_placement_tests::` for the affected
stock ring, BaseNormal and directional BuildOffAlly admission checks, especially
`place_ready_building_requires_base_normal_provider_within_adjacent_range`,
`base_normal_false_structures_do_not_extend_build_area`,
`build_off_ally_lends_an_eligible_allied_provider_only_while_enabled` and
`build_off_ally_asks_the_providers_house_about_the_placer`.
