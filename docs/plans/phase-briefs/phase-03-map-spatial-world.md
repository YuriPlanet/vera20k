# Phase 3 brief — Map and spatial world

Rows 37–52 of the [implementation order](../2026-07-30-clean-slate-system-implementation-order.md).
State: **IN PROGRESS; every row remains open**. Baseline: fetched `origin/main`
`ed8f4837910be9329505c3dfc2fc074d9c1f3106`, integrated on 2026-09-11 before final gap-operation validation.
This is the current investigation frontier, not a completed mechanism census.

## Rows

Historical registry values below are native evidence / Rust implementation / parity from
[registry.v2.json](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/system-map/registry.v2.json). They are inherited status,
not newly established equivalence. The System Map was retired on 2026-09-10;
do not maintain these inherited values as current status. Paths are representative owners;
references are starting evidence whose applicability must be checked per mechanism.

| Row | GSI | Rust owner(s) | Native anchor or research starting point | Registry | Notes |
|---|---|---|---|---|---|
| 37 | GSI-04.01 | `src/map/cell_index.rs`, `playfield.rs`, `resolved_terrain.rs`; `src/sim/cell_rect.rs` | Get_CellClass `0x005657A0`, world lookup `0x00565730`, IsCellInPlayfield `0x00578460`; [dummy contract](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_GET_CELLCLASS_FALLBACK_DUMMY_CELL_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | ClipRect operand-order repair and bounded native lookup comparisons merged in PR #321. Constructor/iterator and unmodeled-state candidates require separate proof. |
| 38 | GSI-04.02 | `src/map/theater.rs`, `resolved_terrain.rs` | CalculateLegacyMapTileIndex `0x00544E30`; [translation](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_LAST_TILES_IN_SET_COMPATIBILITY_TRANSLATION_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Load, Fill, map-pack and generated-map paths must be checked separately. |
| 39 | GSI-04.03 | `src/util/lepton.rs`; `src/sim/cell_kernel.rs`; `src/map/resolved_terrain.rs` | ComputeGroundHeightAtCoord `0x0047B3A0`; [domain census](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | The inherited 90-lepton Cell claim is false: both use 104. Ground-only and caller-selected 416-lepton deck composition are distinct. |
| 40 | GSI-04.04 | `src/sim/cell_kernel.rs`, `overlay_grid.rs`, `pathfinding/terrain_cost.rs` | [RecalcZoneType](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_RECALCZONE_TYPE_00483C80_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Preserve synchronous publication before the next reader. |
| 41 | GSI-04.06 | `src/sim/pathfinding/zone_map.rs`, `zone_build.rs`; `src/sim/world/navigation.rs` | FloodFillReachableZones `0x005840C0`; [flood fill](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_FLOODFILLREACHABLEZONES_005840C0_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Tube hierarchy registration and ordered path entry landed in PR #327. Flood publication, remaining lookup domains and lifecycle still require audit; see inherited residuals. |
| 42 | GSI-04.05 | `src/sim/occupancy.rs`, `cell_rect.rs`; `src/sim/world/lifecycle.rs` | AddContent `0x0047E8A0`, RemoveContent `0x0047EA90`; [live list writers](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_SUBSTRATE_LIVE_OBJECT_LIST_WRITERS_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Cell lists, layer transitions and reservations are distinct authorities. Do not import all AI behavior merely because it reads occupancy. |
| 43 | GSI-04.07 | `src/map/authored_overlay.rs`; `src/sim/overlay_grid.rs` | OverlayClass::Mark `0x005FC570`, DestroyOverlay `0x00480CB0`; [authored boundary](../../research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAYPACK_INLINE_TRANSACTION_REINVESTIGATION_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Includes mutation order, ownership, shared dummy effects and projection to consumers. |
| 44 | GSI-04.09 | `src/sim/tiberium/mod.rs`, `overlay_grid.rs`; `src/map/authored_overlay.rs` | Reduce_Tiberium `0x00480A80`; [quantity mutation](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Identity and quantity are this row; growth/harvesting policy enters only as a proved prerequisite or consumer. |
| 45 | GSI-04.10 | `src/sim/terrain_object.rs`, `terrain_spawn.rs` | [TerrainClass timing](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_TERRAINCLASS_AI_TIMING_AND_RNG_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Verify ordinary trees/rocks as well as TIBTRE; spawning is not full destruction/fire coverage. |
| 46 | GSI-04.12 | `src/sim/bridge_state/mod.rs`, `src/map/bridge_facts.rs`, `src/sim/world/bridge_orchestrator.rs` | [bridge coverage](../../research/bridges/00-system-models/ACTIVE_RETAIL_BRIDGE_COVERAGE_REINVESTIGATION_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Existing bridge work is substantial but explicitly incomplete. Fresh-load record restamping merged in PR #329 after validation at `8b147e5e`; broader bridge lifecycle remains open. |
| 47 | GSI-04.13 | `src/map/bridge_facts.rs`; `src/sim/movement/movement_bridge.rs` | [bridge coverage](../../research/bridges/00-system-models/ACTIVE_RETAIL_BRIDGE_COVERAGE_REINVESTIGATION_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Distinguish high bridge decks, wood bridges and low bridge tubes. |
| 48 | GSI-04.15 | `src/map/tubes.rs`, `tube_facts.rs`; `src/sim/movement/tube_movement.rs` | ReadTubesINI `0x007283C0`; [bridge coverage](../../research/bridges/00-system-models/ACTIVE_RETAIL_BRIDGE_COVERAGE_REINVESTIGATION_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / DRIFT | Active low-bridge tubes cannot be excluded as TS-only based on their name. |
| 49 | GSI-04.16 | `src/map/waypoints.rs`; `src/map/map_file.rs` | Read_Waypoints `0x0068BDC0`; [map substrate](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) | CONTRACTED / PARTIAL / DRIFT | Signed values and canonical key recovery already landed; audit starts/regions and downstream use separately. |
| 50 | GSI-04.18 | `src/sim/vision/mod.rs`; `src/sim/snapshot.rs` | [shroud reveal](../../research/SHROUD_REVEAL_SYSTEM_GHIDRA_REPORT.md) | ANCHORED / PARTIAL / UNCHECKED | Ordinary sight, transient mapping, pending conceal and retained source/viewer authority merged in PR #330. The operational correction publishes gap changes at the Building visit, retains per-player admission, and preserves lifecycle/SpySat ordering and snapshot144 state; see [bounded evidence and validation](../../research/PHASE3_GAP_OPERATIONAL_PUBLICATION_NATIVE_REPORT.md). Optional producers, discovery and the wider visibility domains remain open. Historical SHROUD_DISPARITIES is not a current gap list. |
| 51 | GSI-04.11 | `src/sim/smudge_grid.rs`, `combat/smudge_dispatch.rs` | [smudge class](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SMUDGE_CLASS_GHIDRA_REPORT.md), [spawn callers](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SMUDGE_SPAWN_TRIGGERS_GHIDRA_REPORT.md) | CONTRACTED / PARTIAL / UNCHECKED | Placement, caller-specific ore mutation, RNG order and restore require evidence. |
| 52 | GSI-04.20 | `src/map/lighting.rs`; `src/app/presentation/lighting.rs` | ApplyAreaLightConvert `0x00554AF0`; [light source lifecycle](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LIGHTSOURCE_LIFECYCLE_POWER_DAMAGE_SAVELOAD_GHIDRA_REPORT.md) | N/A / N/A / N/A (group) | Group status is not exclusion or closure. Separate active ambience, lamp lifecycle and global tint from dormant TS behavior. |

## Loops

The archived [topology](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/system-map/topology.v2.json) records representative
connections, not an exhaustive Phase 3 loop inventory:

- `LOOP-005-BUILD-PLACE`: cell passability and occupancy, stages 10–11.
- `LOOP-006-FACTORY-EXIT`: exit occupancy, stage 7.
- `LOOP-008-REVEAL-RADAR`: persisted per-cell knowledge, stage 5.

Trace unlisted map-load, overlay, terrain, bridge and lighting transactions from
their actual entrypoints. A downstream reader alone does not make its entire
owning system a Phase 3 prerequisite.

## Prior work

The 2026-09-10 increment at `258a59ce` repairs LocalSize clipping operand order
and adds original-executable spatial comparisons. Its [evidence report](../../research/PHASE3_MAP_SPATIAL_NATIVE_COMPARISON_20260910.md)
records the failing pre-fix result, corrected production path, independent
review, four passing focused tests, full library results (8,587 passed, zero
failed, 119 ignored), and Clippy exit zero with 1,144 warnings. This is bounded
mechanism evidence; it does not close a row.

The bridge-record increment at `9b6de643`, with test-contract correction
`e92629af`, follows the native interleaved record scan and its immediate base-zone
consumer. Its [evidence report](../../research/PHASE3_CELL_ITERATION_BRIDGE_RECORDS_20260910.md)
records 83 original-executable producer cases, 86 consumer cases, production
cache/connectivity/restore checks, independent source review, and final library
results (8,590 passed, zero failed, 119 ignored). Clippy exited zero with 1,144
warnings. Derived record geometry persists under snapshot schema 140; schema 139
is rejected. This bounded repair does not close bridge lifecycle, hierarchy,
other iterator consumers or any Phase 3 row.

The archived **Phase 3 integration goal** task
`01a0529b-815e-7e31-be6d-90511e997891` reports recovery through PRs #166/#167 and
#173–#193 at `5062bcea`. That completed recovery of eligible slices, not Phase 3.
Current source has subsequently changed; use Git and the actual consumers.

[PR #172](https://github.com/YuriPlanet/vera20k/pull/172) remains an open, conflicting
mixed-history archive at `49f7707f`. Recover individual evidence and coherent
changes only. Its rejected animation shadow/layer and lowercase atlas work,
partial Railgun/AI/trigger/crate/capture work, and Explodes/Temporal research are
not preapproved implementations or automatically required Phase 3 scope.

Current bridge work and remaining transactions are described in the
[bridge plan](../2026-08-28-active-retail-bridge-parity-design.md). The plan's
status is historical; reconcile it against current source before selecting work.

## Corrections

- The new original-instruction corpus exposed reversed ClipRect operands in
  `src/map/playfield.rs`: native clips candidate Size against LocalSize. Ordinary
  intersection symmetry hides the difference; accepted signed-overflow input
  does not. The pre-fix Rust comparison failed, and the first increment repairs
  the production order. See the [comparison report](../../research/PHASE3_MAP_SPATIAL_NATIVE_COMPARISON_20260910.md).

- Old GSI-04.01 gaps G1 dummy reservation reset and G2 IsoMapPack miss stamping
  have production implementations. The old reservation-writer candidate is also
  stale: current lifecycle code implements the Mark/Clear perimeter paths.
- The old copied-dummy target-Z claim is a [documented false positive](../../gap-scans/2026-08-25-disparity-scan-gsi-04-01-dummy-cell-target-z.md).
- CellClass constructor order and anti-diagonal Fill/RNG order are different;
  never fix a hypothetical identity gap by reordering Fill.
- The old multiplayer Resize-prefix hypothesis is partly stale. Current
  `src/sim/native_identity.rs::build_noncampaign_fresh_id_prefix` accounts for
  both Cell/dummy constructor generations, and `scenario_bootstrap` tests their
  identity checkpoints. This does not establish individual Cell identity
  consumers, preview reuse or save/restore equivalence. See the current
  [prefix investigation](../../research/bridges/01-assets-map-load-overlay/FULL_INIT_AND_PREVIEW_NATIVE_ID_PREFIX_REINVESTIGATION_GHIDRA_REPORT.md).
- Generic storage iteration is not a native-order contract. Authored overlay
  recalculation already has `NativeOverlayMapShape::recalc_cells`, and other
  owners explicitly order their sweeps. Audit each active consumer before
  changing iteration shared by unrelated systems.
- TS-only exclusions require active-binary/data evidence. Neither inherited TS
  code nor a generic registry group is enough to establish applicability.

## Inherited residuals

The reverse-audit triage at `6c7ccf92` found substantial production code in every
row, but no phase-wide completion evidence. Three previously asset-gated checks
now pass on the available retail installation: automatic bridge shells, low-end
bridge TMP fields and all six theater compatibility tables (one test each, zero
failures). These validate their named data contracts, not complete traversal.

The fresh-load omission is `0x00586BF0`, called after
record construction and zone setup by `0x00684C30`. It visits inactive non-Tube
records in reverse and stamps four transverse cells at nonstructural gaps.
The [original comparison](../../research/PHASE3_BRIDGE_RECORD_GAP_RESTAMP_NATIVE_REPORT.md)
uses Deadman retail cell data: original record production emits four inactive
records, and restamping changes 80 real cells. At empty cell `(57,42)`, flags
change from zero to `0xC00`; original ore admission changes from allowed to denied,
while pre-fix Rust admits it. Independent replay reproduced the result. Targeted
dataflow evidence establishes that the intervening zone calls preserve these
records and real-cell flags; this is not full zone execution or terminal shared-
dummy equivalence. The implementation publishes the operation at the fresh-
load tail and retains real-cell `0x400/0x800` through hashing and restoration;
live shared-dummy state is retained and hashed. Twenty original restamp cases
cover vertical, preserved-bit, ordering and dummy behavior; 48 original setter
prefixes cover the direction/destruction prerequisite. Snapshot version is 141.
Independent source review and replay of all 68 original cases passed. At
`8b147e5e`, focused tests passed 35 cases with zero failures and two ignored;
the repaired Deadman check passed all 80 native cell flags and production ore
rejection. The full library passed 8,605 tests with zero failures and 120 ignored;
Clippy exited zero with 1,144 warnings. Report commit `6ec64a17` records these
receipts and the synthetic building-placement and restore coverage limits.

The [ordinary-shroud comparison](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_SHROUD_CURRENT_SIGHT_NATIVE_REPORT.md)
establishes that hostile gap concealment
preserves active unit sight. At `c7432d8c`, Rust cleared that sight and the resulting
map knowledge. Fire and psychic reveals cannot be treated as sustained sight:
original fire unshrouds without reducing the sight counter, while psychic reveal
reduces and then increases it. Effective allied sight requires source-aware writes
to direct viewers; copying another viewer's derived knowledge makes alliances
incorrectly transitive.

Departure from an existing gap schedules concealment. Original `0x00578100`
runs at the early Logic `frame % 120 == 0` gate before object and House updates.
Returning sight can cancel pending conceal when knowledge remains mapped; a
second fresh gap can change that outcome. Removing the generator after
departure does not cancel pending. Independent original execution established these observations,
including the frame-119/frame-120 boundary and the complete two-pass cell sweep.
The retained original corpus contains 23 sequences and nine signed timer cases, including temporary reveals,
departure after frame 120 waiting until frame 240, return after a second gap,
first-ever fire versus Psychic mapping at the periodic boundary, and an admitted
timed refresh preserving knowledge before concealment. Selected original SpySat
bulk stores preserve pending conceal; composed source/gap cases retain supplied
object order. These cases do not execute the full conditional callback dispatch.
Fresh review additionally proved that unchanged visibility refreshes must not
invent new native reveal events, and first-ever fire mapping has different pending
behavior from Psychic mapping even without a gap. Retained source admissions and
distinct transition writers are necessary prerequisites. Original movement callers
release stored sight geometry before admitting current geometry. The explicit
15-frame refresh for admitted moving, high-flying sources changes pending conceal
and is a required dependency, with per-viewer timer history in Rust's all-viewer
model. Passive House/cache materialization cannot substitute for that event.
The same distinction applies to SpySat: actual activation or loss brackets the
bulk map change with source and gap release/re-admission, in forward object
order. Repeated active updates do not map the cells again. Native gap removal
reads the old SpySat-active latch; the bulk change preserves pending conceal.
Independent review also requires concealed cells to lose public targetable
visibility, directional source-to-viewer admission for the new timed event, and
indexed source lookup at the project's 20,000-unit scale. Implementation
`4823a1b7` passed independent source/native review, 21 focused tests, 69 vision
tests, 17 GSI tests and deterministic replay. The pre-v142 hash and all three RNG
pins remain unchanged; the documented new composition is `C9FF66052C998226`.
At final tested commit `2038cbea`, the full library passed 8,626 tests with zero
failures and 120 ignored; Clippy exited zero with 1,145 warnings. The fresh
independent critic verified these receipts, all three retained pre-v142 hash
guards and unchanged production source, and passed final publication review.
These bounded results do not close row 50 or the phase.
Full generator admission, reveal traversal and edge-cache equivalence remain open.
Whole movement, combat, reveal-policy and drawing systems retain their later
phase owners. Their trigger phase does not exclude spatial publication required
by Phase 3. Fogged-object storage and restore tests do not establish production
insertion; its active-retail fog gate remains unproved. A fresh read of terrain
damage `0x0071B920` rejects the old surviving-corpse hypothesis: both lethal arms
destroy and uninitialize immediately, consistent with current Rust removal.

The height-consumer increment merged in PR #328 repairs the Bounce adapter's selected Cell
identity, ordered live queries, raw bridge flags and 416-lepton deck composition.
Its [evidence report](../../research/PHASE3_BOUNCE_GROUND_QUERY_DELIVERY_NATIVE_REPORT.md)
separates exact ground/surface query comparisons from bounded flat-contact
outcomes. Broader slope reflection, cliff response and quaternion physics belong
to GSI-05.14; their existence does not make them proven Phase 3 prerequisites.
The surrounding VoxelAnim host's water, damage-list and expiry queries remain
separate spatial audit work. Neither this increment nor its 48 native cases
closes row 39 or the whole VoxelAnim update.
The implementation at `af78c1bd` passed the full library suite (8,602 passed,
zero failed, 119 ignored) and Clippy (exit zero, 1,144 warnings). The fresh
read-only critic independently reproduced the final 48 native cases and
reviewed the source and production stop case with no actionable finding.

The Tube hierarchy and ordered path-entry increment merged in PR #327 is validated at source
`154b171e`; report commit `906b7e40` records final receipts. Its
[evidence report](../../research/PHASE3_TUBE_HIERARCHY_20260910.md) covers original
instructions for the shared high/Tube helper, full/local record order, raw path
tokens, live bridge-aware DWORD queries, restore preparation and 19 ordered entry
cases. Focused checks passed (11 tests, one ignored), zone-search passed 31 tests,
and zone-build passed 32 tests with one ignored. The full library passed 8,600
tests with zero failures and 119 ignored; Clippy exited zero with 1,146 warnings.
The fresh independent critic passed merging this bounded increment after all
confirmed candidate findings were fixed. This does not close M3 or any phase row.

The native entry at `0x0042C900` retains source and destination Cell pointers,
executes both zone queries, then projects those retained pointers and finally
runs conditional playfield checks. Shared-dummy writes during the intervening
queries can change what a retained pointer observes. Hoisting a pure projection
or filtering out the zone grid before those queries changes native state order.
The live entry owner now preserves this sequence even when hierarchy is disabled.

Projection `0x00583180` uses packed signed-word arithmetic, lane-projected
endpoint distances and a signed-short distance result. Its no-record helper
`0x005835D0` alternates two live Cell pointers, checks both candidate endpoints
and then reloads the source coordinate for distance selection. Missing candidates
can fall through to an unchecked record[-1] read; cyclic walks and this raw-memory
domain remain open rather than receiving invented native outcomes. Its immediate
`0x0042C290` consumer independently projects endpoints into each hierarchy level;
rectangular `zone_at` lookup is not sufficient for wrapped or signed coordinates.

The remaining movement predicates also require explicit authority. The
`0x004DA1D0` virtual checks current-or-queued Retreat and a Team predicate at
`0x006EC300`; the latter performs a mode-1 waypoint lookup and can change the
shared dummy before endpoint membership. These are not pure predicates.
Fresh retail extraction of `mapsmd03.mix/all01umd.map` (SHA-256
`dee38769f2247a85908705486c175ef14a4cf90437899defe7c8b4c0d1c51fb0`)
contains Team `0925E45C`, TaskForce `0B0815BC` (two HTNK, one TTNK), Script
`0973352C` beginning with action `3,14`, and reinforcement actions referencing
that team. Native reinforcement `0x0065D8E0` calls `0x0065DD30`, which sets
Team+0x77; Team AI then sets +0x7F before script execution. This establishes an
active YR ground-team branch, while Rust currently lacks the corresponding
Team+0x7F authority and refuses action 3. It cannot be excluded as TS-only.
The six discovered direct set-one writers of mover+0x3D4 are aircraft-only;
that narrower finding does not establish a general ground-mover flag.

Unexplained raw path-memory reads, invalid registry references, native cycles,
high-cardinality label packing and the wider movement-predicate authority remain
open. A bounded helper comparison cannot certify the complete mechanism.

The original-process [startup capture](../../../tools/spatial_oracle/tube_startup_capture.json)
now establishes floating control `0x0E7F` at the adjacent-constant initializers
`0x0049F0E0` and `0x0049F190`. Its [reproducible tool](../../../tools/spatial_oracle/tube_startup_capture.py)
uses hardware breakpoints, verifies the loaded executable section against the
pinned original, and stops its owned process before WinMain. Independent replay
passed. Original initializer emulation matches the captured constant bytes;
this resolves that startup uncertainty, while runtime writer coverage and the
remaining arbitrary raw-address domain remain open.

The bridge plan retains unresolved construction, restamp, topology and consumer
transactions. Current shroud, smudge, terrain and lighting code has not received
this goal's exhaustive reverse audit. These rows remain open regardless of the
outcome of the first map-lookup comparison.

## Coverage

No phase-wide native differential or completed reverse audit is recorded by this
goal. Existing Rust regression tests and older scoped critic passes do not imply
whole-row equivalence. The [first comparison increment](../../research/PHASE3_MAP_SPATIAL_NATIVE_COMPARISON_20260910.md)
records 2,144 original-executable calls/prefixes for lookup, playfield predicates,
normalization and retained dummy identity. Its report records the executable,
fixtures, endpoint limits, production test consumers and passing validation.
This bounded corpus does not close GSI-04.01. Ignored library tests remain
unexecuted; they are not passing parity evidence.

## Open queue

1. Finish the ordinary current-sight/hostile-gap repair, including transient-source
   distinction, effective allied sight, early periodic concealment and persistence.
   Complete native comparisons, production tests, independent review and merge.
2. Reconcile the remaining rows and retained hypotheses with current production
   and retail evidence. Prioritize observable ordinary gameplay; preserve open
   constructor, shared-dummy, iterator, connectivity and consumer-order domains.
   Hierarchy, height-query and fresh-load restamp increments already merged;
   their residuals require their own evidence.
3. Prove option-dependent fog applicability before accepting an exclusion. Stock
   `FogOfWar=no` establishes the ordinary scenario, not a universal TS-only gate.
4. Complete the phase-wide reverse audit only when every in-scope mechanism and
   evidence-backed exclusion is accounted for. Any omission keeps its row open.

## Start here

Read the current task checkpoint and verify Git/process and PR/merge state.
Verify refreshed `origin/main`
before selecting the next open mechanism. Each mechanism needs independent
review, the required library tests and Clippy, then PR publication, merge and
verification before another mechanism starts. Preserve a blocked mechanism and
continue independent work when necessary. No phase or row is closed by this brief.
