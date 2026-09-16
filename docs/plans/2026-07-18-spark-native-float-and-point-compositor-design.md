---
title: Spark Native-Float Compatibility and Tactical Integer Point Compositor Design
date: 2026-07-18
status: design approved; implementation remains gated by the explicit native/runtime prerequisites
scope: Behavior-3 per-particle x87-compatible state and arithmetic, collision tick, and tactical u16 A/Z single-pixel consumer/committer contract. Spark burst production, persistent lights, exact A/Z production, and unresolved runtime values remain separate prerequisites.
source: [docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md) plus docs/contracts/2026-07-18-spark-collision-pixel-compositor-implementation-contract.md and the live Ghidra x87 startup/conversion chain recorded in this design.
---

# Spark Native-Float Compatibility and Tactical Integer Point Compositor Design

**Status:** Approved on 2026-07-18

## Goal

Provide a deterministic Rust-native representation of the verified Spark
particle x87 arithmetic and a shared tactical integer point-composition
contract that can reproduce the active Yuri's Revenge movement, collision,
A/Z predicates, and packed single-pixel result without introducing simulation
floating point or silently adapting the existing approximate renderer.

## Architecture Context

The current particle architecture has two ownership levels:

- Simulation owns ParticleSystem values in a BTreeMap-backed store.
- Each ParticleSystem owns a Vec of Particle values in native-relevant forward
  order.
- The system AI removes one system from the store, ticks it with mutable
  Simulation access, and reinserts it at the same stable ID.
- Smoke, Gas, and Fire use the generic fixed-point particle state and SHP
  renderer. Spark and Railgun remain rejected/no-op paths.

The current generic Particle stores fixed-point direction plus scalar velocity,
a u8 color index, and a fixed-point accumulator. Those fields cannot represent
the verified Spark state: three independent f32 velocities, a signed i32 color
index, and an f64 accumulator.

Rendering currently converts particles into SHP sprite instances. The tactical
visibility path stores an R8 shroud value and applies it through a later
fullscreen multiply. Scene depth uses Depth32Float. Active gamemd behavior 3
instead draws one direct point after sampling a complete u16 A word and applying
a strict read-only predicate against a u16 tactical Z word. Neither current
buffer has been proved equivalent.

The separate Spark particle-system and lighting design remains upstream. It
owns burst production and persistent-light lifecycle. This design owns only:

- Per-particle Spark-compatible numeric state and behavior-3 tick semantics.
- The deterministic compatibility arithmetic needed by that state.
- The point-path command, predicate, color, packing, and write contract.
- The shared interface through which an exact tactical A/Z producer will
  eventually provide samples.

Primary evidence:

- [docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md)
- [docs/research/PARTICLE_TIMING_SPARK_RAILGUN_NORMALIZED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_TIMING_SPARK_RAILGUN_NORMALIZED_GHIDRA_REPORT.md)
- [docs/research/PARTICLE_RNG_CLASSIFICATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_RNG_CLASSIFICATION_GHIDRA_REPORT.md)
- docs/contracts/2026-07-18-spark-collision-pixel-compositor-implementation-contract.md
- Live Ghidra roots 0x0062C6E0 and 0x0062CEC0.
- Live x87 startup and conversion chain at 0x007CD80F, 0x007CBDAF,
  0x007C8F46, 0x007CEAAF, 0x007CBF14, 0x007CC01C, 0x006BBFC1,
  0x006BBFC9, 0x007C5EE4, and 0x007C5F00.

## Impact Analysis

### Expected implementation surfaces

- src/util/native_x87.rs: integer-backed x87 compatibility subset.
- src/util/mod.rs: expose the compatibility types.
- src/rules/particle_type.rs: exact ColorSpeed representation and parsing
  boundary.
- src/sim/particles/mod.rs: SparkRuntimeState and module declaration.
- src/sim/particles/spark.rs: behavior-3 per-particle tick.
- src/sim/particles/system_ai.rs: forward Spark dispatch and reverse cleanup.
- src/map and read-only simulation query surfaces: candidate cell, ground,
  slope, live bridge bit, occupancy-order building, and wall overlay facts.
- src/sim/world/world_hash.rs and snapshot code: raw Spark state hashing and
  restoration.
- src/app_instances/particles.rs or the current app-render instance builder:
  ordered SparkPointCommand construction.
- src/render/tactical_compat.rs: shared A/Z frame view and pure point resolver.
- src/app_render/draw_passes.rs and related GPU resources: eventual exact point
  commit at a proven native-equivalent composition position.

### Dependency direction

The compatibility arithmetic belongs in util so rules, sim, and render may use
it without reversing dependencies. Simulation continues to depend only on
rules, map/world query surfaces, and util. It never depends on render, UI,
sidebar, audio, or net.

Rules provide exact numeric constants and byte color data. Simulation owns
authoritative Spark state. App-level render extraction creates immutable
commands. Render owns A/Z samples, display packing, point rejection, and
physical composition.

### Blast radius and risks

- Snapshot shape changes require coordinated SNAPSHOT_VERSION ownership and
  must not race another session's golden rebaseline.
- The native measured-performance latch belongs to the common particle draw
  gate. Making it Spark-local could change call-order behavior for mixed
  particle types.
- Current batching may regroup points or move them relative to overlapping
  tactical objects. A draw ordinal must survive extraction and commit.
- Applying the existing fullscreen shroud multiply to an already A-modulated
  point would darken it twice.
- Hardware f32/f64, compiler reassociation, unchecked shifts, and ordinary Rust
  overflow could each change authoritative bits.
- The current static bridge/deck facts are not yet proved equivalent to the
  live CellClass bit through collapse and repair.
- An assumed RGB565 display or an assumed Z multiplier would turn runtime
  blockers into hardcoded drift.

## Chosen Approach

Use a bounded, integer-backed software x87 compatibility kernel together with a
shared tactical integer point-composition interface.

Spark state stores raw IEEE f32/f64 bit patterns, but no Rust hardware float is
used by simulation. The compatibility kernel normalizes operands into an
integer significand/exponent form, performs only the verified operations, and
applies the process-established 53-bit precision and truncate-toward-zero mode
at each operation. Explicit f32 and f64 store methods reproduce native memory
rounding boundaries.

Above simulation, immutable Spark point commands enter a pure resolver. The
resolver consumes exact u16 A/Z samples and runtime display parameters and
returns either a rejection or one ordered packed-pixel write. The A/Z buffers
are shared tactical resources rather than Spark-owned mirrors.

This approach is selected because it:

- Preserves deterministic lockstep and the prohibition on sim floating point.
- Makes every native rounding/store boundary explicit and testable.
- Avoids platform-pinned 32-bit x87 as a production dependency.
- Avoids claiming the existing R8/Depth32Float paths are equivalent.
- Translates native semantics into Rust ownership instead of porting native
  classes, surfaces, or global pointer structures.

The 32-bit x87 instruction sequence may still be used as a test oracle, but it
is not production simulation code.

## Tiny-Detail Ledger

- Standard YR dispatches particle behavior 3 to the Spark AI root and stock
  Spark, WeldingSpark, FirestormSpark, and LargeSpark activate that behavior.
  [doc: [PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md)
  §Active-YR reachability]
- Authoritative widths are signed i32 coordinates/index, f32 X/Y/Z velocities,
  f64 accumulator, RGB bytes, signed i16 lifetime, and a deletion byte.
  [doc: same §Field ledger]
- CRT startup selects 53-bit precision; WinMain then selects truncation toward
  zero and captures control word 0x0E7F for Math__ftol. Math__ftol does not
  restore a prior mode. [GHIDRA 0x007CD80F, 0x007CBDAF, 0x007C8F46,
  0x007CEAAF, 0x007CBF14, 0x007CC01C, 0x006BBFC1, 0x006BBFC9,
  0x007C5EE4, 0x007C5F00]
- Persistent Z velocity stores old_vz minus Gravity, while the same-tick
  displacement stores old_vz minus two Gravity applications.
  [GHIDRA 0x0062C705..0x0062C71C, 0x0062C75E..0x0062C76A]
- Signed coordinates pass through explicit f32 stores and Math__ftol; candidate
  f32 additions are stored before candidate conversion. The integer candidate
  remains authoritative for cell/bridge work, while the stored candidate f32
  remains authoritative for ground/contact/clamp predicates and the final
  selected-coordinate Math__ftol commit.
  [doc: same §Process x87 control mode and exact store boundaries]
- World-lepton to cell conversion divides by 256 with signed truncation toward
  zero. Invalid cells use native dummy-cell semantics.
  [doc: same §Cell and ground semantics]
- Candidate ground uses the verified common 104-lepton Cell evaluator plus its slope-table
  contribution. Cell axes are not isometric screen axes.
  [doc: same §Coordinate and numeric-frame diagram]
- Structural crossing checks the old or candidate cell live bit 0x100. The
  plane is G+416; descending and ascending equality sides differ; ascending
  commits G+396. These corrected numeric values come from
  [PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md).
  [doc: same §Collision decision table]
- Near-ground clamping is strict: G-100 is not clamped. Building/wall contact
  is exactly [G,G+150).
  [doc: same §Collision decision table]
- Building contact selects the first WhatAmI==6 object in cell-list order and
  applies the LaserFence and one-by-one undeploy exclusions. Wall fallback
  accepts only overlay IDs 2, 0x1A, and 0xF3.
  [doc: same §Building and wall selection]
- The slope reflection uses inverse slope matrix, inverse matrix-vector
  multiplication, scalar-by-one of that stored local result, local-Z negation,
  forward matrix-vector multiplication, and final-Y negation. Its result is
  stack-local and never becomes persistent bounce velocity.
  [doc: same §Helper roles, frames, and precision]
- Coordinate/deletion commit precedes color RNG. A deletion marker does not
  skip that draw. Lifetime decrement follows color, and dead cleanup runs in
  reverse after forward AI.
  [doc: same §Commit, RNG, lifetime, and cleanup ordering]
- Every Spark tick draws exactly once from gameplay RNG range
  0..=0x7FFFFFFE. Arithmetic order is
  ((rng * (1/2147483646)) * 0.05) + ColorSpeed + old accumulator.
  [GHIDRA 0x0062CA72..0x0062CAB8; constants 0x007E3570 and 0x007E8AE8]
- Accumulator advancement branches only when strictly greater than 1.0.
  It increments/reset only when signed index is less than count-2; otherwise
  it stores exact 1.0.
  [doc: implementation contract §Color and point compositor order]
- Signed i16 lifetime decrement uses native wrapping; only a post-decrement
  value equal to zero deletes through lifetime. Starting zero becomes -1.
  [doc: same §Commit, RNG, lifetime, and cleanup ordering]
- Draw gates are performance latch with Damage override, extra-animation
  suppression, optional fog condition, behavior branch, projection/clip, A,
  Z, color/A/packing, then destination write.
  [doc: same §Early gates in exact order]
- Planar projection uses separate wrapping signed i32 `x*60`, `y*-60`, `x*30`,
  and `y*30` terms. Each term crosses signed `/2` before wrapping term addition
  and final signed `/256`; those stages must not be algebraically folded. Z
  adjustment uses the runtime multiplier, z>=728 correction, 0.5 bias, and
  Math__ftol.
  [doc: same §World-to-client projection]
- Tactical offsets are subtracted, radar viewport Y is then added, and clipping
  includes left/top but excludes right/bottom.
  [doc: same §World-to-client projection and §Clip boundaries]
- Index zero interpolates per-particle start RGB toward ColorList[1]. A nonzero
  index uses ColorList[index] toward ColorList[index+1]. Each channel uses
  native f64 operation order plus Math__ftol with no clamp. One
  `1.0 - accumulator` x87 value is retained across all three channels; each
  channel adds `next_term + current_term` in that order.
  [doc: same §Color source and interpolation]
- A is a zero-extended u16. Zero rejects; 1..126 uses signed IMUL and arithmetic
  shift right by seven; 127..65535 leaves channels unchanged.
  [doc: same §A-buffer address, width, and modulation]
- Z base arithmetic wraps to u16 before zero-extension. The signed candidate
  must be strictly less than the zero-extended stored u16. Equality rejects and
  Spark never writes Z.
  [doc: same §Z-buffer address and strict predicate]
- Channel loss shifts are arithmetic, channel placement and masking use runtime
  DirectDraw values, and the writer stores a u16 only on a two-byte surface;
  otherwise it stores the low byte.
  [doc: same §DirectDraw packing and destination write]
- A failed destination lookup/lock produces no pixel, the caller ignores the
  failure, and no Z value changes.
  [doc: same §DirectDraw packing and destination write]
- Behavior 3 produces one point during object rendering, never a particle SHP
  or white quad. A applies once and the point precedes persistent lights.
  [doc: same §Visual/UI composition ledger]
- Hashing and snapshots preserve every raw Spark bit, signed index, coordinate,
  lifetime, deletion state, particle order, and gameplay RNG cursor.
  [doc: implementation contract P20 and AT-13]

## Design

### Components

#### Native x87 compatibility module

The util module owns:

- NativeF32Bits backed by u32.
- NativeF64Bits backed by u64.
- An internal normalized 53-bit value represented only with integers.
- A fixed X87_CHOP_53 execution configuration.
- Explicit load, arithmetic, compare, memory-store, and integer-conversion
  functions.

The required surface is conceptually:

~~~text
load_i32
load_f32_bits
load_f64_bits
add_chop53
sub_chop53
mul_chop53
compare_chop53
store_f32_chop
store_f64_chop
ftol_i64_chop
~~~

The module does not expose Add, Sub, or Mul operator implementations. Callers
must name each native operation so evaluation order cannot be hidden by a Rust
expression or compiler reassociation.

The finite stock Spark domain is required. Signed zero and raw input bits are
preserved. NaN, infinity, subnormal, invalid-conversion, and overflow behavior
remain UNCHECKED until the native oracle covers them; implementation must not
silently canonicalize or claim full-domain verification before that evidence.

#### Exact particle rules

ParticleType stores ColorSpeed as NativeF64Bits instead of routing the literal
through f32 and SimFixed. The parser boundary retains enough source context to
report the section, key, and literal on failure.

The exact gamemd decimal conversion is not yet proved. The interface is designed
for a deterministic integer decimal-to-binary implementation, but stock .13
must first be captured from retail memory or the parser must be verified against
the native INI reader. Rust's ordinary parse result is not authority.

ColorList and start colors remain byte triplets from merged rulesmd-over-rules
data. No hardcoded Spark palette is introduced.

#### SparkRuntimeState

Particle receives an optional behavior-specific state:

~~~text
SparkRuntimeState
  velocity_x: NativeF32Bits
  velocity_y: NativeF32Bits
  velocity_z: NativeF32Bits
  start_rgb: [u8; 3]
  color_index: i32
  color_accumulator: NativeF64Bits
~~~

Existing signed coordinate, signed lifetime, and deletion fields remain on
Particle when their widths already match. Generic fixed-point direction,
scalar velocity, u8 index, and SimFixed accumulator are not authoritative for
behavior 3.

Using an optional bounded state avoids replacing the existing Particle model,
introducing a second store, or forcing Smoke/Gas/Fire through the compatibility
kernel.

#### Spark collision queries

Simulation exposes the smallest read-only queries required by the tick:

- Signed lepton-to-cell conversion with truncation toward zero.
- Candidate cell terrain level, slope, and ground height.
- Live structural bridge bit for old and candidate cells.
- First building in native-equivalent occupancy list order.
- Building type/runtime facts for LaserFence and undeploy exclusion.
- Candidate-cell overlay ID for wall fallback.

These are mechanism queries, not a native CellClass or BuildingClass port.
They preserve current deterministic ownership and list order.

#### Spark tick owner

The Spark system AI follows the existing remove/tick/reinsert pattern. It
iterates ParticleSystem.particles forward, completes the full movement,
collision, color, and lifetime sequence for one particle, then advances.
Afterward it removes marked particles in reverse.

The system remains unavailable through the public spawn path until its upstream
burst producer, bridge prerequisite, and render prerequisites are ready.

#### SparkPointCommand

App-level extraction creates an immutable command containing:

- World coordinate.
- Raw accumulator and signed index.
- Per-particle start RGB.
- Stable particle type/color-list reference or the exact selected byte pairs.
- Behavior, Damage, and other draw-gate inputs.
- A native draw ordinal that survives collection and commit.

Simulation does not construct or import this render type.

#### TacticalCompatBuffers and frame view

Render owns a shared tactical resource:

~~~text
TacticalCompatBuffers
  a_words: u16 storage
  z_words: u16 storage
  dimensions and pitches

TacticalCompatFrame
  immutable A/Z frame view
  tactical viewport and offsets
  radar viewport Y offset
  NativeF64Bits AdjustForZ multiplier
  DirectDrawPixelFormat loss/shift tuple
  destination bytes-per-pixel contract
~~~

The resource is shared because brackets, concealment, and other native
tactical consumers use the same substrate. Spark cannot manufacture a private
R8 or float-depth mirror and call it equivalent.

#### Pure point resolver and ordered committer

The pure resolver accepts one SparkPointCommand and one TacticalCompatFrame.
It runs gates, projection, clip, A/Z reads, color selection/interpolation, A
modulation, and packing. It returns either:

- A typed rejection reason for diagnostics/tests; or
- PackedPointWrite containing coordinate, packed value, byte width, and draw
  ordinal.

The ordered committer applies accepted writes without regrouping them by
texture, color, particle system, or any other batch key.

Production integration must either write into a native-compatible tactical
surface at the proven object-rendering position or provide an exhaustively
equivalent wgpu composition path. The current fullscreen shroud multiply is not
an authorized adapter.

### Interfaces / Contracts

#### Arithmetic contract

- Every native instruction-level arithmetic operation is a separate call.
- Every verified f32/f64 memory store is a separate quantization call.
- All integer overflow, narrowing, and shifts use explicitly named wrapping or
  arithmetic semantics.
- Math__ftol first produces the native signed 64-bit conversion result; callers
  explicitly consume the native-width portion required at each callsite.
- Raw values are hashed and serialized exactly; no -0 or NaN canonicalization
  is allowed.

#### Simulation contract

- One forward particle tick consumes at most one color RNG draw, and every
  active Spark particle consumes exactly one regardless of collision deletion.
- No render gate may suppress simulation, lifetime, collision, or RNG.
- Collision reads authoritative map/world facts and commits before color.
- Reverse cleanup happens in the same owning system tick.
- Public Spark activation is atomic: no partial path may silently run generic
  particle motion or generic SHP output.

#### Render contract

- The common particle performance latch is evaluated in native traversal order.
- Point resolution is read-only with respect to A and Z.
- A is applied exactly once.
- Accepted writes retain source order.
- No behavior-3 SHP instance or generic depth test is emitted.
- Missing exact resources return unavailable; they never select an approximate
  fallback.

#### Safe malformed-state boundary

Native out-of-range ColorList indexing is unchecked memory access. Rust uses
checked access and returns an explicit InvalidColorIndex rejection. It does not
clamp, wrap, substitute ColorList[0], or use unsafe memory. Valid stock state
must remain bit exact; malformed native behavior is documented as an explicit
compatibility boundary.

### Data Flow

#### Rules to spawn

1. Merge rules.ini then rulesmd.ini overrides.
2. Preserve behavior, ColorList, start colors, velocity inputs, and system links.
3. Convert ColorSpeed through the exact native-double parser boundary.
4. Spark construction selects start color and initializes raw velocities,
   signed index zero, and exact accumulator zero in native RNG order.
5. Attach SparkRuntimeState to the Particle.

#### Authoritative Spark tick

1. Store persistent old_vz minus Gravity as f32.
2. Subtract Gravity again and store the probe Z as f32.
3. Convert each old signed coordinate to stored f32.
4. Execute old-coordinate Math__ftol calls.
5. Add stored coordinate floats and stored displacement floats in native order.
6. Store each candidate float and convert it through Math__ftol for the integer
   cell/bridge candidate without discarding the stored candidate float.
7. Query the integer candidate cell, ground, slope, bridge, building, and wall
   facts.
8. Run integer bridge predicates, raw candidate-f32 ground/contact/clamp
   predicates, and the exact transient slope transform sequence.
9. Convert the final selected X/Y/Z stored floats through Math__ftol again, then
   commit the coordinate and deletion byte.
10. Draw RNG once from 0..=0x7FFFFFFE.
11. Compute, without reassociation:

~~~text
scaled_0 = rng * (1 / 2147483646)
scaled_1 = scaled_0 * 0.05
with_speed = scaled_1 + ColorSpeed
new_accumulator = with_speed + old_accumulator
~~~

12. Store f64, then apply the strict greater-than-one advancement rule.
13. Decrement signed i16 lifetime with wrapping behavior and test equality zero.
14. After all forward ticks, remove dead particles in reverse.

#### Render extraction and point resolution

1. Traverse renderable objects/particles in the native-equivalent order.
2. Create commands with stable draw ordinals; do not tick or consume RNG.
3. Evaluate performance, detail, optional fog, and behavior gates.
4. Project with each wrapping `60`/`30` multiplication followed by its own
   signed `/2`, then wrapping term addition and final signed `/256`.
5. Compute AdjustForZ from the runtime multiplier, threshold, bias, and
   Math__ftol.
6. Apply tactical/radar offsets and inclusive/exclusive clipping.
7. Load complete u16 A; zero rejects.
8. Form wrapped-u16 Z base and signed candidate; strict comparison decides.
9. Select the exact current/next color pair.
10. Compute once and retain across all three channels:

~~~text
one_minus_a = 1.0 - accumulator
~~~

11. For each channel, perform:

~~~text
next_term = next * accumulator
current_term = current * one_minus_a
channel = Math__ftol(next_term + current_term)
~~~

12. Apply the u16 A threshold rule once.
13. Pack through runtime loss/shift parameters.
14. Return and commit one ordered point write without changing Z.
15. Preserve later persistent-light composition.

### Error Handling

- Invalid INI numeric literals produce a structured rules error.
- Unsupported exceptional numeric inputs produce an explicit compatibility
  error until their native behavior is verified.
- Invalid ColorList state produces an explicit render rejection and no point.
- Missing A/Z resources, multiplier, or display packing produces compositor
  Unavailable, not a fallback.
- Destination address/lock failure produces no write and otherwise does not
  alter state.
- Development assertions may protect impossible internal states, but valid
  gameplay input must not panic.

### Testing Strategy

#### Arithmetic differential tests

Compare raw input/output bits at every operation and store boundary against
retail/x87-derived fixtures:

- Integer to f32 stores around precision boundaries.
- Positive and negative truncation.
- f32 add/subtract and double-gravity fixtures.
- f64 RNG/color accumulation in the exact operation order.
- Math__ftol boundaries and signed results.
- Explicit examples where fixed point or ordinary round-to-nearest differs.

An x86 helper can accelerate oracle generation, but final VERIFIED claims must
name retail/gamemd-derived captured evidence or an exhaustive proof. Rust versus
Rust goldens are regression checks only.

#### Simulation tests

Carry forward contract AT-1 through AT-7 and AT-13 through AT-15:

- Raw state round trip.
- Flat-ground double-gravity trace.
- Negative coordinate and ground-clamp boundaries, including a raw `-0.5`
  candidate that integer-converts to zero but still takes the below-ground path.
- Healthy bridge crossing predicates and equality.
- Building/wall contact and occupancy rebuild order.
- Forward RNG/lifetime and reverse cleanup order.
- Color source/advancement boundaries.
- Hash and snapshot sensitivity for each authoritative field.
- Stock merged-rule values and exact ColorSpeed bits.
- Destroyed/repaired bridge transition after its investigation.

#### Point-kernel tests

Carry forward AT-8 through AT-12:

- Render gates do not affect simulation.
- Projection wrapping before each `/2`, including `(50_000_000,0,0)` and
  `(i32::MAX,1,0)`, plus all clip edges.
- Complete A thresholds 0, 1, 126, 127, 128, and 65535.
- Z candidate-1/equal/+1, negative candidate, above-u16 candidate, and wrapped
  base.
- Index-zero and nonzero color pairs, one retained `1.0-a` interpolation value,
  native add order, and no clamp.
- Runtime channel packing for captured display layouts.
- One accepted destination pixel, unchanged Z, no SHP, stable ordering, one A
  application, and later light composition.

#### Retail certification

Execute contract AT-16 in a controlled retail tactical session:

- Capture g_AdjustForZ_Multiplier.
- Capture all six DirectDraw channel loss/shift globals.
- Break on stock WeldingSpark at 0x0062CEC0.
- Record raw particle fields, projected point, A/Z words, Z candidate,
  interpolated channels, packed value, gate states, and draw ordinal.
- Observe 0x007BAEB0 and read back the destination pixel.
- Exercise A and Z thresholds, clip edges, and a negative-coordinate fixture.
- Feed the exact capture into Rust and require identical rejection/predicate,
  coordinate, packed value, touched bytes, and final destination result.

Focused tests run serially. Before a final Cargo check, active cargo/rustc
ownership is checked. Only edited Rust files are formatted during eventual
implementation.

## Activation Gates and Deferred Prerequisites

The design is implementation-ready for the software arithmetic surface,
Spark state ownership, injected-input simulation tests, command representation,
and pure point resolver. End-to-end public Spark activation remains blocked by:

1. Exact native ColorSpeed parsing/value capture.
2. Cell structural bit 0x100 lifecycle through bridge collapse and repair.
3. Exact tactical A-buffer producer/frame composition.
4. Runtime AdjustForZ multiplier.
5. Runtime DirectDraw loss/shift tuple.
6. Retail final-pixel oracle.
7. Proven renderer overlap/order and single-A composition.
8. The separate upstream Spark burst producer and light lifecycle.

No blocker may be replaced by a guessed constant or current approximate buffer.

## Architectural Decisions

- Follow the existing BTreeMap system-store and Vec particle-order pattern.
- Add behavior-specific state instead of replacing generic particles or adding
  an ECS.
- Put numeric compatibility in util to preserve dependency direction.
- Use raw-bit state and integer arithmetic instead of hardware simulation
  floats.
- Keep renderer data immutable across the sim/render boundary.
- Use a shared tactical A/Z substrate rather than a Spark-only mirror.
- Preserve point order explicitly rather than assuming batching is harmless.
- Reject unavailable exact inputs instead of silently selecting drift.
- Keep native unchecked ColorList behavior memory-safe through an explicit
  malformed-state boundary.
- Do not recreate x87 global state, C++ inheritance, DirectDraw vtables, circular
  pointer buffers, or native singleton ownership.

The deliberate new pattern is the bounded software x87 compatibility module.
It is justified because the current fixed-point pattern cannot reproduce the
verified native store/rounding semantics, while direct Rust floats would violate
deterministic simulation policy.

## Alternatives Considered

### Platform-pinned hardware x87

A 32-bit native helper could execute the original instruction form with less
initial arithmetic code. It was rejected for production because results and
control state would depend on platform, compiler, calling context, and process
mode. It also conflicts with the simulation no-floating-point rule. It remains
useful only as a differential oracle.

### Existing fixed-point simulation plus current GPU buffers

This would reuse SimFixed, R8 shroud, Depth32Float, and the SHP particle pass.
It is rejected as known DRIFT: it loses f32/f64 store boundaries, x87
truncation, the complete u16 A domain, wrapped integer Z, strict equality,
runtime DD packing, one-point write behavior, and single A modulation.

### Precomputed transition tables

Tables could encode selected stock motion/color transitions, but the state
space includes signed coordinates, three f32 velocities, a 31-bit RNG draw,
f64 accumulation, terrain/collision inputs, A/Z samples, and runtime display
parameters. A bounded table cannot cover the active domain and would hide
unverified fallbacks. It was rejected.
