# Bounce cliff direction and complete Update controls

```sh
python -m tools.spatial_oracle.bounce_height --cliff --check
```

The physical extension also needs the existing
`VERA20K_PROJECTILE_RENDER_ASSETS` extracted reader assets described by the
typed-warhead-debris owner, and the configured retail install containing
loose `Hills.mmx`. It uses those existing owners rather than a new extractor.

The additive `--cliff` mode reuses `bounce_height.py`'s sparse Cell VM and
original map queries. The default command and historical `bounce_height`
payload remain unchanged. Each new VM executes the existing
`native_slope.initialize_slope_tables` original startup chronology before
sampling any mirror or slope matrix; it does not copy matrices from a
different VM or build them with host arithmetic.

The payload contains 23 direct direction controls and 23 complete
`BounceClass::Update @ 439B00` transactions. `Cell4848B0`, selector755C60,
matrix-vector transform5AFB80 and complete Update run without replacements.
Call/return observers record inputs and actual outputs; no hook supplies a
direction, matrix, velocity, branch result or sampled Cell level.

`physical_tire` and `source_body_hex` come from the existing consumed
`typed_warhead_debris.json` original physical TIRE reader/full constructor
row. In particular, the reader's stored double Elasticity is
`3FE99999A0000000`, and constructor gravity is `3FF6666660000000`; neither
is replaced by host f64 literals 0.8/1.4. The complete original Bounce body
provides spin and orientation. Update controls override only position and
velocity with explicitly supplied finite numerical boundary inputs. They
do not claim an ordinary stock producer naturally reached those states.

`direction_cases[].input` supplies a requested receiver coordinate, an
argument world coordinate and sparse Cell tuples
`[x,y,signed_level,slope,flags,overlay]`, plus Dummy state. The original
5657A0 selects the receiver before the actual4848B0 call. Four cardinal
directions and all four diagonals with neither/A-only/B-only/both side
admission are covered. Same-cell and two retained-Dummy histories cover
the initial argument reselection and ordered side lookup effects.

The direction table is indexed by i32 differences of sign-extended i16
coordinates; only later side coordinates truncate to i16. A-only returns
`(direction-1)&7`, while B-only returns `direction+1` without masking.
The NW/B-only case returns8 in both direct and whole-Update controls.
755C60 subtracts4 once, so8 selects row4. `initialized_matrix_bits` retains
all five actual rows from B450B8, including the fifth row's adjacency to
the initialized slope storage. Replacing that row with direction0 or
masking directions by3 would change native outputs.

`update_cases[].initial_body_hex`/`final_body_hex` are the complete80-byte
Bounce body. The20 directional cases enter the original rollback arm;
the three controls cover one-level rise, equal-level contact and airborne
no-contact. Final body bits include restored position, direction-dependent
velocity, original double Elasticity scaling, reversed spin and integrated
orientation. `outcome` is the original return value.

Each row also retains:

- `direction_calls`: actual retained receiver coordinate/Dummy identity,
  argument CoordStruct and signed result.
- `lookup_trace`: ordered world/packed lookups with immediate selected live
  coordinates and shared-Dummy identity, captured at matched native returns.
- `matrix_calls` and `transform_calls`: actual selector result, matrix words,
  input and output vector words. Whole Update also records its preceding
  ground-reflection matrix operations; this is not only the final cliff call.
- `final_dummy_coord`, selected reached PCs and before/after SHA256 for all
  three full1012-byte RNG states. Their supplied input bytes are retained
  once at top level, and unchanged states are asserted on every transaction.

These are bounded numeric/kernel and caller controls. Cell/table geometry,
numeric boundary states and RNG starting states are supplied; source
placement, real-map cliff topology, normal flight history, debris model
loading, rendering, damage, complete object lifetimes, Windows startup and
save/load are outside this corpus. No bulk flight history or repository
source hash is retained. The sidecar pins the original executable and the
reproduction command. Rust consumes the corpus through the existing Bounce
and canonical world terrain owners.

## Physical Hills constructor and retained host flight

`physical_hills` adds one original constructor-entry Seed5 control, separate
from the supplied single-tick states above. Existing `typed_setup` runs the
physical TIRE scalar reader and full7493B0 constructor. The constructor's
actual source coordinate is `(26240,16256,208)`; all launch velocity, spin,
orientation and seven RNG requests come from original instructions. The
authored Smashing list in this fixture allocates the TIRE type only; it does
not claim an ordinary stock Smashing producer or a pre-death world seed.

Existing MIX/map-pack owners unwrap the physical loose Hills wrapper and
decode its inner map. The original theater reader determines tile files;
the existing TMP constructor/lazy loader supplies original5471B0 slopes and
544BE0 land results for a bounded20-cell footprint `(100..104,63..66)`.
These retain actual tile/subtile/level/no-overlay tuples, including lower
`(102,63)` tile505/sub0/level2 and upper`(103,63)` tile49/sub0/level6. The
fixture supplies their sparse native Cell enrollment. Every observed world
or packed lookup must return a registered physical coordinate; a Dummy or
unrepresented coordinate fails the generation.

The SAME typed constructor VM/object executes original full749F30 host
visits, with original matrix startup. No Bounce body or velocity is copied
into that host, and no duration is synthesized. Compact checkpoints retain
full80-byte before/after bodies, actual duration, Location, alive/Limbo,
Logic/Display/registry membership, pending-delete count and actual nested
Bounce/direction/matrix/lookup calls:

| Visit | Actual host result |
|---|---|
|25|Falling; duration126→125|
|26|Cliff Bounced; duration125→124; direction2|
|27|Falling; duration124→123|
|144|Stopped; duration7→0; still registered in Logic/Display|
|145|Expiry/UnInit; dead/Limbo, removed from Logic/Display, pending1; registry identity retained|

Original725C70 subsequently drains that retained identity and runs scalar
cleanup: pending count becomes0 and Voxel registry membership disappears.
Existing OS readability transport is explicit, and allocator/delete retain
readable fixture storage; this is not a host memory-deallocation claim.
`final_state` means host state after145; `before_drain`/`after_drain` retain
the separate deferred boundary. `delete_visit` names the logical host
retirement, not the later scalar cleanup.

`constructor_rng_before`/`constructor_rng_after` contain all three full
streams; `constructor_requests` retains the original seven requests/raw
words. The host flight and drain assert all three states unchanged from
the returned constructor state. Seed5 describes constructor entry only.
Ordinary HARV death admission, whole-world scheduling/RNG prefixes and
visible rendering remain separate production gates.
