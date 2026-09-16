# Spark Live Collision Adapter and Owner Design

## Goal

Query authoritative Spark collision facts at the native point inside each particle tick, then run forward particle AI and same-tick reverse cleanup without enabling public Spark spawning or rendering.

## Architecture Context

`Simulation` owns a stable-ID-ordered `ParticleSystemStore`; each `ParticleSystem`
owns its particles in a `Vec` whose forward order is native-relevant. The system
dispatcher removes one system, ticks it with mutable `Simulation` access, then
reinserts it at the same stable ID. Smoke, Gas, and Fire already use this owner
shape. Spark remains disabled in production dispatch.

`src/sim/particles/spark.rs` now contains the deterministic behavior-3 arithmetic
kernel. It commits the persistent Z velocity, creates both stored-`f32` and integer
movement candidates, resolves injected collision facts, commits deletion and
coordinates, consumes the scenario-owned particle RNG, advances color, and
decrements signed lifetime. Its present `SparkTickInputs` contains collision facts
before the tick begins, so it cannot reproduce the native query point in production.

The live facts already have plausible Rust owners:

- `ResolvedTerrainGrid` owns static cell level and slope bytes.
- `BridgeRuntimeState` owns mutable deck state, while resolved terrain retains the
  original structural flags. Their relationship to the live native `0x100` bit
  through collapse and repair is not yet proved.
- `OccupancyGrid` preserves CellClass-style list order. Non-buildings prepend,
  buildings append, and rebuild uses `occupancy_enter_order` before stable ID.
- `EntityStore` and `RuleSet` resolve the first building candidate and its type.
- `OverlayGrid` owns live non-bridge overlay IDs, including wall removal/damage.
- `Simulation::particle_rng()` is the named scenario-RNG route for particle draws.

The approved native-float design remains authoritative for arithmetic and state.
This focused design adds the world-query seam and owner traversal only; it does not
replace that design or authorize the still-blocked renderer path.

Primary evidence:

- [docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md)
- `docs/contracts/2026-07-18-spark-collision-pixel-compositor-implementation-contract.md`
- `docs/plans/2026-07-18-spark-native-float-and-point-compositor-design.md`
- Live roots `0x0062C6E0`, `0x0062E840`, `0x00578080`, `0x0047B3A0`,
  `0x007559B0`, and the bridge-flag write sites found by the prerequisite pass.

## Impact Analysis

Expected implementation surfaces:

- `src/sim/particles/spark.rs`: expose bounded begin/query/finish stages without
  duplicating arithmetic or changing their verified order.
- `src/sim/particles/spark_world.rs`: new read-only adapter over terrain, bridge,
  occupancy, entities, rules, and live overlays.
- `src/sim/particles/mod.rs`: declare the adapter module if it remains separate.
- `src/sim/particles/system_ai.rs`: forward Spark traversal, authoritative RNG
  routing, and explicit reverse-index cleanup.
- `src/rules/particle_type.rs` and `src/rules/ruleset.rs`: retain verified native
  `ColorSpeed` and `Gravity` raw bits if the prerequisite investigation confirms
  the existing INI reader boundary.
- Focused unit and integration tests alongside those modules.

Risk areas:

- Querying before the movement candidate exists changes native timing.
- Holding immutable world borrows while requesting mutable RNG can create borrow
  pressure; the adapter must return owned facts before RNG is borrowed.
- Static bridge facts can become stale after collapse or repair.
- Replacing first-match list traversal with ID order or unordered presence changes
  which building suppresses or triggers contact.
- `retain` preserves survivors but does not express native reverse destruction
  order; cleanup must walk indices in reverse.
- Missing map data, unsupported conditional state, or malformed ColorList data
  must not silently select an approximate path.
- The target files are already in a dirty shared worktree. Implementation must
  inspect current diffs and preserve unrelated session changes.

## Chosen Approach

Use a concrete read-only `SparkCollisionWorld` adapter called between the existing
motion and collision stages. First close the bounded binary prerequisites; then
wire one owner function that performs the complete per-particle sequence.

The adapter returns an owned `SparkCollisionFacts`, so its immutable world borrows
end before `Simulation::particle_rng()` is borrowed mutably. The pure injected-fact
test wrapper remains available, but production ordering is owned by a single
world-aware entry point rather than by external callers assembling facts early.

The owner ticks particles in vector order. After every forward tick has completed,
it scans indices from last to first and removes marked particles. Public Spark
system creation and behavior-3 rendering remain unavailable until their separate
activation gates close.

Before implementation, a bounded Ghidra pass must prove:

1. Exact `0x0047B3A0` ground-height interpolation, signed coordinate reduction,
   and dummy-cell behavior.
2. Exact slope-matrix raw values or their exact startup construction for every
   slope byte reachable by stock YR terrain.
3. Live `CellClass+0x140 & 0x100` mutation through bridge collapse and repair and
   its Rust authority mapping.
4. Native read/store widths and raw stock values for `Gravity` and `ColorSpeed`.
5. Any conditional LaserFence runtime state needed by the stock-active query. If
   stock remains data-inert, the adapter must still reject unsupported non-stock
   state explicitly rather than accepting it as an ordinary building.

## Tiny-Detail Ledger

- Standard YR dispatches behavior 3 to the Spark root; this is live YR, not a TS
  ghost. [doc: collision report section `Active-YR reachability`]
- Persistent `old_vz - Gravity` is stored before any world fact is queried.
  [GHIDRA `0x0062C705..0x0062C71C`]
- Old-coordinate stores, probe store, old `ftol`, vector addition, and candidate
  `ftol` preserve the verified component order. [doc: collision report section
  `Process x87 control mode and exact store boundaries`]
- The integer candidate selects the cell and bridge predicates, while retained
  candidate `f32` Z selects ground/contact/clamp predicates. [GHIDRA
  `0x0062C6E0`]
- Candidate X/Y are converted to the native cell frame with the exact verified
  signed semantics. Rust now has the shared mutable dummy substrate in
  `cell_rect`, but Spark still returns typed unavailable/off-array errors rather
  than routing through it; that caller-specific integration remains open. [doc:
  [PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md)]
- Candidate ground includes signed terrain level and exact slope contribution.
  [GHIDRA `0x00578080`, `0x0047B3A0`; exact formula pending prerequisite]
- The slope matrix is selected by the candidate cell's slope byte, not particle
  facing. [GHIDRA `0x006D6AD0`, `0x007559B0`]
- Structural collision checks the live old-cell or candidate-cell `0x100` bit;
  plane and equality rules use `G+416`, with ascending commit `G+396`.
  [doc: collision report section `Collision decision table`]
- Building lookup scans the candidate ground object list and stops at the first
  `WhatAmI==6`; an excluded first building does not authorize unordered searching
  for another. [GHIDRA `0x0047C520`]
- LaserFence/connectivity and exact 1x1 `UndeploysInto` exceptions run before wall
  fallback. [GHIDRA `0x00457620`, `0x00465D40`; collision report]
- Wall fallback consumes the live candidate-cell overlay and accepts only IDs
  `2`, `0x1A`, or `0xF3`. [GHIDRA `0x00480510`]
- Fact collection consumes no RNG and mutates no authoritative state.
  [GHIDRA `0x0062C6E0` call order]
- Collision writes deletion before final coordinate conversion/commit; coordinate
  setter writes X, Y, Z. [GHIDRA `0x0062CA34..0x0062CA6C`, `0x005F6940`]
- Every active Spark particle then consumes exactly one ranged scenario-RNG draw,
  including a particle already marked by collision. [GHIDRA
  `0x0062CA72..0x0062CA86`, `0x0065C7E0`]
- Color update precedes wrapping signed-`i16` lifetime decrement; only a result of
  zero deletes through lifetime. [GHIDRA `0x0062CE40`]
- Owner traversal is forward AI followed by reverse same-tick dead cleanup.
  [GHIDRA `0x0062E840`]
- Public spawn and rendering stay disabled; no generic motion, SHP, bridge, or
  renderer fallback may masquerade as Spark parity. [implementation contract]

## Design

### Components

#### Pure Spark kernel

Keep arithmetic, collision decision, color, and lifetime in `spark.rs`. Split the
current tick only at the native world-query boundary:

- Begin: validate runtime state, store persistent Z, and build `SparkMotionStep`.
- Finish: consume owned facts, resolve/commit collision, draw RNG, update color,
  decrement lifetime, and return the existing tick result.

The existing injected-fact wrapper composes these stages for focused tests. There
is no second collision implementation.

#### SparkCollisionWorld

The adapter is a small immutable view or bounded query function over:

- resolved terrain;
- runtime bridge state;
- overlay grid;
- ordered occupancy;
- entity store;
- interner/type-handle data and `RuleSet`.

It accepts `SparkMotionStep`, derives old and candidate cells in the verified
native frame, and returns only `SparkCollisionFacts`. It owns no storage and has no
RNG access.

#### Spark owner

The owner runs:

1. Begin one particle tick.
2. Query and own its collision facts.
3. Finish that particle using `particle_rng()`.
4. Continue forward.
5. Remove marked particles by descending index.

The Spark branch may be internally testable while public construction remains
rejected. It must not opportunistically add burst spawning, lights, or rendering.

### Interfaces / Contracts

- World-query input is the exact `SparkMotionStep`, not only a rounded cell.
- Facts are owned values; no world borrow crosses the RNG call.
- Missing ordinary cell data still returns a typed unavailable/error result in
  Spark even though the shared dummy substrate exists; closing that routing is
  separate mechanism work.
- A genuinely unsupported prerequisite returns a typed unavailable/error result.
  It never substitutes flat ground, identity slope, static bridge state, or an
  ordinary building.
- Valid stock gameplay must complete without panics or per-particle allocation.
- System and particle order remain deterministic and hash-visible through their
  existing owners.

### Data Flow

1. `system_ai` removes a Spark system from the stable-ID store.
2. It visits `particles[0..len]` in ascending order.
3. `spark.rs` commits persistent Z and creates stored-float/integer candidates.
4. `spark_world.rs` reads the candidate terrain, slope, live bridge bits, ordered
   first building, conditional exclusions, and live overlay.
5. `spark.rs` commits collision deletion/coordinates.
6. The owner supplies the authoritative particle RNG for color progression.
7. `spark.rs` updates color and lifetime.
8. The owner removes marked particles from highest index to lowest.
9. Existing system lifetime and store reinsertion continue in their established
   outer owner.

### Error Handling

- Kernel numeric/malformed-state errors remain typed.
- Missing map prerequisites and explicitly unsupported conditional states return a
  typed adapter error while public activation is disabled.
- The production activation gate must require all stock-reachable adapter results
  to be available; it may not swallow an error and continue with approximate facts.
- Queries have no partial world commit. The already-native persistent-Z commit
  remains visible if a later compatibility error occurs, matching the staged
  kernel contract.

### Testing Strategy

- Preserve the current kernel ordering and x87 tests.
- Prove the fact callback occurs after persistent-Z/motion creation and before
  collision/RNG.
- Flat, sloped, invalid/dummy, bridge equality, below-ground, contact-band, and
  wall-ID tables use Ghidra-derived fixtures.
- Two buildings with reversed stable IDs but controlled enter order select the
  same first building before and after occupancy rebuild.
- Destroyed/repaired bridge fixtures assert the live structural query against the
  new native evidence.
- Two-particle owner tests pin forward RNG consumption and reverse cleanup without
  survivor reordering.
- Headless missing-map and unsupported LaserFence cases assert explicit
  unavailability, not a fallback.
- Run focused tests serially, then one `cargo check -q`, after confirming no other
  Cargo owner is active. Format only edited Rust files with edition 2024.

## Architectural Decisions

- Follow the existing remove/tick/reinsert system owner and particle-vector order.
- Keep the kernel world-independent and put cross-subsystem reads in one sim-layer
  adapter.
- Use owned fact snapshots to solve Rust borrowing without moving query timing.
- Prefer a concrete bounded adapter over a general trait because Spark has one
  production world and the kernel already has injected-fact tests.
- Preserve native mechanism through explicit stages rather than exposing a caller
  API that can precompute facts early.
- Defer public activation only for named missing prerequisites; do not replace
  them with guessed constants or current approximate render/map paths.

## Alternatives Considered

### Generic SparkCollisionFactSource trait

A trait would make fake providers easy, but the pure kernel already accepts
injected facts. It adds generic/dynamic abstraction across a single production
world without improving ordering or parity.

### Implement a supported subset immediately

Flat ground, healthy static bridges, ordinary buildings, and walls could be wired
with explicit errors for slopes, damaged bridges, and LaserFence state. This avoids
silent approximation but leaves active-gamemd holes and cannot be the production
adapter. It is rejected in favor of closing the bounded prerequisites first.

### Precompute collision facts before the particle tick

This fits the current `SparkTickInputs` shape but queries before the native
persistent-Z/movement candidate exists and cannot derive candidate-cell ground or
live facts at the correct point. It is known DRIFT and rejected.

### Reuse render slope math or static bridge flags

The render slope path uses ordinary hardware floats, and the static bridge flags
are not proved live after collapse/repair. Neither is an authorized simulation
source; using them would introduce known or unchecked drift.
