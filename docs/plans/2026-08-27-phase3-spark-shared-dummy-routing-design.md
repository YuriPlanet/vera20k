# Phase 3 Spark shared-dummy routing design

**Status:** PARITY PASS — implemented in `4c71b488`; fresh critic 7 found zero
findings, open questions, approximations, or residuals

**Completion ledger:** validation repairs `72bf8e15`, `96779c16`, and
`0054549e`; critic 4 authoritative-prose repair `439059f1`; critic 5 source-
provenance repair `c5cd916f`; critic 6 ledger repair `548fc0cf`. Critic 5 also
recorded the lower-priority stale status prose; critic 6 rechecked the prior
fixes and found only that remaining ledger issue. Fresh critic 7 rechecked all
prior findings and independently audited the complete mechanism, returning PASS
with zero findings. Current-HEAD focused validation: `gsi_04_03` 69/0,
`sim::particles::spark` 45/0, exact v106 rejection 1/0.

**Integration rebase (2026-08-30):** The source branch accumulated unrelated
snapshot changes before this mechanism and therefore used `111 -> 112`.
Current main enters this slice at version 106, so the identical Spark hash and
resume boundary is integrated as `106 -> 107`; no excluded source-branch
schema fields are imported.

**Phase/GSI ownership hypothesis:** Phase 3 / GSI-04.03, behavior-3 Spark
height/collision lookup routing. The general overlay writer lifecycle remains
owned by GSI-04.07; this design closes only the exact projection Spark observes.

**Native authority:**
[docs/research/PHASE3_SPARK_SHARED_DUMMY_ROUTING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_SPARK_SHARED_DUMMY_ROUTING_GHIDRA_REPORT.md)

## Verdict

Replace Spark's eager, strict real-cell fact collection with the native staged
lookup transcript over the existing process-global `SharedCellDummy` identity.
A cell miss must select and stamp the dummy, then continue through ordinary
collision, coordinate, deletion, color-RNG, and lifetime processing.

The implementation must not turn the staged native transcript into another
eager fact bundle. Candidate ground is read first, candidate structural state is
read second, the old cell is selected only when candidate structural state is
clear, building/wall reads occur only in the contact band after bridge crossing
has failed, and the candidate slope is selected only after a collision has been
chosen.

This slice will not add a general dummy overlay field. The verified supported
writers that can reach the dummy use bridge-family overlay IDs, none of which is
one of Spark's three wall IDs (`2`, `26`, `243`); ordinary valid wall placement
cannot write its wall ID to the dummy. For Spark, a dummy's sentinel wall query
therefore returns false exactly. General persistence of dummy overlay identity
is recorded for GSI-04.07 rather than approximated here.

## Player-experience and lockstep ledger

| Situation | Native result | Current Rust divergence | Required result |
|---|---|---|---|
| Constructor floor lookup enters a null fixed-array slot | Dummy-derived ground; creation continues | Miss becomes `None`, so Z may not clamp | Use dummy level/slope and repeat the lookup only when `input_z <= first_ground` |
| Flying Spark crosses the allocated Size rim | Normal dummy-based collision/no-collision | Query errors after persistent-Z write | Commit coordinates/delete state, consume one color draw, then decrement lifetime |
| Candidate structural dummy bit is clear | Old cell is selected and can become the final dummy stamp | Rust selects old first and always selects both | Candidate first; old only on the native short-circuit arm |
| Candidate structural dummy bit is set without crossing | Old lookup is skipped; candidate remains last stamp | Rust selects old and can alter the stamp | Skip old exactly |
| Any collision is selected | Candidate slope lookup occurs last and restamps candidate on a miss | Rust reads slope eagerly with the first candidate selection | Delay slope selection until collision is known |
| No collision is selected | No slope lookup; last stamp remains candidate or old | Rust still resolves slope | Represent slope as absent and perform no lookup |
| Noncanonical axes alias an allocated slot | Real-cell hit; dummy is untouched | Current strict helper already aliases some cases but is separate | Reuse the common fixed-stride fallback helper without component bounds |

Severity is edge-bound but deterministic: when a Spark reaches an allocated map
rim, the current error path changes coordinates, deletion, color RNG, lifetime,
and therefore subsequent lockstep state.

## Verified native contract

### Constructor

For behavior 3, construction takes the lifetime RNG draw, copies the input
coordinate, calls `CellClass::GetGroundHeight(input)`, and calls the same helper
a second time only if `input_z <= first_ground`. It then commits coordinates and
only later takes the optional authored start-color draw. A miss is a normal
dummy selection on both ground calls.

### Per-tick transcript

After the persistent-Z store and candidate arithmetic:

1. candidate ground lookup;
2. candidate Cell lookup and raw/live structural read;
3. old Cell lookup only if candidate structural is clear;
4. bridge-crossing classification;
5. building lookup, then wall lookup only when there is no bridge collision and
   raw candidate Z is in `[ground, ground + 150)`;
6. complete collision classification, including below-ground paths;
7. candidate slope lookup only when a collision was selected;
8. normal commit/delete, one color draw, and lifetime decrement.

The candidate Cell pointer is retained across the optional old lookup. If both
select the dummy, they alias the same object after the old-coordinate restamp.
The coordinate restamp does not clear level, slope, flags, overlay, or list
state.

### Spark-consumed fallback state

| State | Rust source in this design |
|---|---|
| packed requested coordinate | existing `get_cellclass_fallback_leptons` stamp |
| signed level and slope | real `ResolvedTerrainCell` or live dummy snapshot |
| structural bit `0x100` | real static-plus-runtime projection; dummy raw flag directly |
| ground object list | real occupancy list; dummy is the verified permanently empty list |
| sentinel wall identity | real `OverlayGrid`; dummy false under the verified supported writer exclusion |

## Current architecture and constraints

- `src/sim/cell_rect.rs` already owns the non-null fixed-stride lookup and one
  process-global dummy handle. Spark must reuse it rather than creating another
  fallback API.
- `src/map/resolved_terrain.rs` already stores dummy coordinate, level, slope,
  and the modeled raw bridge-bit subset, reconstructs it at map Resize, and
  includes its bridge bits in the state hash. Dummy coordinate, level, and
  slope are currently hashed only while a projectile retains `DummyCell`.
  Spark makes level/slope future-state authority without such a projectile, so
  this slice must hash dummy level/slope unconditionally with the bridge subset.
- `src/sim/particles/spark_world.rs` currently performs old/candidate strict
  lookups, eagerly resolves all facts, and errors on fixed-array misses.
- `src/sim/particles/spark.rs` owns the exact x87 predicates and collision
  arithmetic. World lookup code must not duplicate or approximate those
  comparisons.
- `src/sim/particles/spark_spawn.rs` currently performs only one optional
  ground lookup and treats any adapter failure as no floor.
- Real structural state remains the existing static Cell flag plus live
  `BridgeRuntimeState::deck_present` projection. Dummy structural state is its
  raw bit and must not require a runtime bridge cell.

## Chosen design

### 1. Spark-local selected-cell view

Add a small private selected-cell view in `spark_world.rs` around
`get_cellclass_fallback_leptons`. It preserves `CellRef::Real` versus the live
`SharedCellDummy` handle and exposes only the facts Spark reads:

- signed level byte and slope byte;
- structural state, with real and dummy routing separated;
- canonical real-cell coordinate for occupancy/overlay adapters;
- a dummy discriminator for the proven empty-building/non-wall projections.

Selection itself is the side effect. The view must not reconstruct, clear, or
copy the shared dummy into an independent identity. A retained dummy view must
continue to refer to the same shared handle after an old-cell restamp.

Factor selection behind a private closure- or trait-driven seam used by the
production adapter and focused tests. The test implementation records the role
and coordinate of every selection (`ground`, `cell`, or `slope`). This is
required because two consecutive candidate stamps have the same final state;
checking only the last dummy coordinate cannot prove that both native calls
occurred.

### 2. One ground evaluator over real or dummy selection

Route both constructor and behavior-3 candidate ground through the common
selected-cell view and `ground_height_leptons`. A miss supplies the live dummy's
level/slope; unsupported slope values remain the existing safe-state error, not
a zero-height substitution.

The constructor floor operation should be factored behind a tiny closure- or
adapter-driven helper so focused tests can prove one versus two lookup calls.
Production behavior is:

```text
first = ground(input)
if input_z <= first:
    input_z = ground(input) // second native lookup/restamp
```

Constructor ground selection must be terrain-only. It may not construct a
`SparkCollisionWorld` whose initializer requires `OverlayGrid`, because native
does not inspect overlay state while flooring a particle. An entirely missing
resolved terrain remains a clearly documented unsupported fixture policy and
may retain the existing no-floor behavior. Missing overlay state is irrelevant
to constructor flooring. A present map with a null/unallocated/invalid cell may
not use either unsupported-world policy.

### 3. Share collision gates with the arithmetic owner

Extract package-private pure helpers in `spark.rs` for the exact bridge-crossing
classification and raw-f32 contact-band gate already implemented there.
`spark_world.rs` uses those helpers solely to decide which native queries occur;
the final collision owner reuses the same helpers when producing the committed
coordinate and kind. Do not copy the inequalities into `spark_world.rs`.

The final complete kind classification must also be available as a pure helper
so the world adapter can decide whether the collision-only slope lookup is
required without reimplementing below-ground precedence.

### 4. Make absent slope explicit

Change `SparkCollisionFacts.slope_matrix` to an optional collision-only fact.
No-collision queries return `None` and therefore prove that no slope lookup was
performed. Collision queries perform the final candidate selection and return
`Some(matrix)`.

The collision owner must reject `None` only when its own exact classification
selects a collision. It must not read or require a matrix for a no-collision
result. This invariant prevents a future eager default matrix from hiding a
missing native lookup.

### 5. Native-ordered world query

`SparkCollisionWorld::query` performs:

```text
ground = select(candidate); evaluate ground
candidate = select(candidate); candidate_structural = structural(candidate)
if candidate_structural:
    old_structural = false // no lookup
else:
    old = select(old); old_structural = structural(old)

bridge_kind = exact shared helper(...)
if bridge_kind is none and exact contact-band helper(...):
    building = accepted_building(candidate) // dummy => false
    if not building:
        wall = sentinel_wall(candidate)      // dummy => false

kind = exact shared complete classifier(...)
matrix = if kind is some:
    slope = select(candidate).slope          // final native restamp
    Some(slope_matrix(slope))
else:
    None
```

For a real candidate, building retains the verified ground-object insertion
order and first-building exclusions. Wall reads the real canonical overlay
cell. Those reads remain errors only for corrupt/missing real-world dependencies
already outside the supported map contract. A fixed-array miss is never an
error.

`SparkCollisionWorld::new` must not eagerly require `OverlayGrid`. Construction
requires only terrain and the stable simulation/rules borrows. The real-cell
wall arm obtains `OverlayGrid` at the instant native calls the sentinel wall
predicate; only reaching that arm may report a missing real overlay dependency.
Dummy wall false never consults the rectangular overlay grid.

### 6. Preserve commit and RNG ownership

Keep `begin_particle_tick` before the world query and
`finish_particle_tick` after it. Once a present-map miss becomes a normal fact
selection, the existing owner supplies the native commit/delete, one color RNG
draw, and lifetime decrement. Do not add RNG to the world adapter.

Update stale comments that describe query failure after the persistent-Z write
as ordinary missing-cell behavior. That partial-write error boundary remains
only for genuine unsupported/corrupt dependencies or arithmetic errors.

## Rejected alternatives

### Keep eager facts and merely substitute dummy values

Rejected because native lookup order and short-circuiting determine the final
shared-dummy coordinate. It would still query old, building/wall, and slope on
branches where native does not.

### Add component-wise or playfield bounds

Rejected because native bounds only the signed fixed-stride linear index.
Noncanonical axes may alias a real slot and must leave the dummy untouched.

### Extend the shared dummy with full overlay identity in this slice

Rejected as unnecessary cross-row expansion. The active supported dummy writers
proven in the Spark investigation cannot produce Spark wall IDs, while general
overlay identity persistence belongs to GSI-04.07. Spark's dummy wall result is
therefore an evidence-backed false projection, not an approximation.

### Treat a null/unallocated slot as an unsupported world

Rejected because valid retail-format Size diamonds necessarily have adjacent
null canonical slots and stock Spark motion can reach them.

## Files expected to change

- `src/sim/particles/spark_world.rs`
- `src/sim/particles/spark.rs`
- `src/sim/particles/spark_spawn.rs`
- focused tests colocated in those modules and, only if needed for the
  production dispatch assertion, `src/sim/particles/system_ai.rs`
- `src/sim/world/world_hash.rs`
- `src/sim/snapshot.rs`
- stale Spark report/comment wording only where the implementation makes it
  factually wrong

Advance `SNAPSHOT_VERSION` from 106 to 107 and reject v106 at the preamble.
Although the wire shape is unchanged, a serialized active Spark resumes with
different coordinate, deletion, color-RNG, and lifetime behavior after this
fix. That is the same behavior-boundary rationale used for v105 to v106.

Hash dummy level and slope unconditionally beside its already-unconditional
bridge subset. Spark now consumes those persistent fields and can alter future
state even when no projectile retains a dummy target. Add a regression proving
that two otherwise equal Spark-reachable worlds with different dummy
level/slope values produce different hashes. Dummy coordinate remains
conditional on a retained pointer/consumer; this slice does not make the
coordinate itself a future Spark input after the current query completes.

## Acceptance tests

1. Constructor lookup seam records one ground call when above the dummy floor
   and two identical calls when equal/below; lifetime RNG remains before and
   optional start-color RNG remains after coordinate flooring.
2. The instrumented production-selection seam proves exact transcripts:
   structural-clear/no collision is `ground(candidate), cell(candidate),
   cell(old)`; structural-set/no crossing is `ground(candidate),
   cell(candidate)`; each collision case appends `slope(candidate)` to its
   applicable prefix.
3. Transcript tests prove bridge-collision and out-of-contact-band paths do not
   query building or wall, and an accepted building suppresses the wall query.
4. Mixed candidate-real/old-dummy and candidate-dummy/old-real cases retain the
   candidate view while preserving the exact selection transcript and final
   dummy stamp.
5. Candidate dummy structural clear plus old dummy, with no collision, finishes
   with the old coordinate stamped and an absent slope matrix.
6. Candidate dummy structural set plus no bridge crossing skips old lookup and
   finishes with candidate stamped.
7. A dummy-fact collision performs the final candidate slope lookup, commits
   and marks deletion, consumes exactly one color draw, and decrements lifetime.
8. A dummy no-collision commits candidate, consumes exactly one color draw,
   decrements lifetime, and leaves slope absent.
9. Collision classification with an absent matrix is rejected, while
   no-collision classification with an absent matrix succeeds without reading
   one.
10. Dummy building is false and dummy sentinel wall is false under constructor
    defaults and supported bridge-writer contamination. Terrain-present
    constructor/no-contact cases do not require `OverlayGrid`.
11. Fixed-stride alias to an allocated real slot is a real hit and leaves an
    earlier dummy coordinate unchanged.
12. Null allocated-mask slots plus negative and high linear indices continue
    through the normal Spark path without `OutOfRangeCell` or `UnavailableCell`.
13. Dummy level/slope changes alter the world hash even with no retained dummy
    projectile; snapshot schema is 107 and v106 is rejected before body decode.
14. Existing real-cell bridge collapse, building ordering, LaserFence exclusion,
    wall, below-ground, slope-matrix, coordinate, deletion, RNG, and lifetime
    tests continue to pass.
15. The row gate `gsi_04_03_spark_shared_dummy_query_order_and_miss_continuation`
    exercises production dispatch and the instrumented production-selection
    seam rather than a Rust-vs-Rust duplicate oracle.

Focused validation should include the narrow Spark world/spawn/system filters
and the existing Spark arithmetic tests. Every command must use
`cargo test -p vera20k --lib <filter>`. The Phase-wide full `--lib` suite remains
deferred until every Phase 3 row is closed.

## Critic packet and pass condition

Each fresh read-only critic receives:

- this design;
- the native report and cited active-retail data;
- the implementation diff;
- literal focused test output.

PASS requires zero findings, open questions, approximations, or residuals inside
this bounded mechanism. Any error-on-miss path, eager lookup, duplicated native
predicate, missing final restamp, constructor single-call clamp, untested RNG /
lifetime continuation, missing exact transcript assertion, conditional-only
dummy level/slope hash, stale snapshot schema, eager overlay precondition, or
unsupported-slope substitution keeps the mechanism open. General dummy overlay
persistence remains explicitly assigned to GSI-04.07 and is not a Spark
behavioral residual because its three-ID predicate is exactly excluded for every
supported dummy writer established by the native report.
