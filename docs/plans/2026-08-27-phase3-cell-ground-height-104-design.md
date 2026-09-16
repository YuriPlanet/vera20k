# Phase 3 Cell ground-height 104-domain correction design

**Phase/GSI ownership hypothesis:** Phase 3 / GSI-04.03, with shared lookup
substrate dependencies in GSI-04.01

**Bounded mechanism:** the active-retail `CellClass` ground-height scalar,
slope evaluator, and the Rust consumers currently routed through the false
90-lepton duplicate evaluator

**Evidence:**
[docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md)

**Integration rebase (2026-08-30):** This design was authored on the source
branch, where a separate naval-placement slice existed and the accumulated
snapshot version was 110. Current `origin/main` has no naval-placement module
or its two documents, and this integration intentionally does not restore that
later AI/naval scope. The current slice therefore migrates four active
production callsites and advances the actual mainline snapshot contract
`105 -> 106`. Source-branch naval facts below are retained only when explicitly
labelled as historical/later-owner context.

## Verdict first

Active retail does not have a 90-lepton Cell ground domain. The executable
independently initializes the Cell, Foot, Techno/InRange, area-damage, Anim,
Bullet, Particle, Unit, and VXL level scalars, but every captured value is 104.
`CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0` consumes the Cell-owned
104 directly. The nearby `90.0` literal is an angle used to derive `pi/6`, not
a height.

Rust already has the correct 104 evaluator and most consumers use it. A later
regression added a second 90 evaluator. The source branch routed five
production callsites through it; current main retains four because the
separate naval-placement slice is absent. This integration removes that false
numeric split from all four active callers, preserves Spark's already-correct
104/416 composition, rebaselines exact fixtures, and corrects the load-bearing
stale documentation that introduced the split.

This slice deliberately does not claim all of GSI-04.03 closed. The shared
dummy/unavailable-cell behavior of height callers, shipped-map slope 17..20
reachability, and remaining cliff/height consumer questions stay explicit row
work after this numeric mechanism passes.

## 1. Native contract

### 1.1 Scalar and slope formula

The Cell-owned scalar at `0x0089E7C0` is 104. For a signed `i8` Level byte and
unsigned slope byte:

```text
base = ftol_chop(sign_extend_i8(Level) * 104 + 0.5)
raw  = (local_y * coeff_y + local_x * coeff_x) * (104 / 256)
       + bias_a + bias_b
slope_term = clamp(raw, 0, maximum)
height = ftol_chop(base + slope_term)
```

Local X/Y are the unsigned low bytes of the world coordinates. Slope 0 is
flat; records 1..20 use the verified table. Rust must safely reject slopes
above 20 rather than imitate native's unsafe out-of-table read.

Flat examples are `Level 0 -> 0`, `1 -> 104`, `2 -> 208`, and byte `0xFF`
(`-1`) -> `-103`. At cell center, slope contributions 0..20 are:

```text
0, 52, 52, 52, 52,
0, 0, 0, 0,
104, 104, 104, 104, 104, 104, 104, 104,
52, 52, 52, 52
```

### 1.2 Floor and deck remain separate

`CellClass::GetCoords @ 0x00486840` and
`CellClass::Get_Center_Coords @ 0x00480A30` return the 104-based ground only.
`CellClass::GetTargetCoords @ 0x00486890` adds the separately initialized 416
only when raw `CellClass+0x140 & 0x100` is set. The common ground evaluator
must never include bridge height.

Spark calls `CellClass::GetGroundHeight @ 0x00578080` and therefore already
uses 104. Its structural plane is `ground + 416`, and its ascending commit is
`ground + 396`. Current Rust's Spark numeric choices match and must not change.

### 1.3 Authority and exclusions

- No rules, art, theater, map, or scenario INI writes these scalars.
- Independently owned native globals do not imply different numeric domains.
- No active symbol `MapClass::GetZPos` was verified; implementation comments
  cite the proven Cell methods instead.
- `90.0`, `RadLevelDelay=90`, and other timing/angle values are unrelated.
- Malformed slopes above 20 remain a safe Rust error boundary.

## 2. Current architecture and exact mismatch

### 2.1 Correct authority to preserve

`src/util/lepton.rs` already owns:

- `LEPTONS_PER_LEVEL = 104`;
- `GROUND_LEVEL_HEIGHT_LEPTONS = 104`;
- one 104-derived 21-entry slope table;
- `ground_height_leptons`, whose signed base, low-byte local coordinates,
  clamp order, and chop behavior match native.

`src/sim/cell_kernel.rs::cell_floor_height` and current Spark, Anim, radar
visibility, combat/AoE, range, miner, overlay, smudge, radiation, lifecycle,
tube, and runtime paths already consume this authority.

### 2.2 False duplicate to remove

`src/util/lepton.rs` also owns the incorrect:

- `CELLCLASS_GROUND_LEVEL_HEIGHT_LEPTONS = 90`;
- `CELLCLASS_GROUND_SLOPE_RECORDS` derived from 90;
- `cellclass_ground_height_leptons` wrapper;
- tests and prose asserting a separate 90 surface.

Four production consumers present on the current integration base used that
duplicate before this slice:

1. `src/render/minimap_interaction.rs` radar-click Cell center Z;
2. `src/sim/projectile.rs` allocated real Cell target Z;
3. `src/sim/projectile.rs` retained shared-dummy Cell target Z;
4. `src/sim/production/production_spawn.rs` produced-unit Cell center
   evaluation. This helper presently discards the computed Z after forming a
   center and returns only X/Y-derived Cell coordinates, so changing 90 to 104
   does not itself move the spawned unit. The call still must migrate because
   it executes the native height/unsupported-slope gate and the duplicate API
   must disappear.

The source branch also had a fifth caller in
`src/sim/naval_base_placement.rs`. That module and its design/research files
are absent from current main and are intentionally not restored by this slice.
Its verified 104 requirement belongs to the later owner if that naval
mechanism is integrated separately.

The duplicate makes flat Level 2 produce 180 instead of 208. Structural
Level-2 targets become 596 instead of 624, and Sonic's additional +50 becomes
646 instead of 674.

### 2.3 Lookup behavior is not silently folded into this numeric change

The repository already has the exact fixed-512 real-or-shared-dummy substrate
in `src/sim/cell_rect.rs::{get_cellclass_fallback,
get_cellclass_fallback_leptons}`. Production spawn and retained projectile
dummy targets already use live dummy state. Other consumers still have
caller-specific absence behavior:

- minimap camera resolution returns `None` for an unavailable clamped Cell;
- Spark returns `UnavailableCell`/`OutOfRangeCell` instead of evaluating the
  shared dummy;
- the stable projectile helper's headless `unwrap_or(0)` is not the same as a
  live nonzero dummy, while the distinct retained-dummy target path is live.

Those differences remain open and must not be described as exact after this
slice. They require their own caller/lifecycle proof because changing lookup
selection affects overlay, bridge, occupancy, target identity, and mutation
order in addition to numeric height.

## 3. Design options

### Option A — change the duplicate constant from 90 to 104

This is numerically sufficient for current callsites but retains two slope
tables and two APIs with identical active behavior. It preserves the exact
failure mode that allowed the stale claim to diverge again. Rejected.

### Option B — retain a Cell-named alias routed through the common evaluator

This documents independent native ownership and eliminates numeric drift, but
the alias still encourages callers to infer a semantic domain split that the
binary disproves. It also leaves duplicate production vocabulary and test
surface without providing a Rust ownership boundary. Rejected unless a compile
dependency discovered during implementation makes immediate removal unsafe.

### Option C — one verified evaluator and explicit provenance at consumers

Remove the false Cell constant, duplicate table, and wrapper. Route all four
active current-main callers through `ground_height_leptons`, while their nearby
comments retain the specific native Cell method and bridge-selection
provenance. Preserve independent semantic composition at the caller: ground
only for GetCoords, ground plus conditional 416 for GetTargetCoords. Selected.

## 4. Required implementation

### 4.1 Consolidate the evaluator

In `src/util/lepton.rs`:

1. Correct the 104 constants' documentation to state that active retail's
   independently initialized ground/object/VXL scalars resolve to the same
   value.
2. Remove `CELLCLASS_GROUND_LEVEL_HEIGHT_LEPTONS`.
3. Remove `CELLCLASS_GROUND_SLOPE_RECORDS`.
4. Remove `cellclass_ground_height_leptons`.
5. Make the `ground_height_leptons` documentation cite
   `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0` and describe the shared
   verified 104 numeric formula without implying bridge inclusion.
6. Replace the false 90-specific unit test with one discriminating 104 Cell
   contract covering flat levels, signed `0xFF`, all 0..20 center records, and
   unsupported 21.
7. Correct `src/sim/cell_kernel.rs::cell_floor_height`'s invented
   `CellClass::GetFloorHeight` label to the verified inner
   `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0` identity.

No floating-point approximation, new table, or schema field is introduced.

### 4.2 Route all false consumers

Replace the removed helper at each of the four active production callsites with
`ground_height_leptons`. Keep existing safe unsupported-slope handling and
existing caller-specific lookup selection unchanged in this numeric slice.

Do not restore the source branch's deleted naval-placement caller. If a later
owner integrates that mechanism, it must use the same evaluator under its own
scope and review.

Update nearby source comments so they say:

- Cell ground is 104;
- GetCoords remains ground-only;
- GetTargetCoords adds 416 only for raw structural bit `0x100`;
- Spark already uses the same ground evaluator;
- independent native globals are ownership detail, not numeric domains.

No consumer may multiply Level directly as a shortcut; every slope-aware path
uses the common evaluator.

### 4.3 Rebaseline dependent tests and snapshots

Update assertions whose only difference is the corrected ground result,
including the known projectile/dummy, bridge, combat, snapshot, lifecycle,
world/Wave, minimap, and production fixtures found by repository-wide
search. Expected compositions include:

```text
Level 2 ground                       = 208
Level 2 structural Cell target       = 624
Level 2 structural Cell + Sonic 50   = 674
Spark Level 2 structural plane       = 624
Spark Level 2 ascending commit       = 604
```

This changes deterministic behavior without changing serialized shape. The
repository's snapshot contract rejects old bytes whenever they would resume
under different authoritative logic: versions 78 and 79 already establish
behavior/hash-only precedents. A v105 save can retain a Cell target or pending
world state that produces different raised-terrain results after this repair.
Therefore `SNAPSHOT_VERSION` advances 105 -> 106, the version history records
the behavior-only ground-domain boundary, the exact version test moves to 106,
and v105 rejection remains covered. Stored expected hashes/coordinates are
rebaselined only where the verified 104 result flows into them.

### 4.4 Correct stale evidence and design prose

Update the load-bearing wrong claims in:

- `docs/research/PARTICLE_SPARK_LIVE_COLLISION_INPUTS_GHIDRA_REPORT.md`;
- [docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md);
- `docs/plans/2026-07-18-spark-native-float-and-point-compositor-design.md`;
- `docs/plans/2026-07-18-spark-live-collision-adapter-and-owner-design.md`.

The two naval-placement documents named by the source-branch design are absent
from current main and remain excluded with their implementation. This
integration does not recreate them merely to patch historical prose.

Each correction cites the new active-runtime census and changes 90 -> 104,
`G+360` -> `G+416`, and `G+340` -> `G+396` where applicable. It must not
rewrite unrelated historical design decisions. Older documents that clearly
label 90 as approximate/unverified remain historical leads; they are not used
as active evidence.

The live-collision report's stale lookup conclusion is corrected as well:
Rust now has a mutable process-global shared-dummy substrate, but Spark's
adapter is not yet routed through it and still returns typed unavailable/off-
array errors. The substrate is present; Spark integration remains an open
caller-specific parity gap. A supersession banner alone is acceptable for a
large historical report only if it names every invalid numeric conclusion and
the dummy-routing correction prominently enough that its old handoff cannot be
mistaken for current evidence.

Do not mechanically replace unrelated 360 values. In particular, the
Ship-locomotion under-bridge Z-offset owns a separate native global and remains
outside this Cell/Spark numeric slice until independently verified.

## 5. Acceptance ledger

### Evaluator

- Flat `0,1,2,0xFF` return `0,104,208,-103`.
- Slope 0..20 at center yields the exact sequence in section 1.1.
- Low-byte local XY, clamp-before-base, final chop, and signed negative behavior
  remain covered at representative boundaries.
- Slope 21 returns `UnsupportedGroundSlope(21)`.
- No `CELLCLASS_GROUND_LEVEL_HEIGHT_LEPTONS`, duplicate Cell slope table, or
  `cellclass_ground_height_leptons` identifier remains.

### Production consumers

- Radar click on a raised Cell reports the 104-based center Z and still excludes
  bridge height.
- Real and retained-dummy projectile targets use 104 ground; raw structural bit
  adds exactly 416.
- Production spawn evaluates the selected real-or-dummy Cell through the 104
  authority and retains the native unsupported-slope gate, while its current
  X/Y-only returned spawn Cell remains unchanged because the intermediate Z is
  discarded.
- Sonic/Wave target composition on Level 2 structural terrain is 674.

### Preservation

- Spark Level-2 floor is 208, plane is 624, ascending commit is 604, and
  Level-0 plane remains 416.
- Particle construction clamps input `Z <= ground` to the 104-based floor.
- Existing correct 104 consumers do not change APIs or add bridge height.
- Bridge selection changes only the conditional +416 term.
- No RNG, scheduling, ownership, object identity, or serialized layout changes;
  snapshot compatibility advances to 106 for the deterministic behavior-only
  boundary.
- No deleted naval-placement module or document is restored. Its 104-based
  candidate requirement remains a later-owner obligation.

### Documentation and residual honesty

- Active source/research/design prose no longer calls Cell ground 90.
- Stale `G+360`/`G+340` Spark claims are corrected.
- Caller-specific unavailable-cell/shared-dummy divergences remain explicitly
  open; no test or comment calls them exact merely because the scalar is fixed.

## 6. Validation and review cadence

The builder checks `Get-Process cargo,rustc` before every Cargo command and
runs only focused library filters, for example:

```text
cargo test -p vera20k --lib ground_height
cargo test -p vera20k --lib native_click
cargo test -p vera20k --lib dummy_target
cargo test -p vera20k --lib sonic
cargo test -p vera20k --lib production_spawn
cargo test -p vera20k --lib spark
cargo test -p vera20k --lib snapshot
```

The integration owner runs the full `cargo test -p vera20k --lib` exactly once
after the final critic pass and before declaring the small PR ready for main.

After the builder commits, a fresh read-only critic receives this design, the
native census, the full diff, and literal focused output. PASS requires zero
open, approximate, missing, or regressed behavior inside this numeric domain
mechanism. Any finding is repaired by the same builder and resubmitted to a new
critic. GSI-04.03 remains open after PASS for the separately recorded lookup,
slope-reachability, cliff, and remaining height-consumer work.

## 7. Decision

Option C is the smallest architecture-correct repair. It restores active
retail's one numeric ground formula, preserves the correct Spark path and
caller-owned bridge composition, deletes the duplicate drift surface, and
leaves distinct lookup-lifecycle disparities visible for their own evidence
and builder/critic loop.
