# FNPC Forward-Side Bridge Projection Design

## Goal

Make `Find_Nearby_Passable_Cell` classify bridge-adjacent candidates with the exact active
`gamemd.exe` forward-side/structural-probe height correction while preserving every other
ring, gate, ordering, and selection behavior.

## Architecture Context

`src/sim/find_nearby_cell.rs` owns deterministic FNPC search. Candidate validation feeds
`collect_candidates`. Ordinary collection invokes the projection per accepted coordinate for
per-ring early-stop; bridge-aware-zone collection skips it entirely and lets any accepted
candidate arm early-stop. Final selection independently invokes the projection for every stored
candidate in order. At inspected base revision `eeb2515e`, Rust reused a cached classification;
the first builder revision `2cef3ea2` restored the second call site but still ran an approximate
six-cell projection and incorrectly performed projection during bridge-aware collection.

`ResolvedTerrainCell.bridge_facts.raw_flags` already owns the modeled live
`CellClass+0x140` bits. `ResolvedTerrainGrid` owns the real-cell/shared-dummy lookup seam;
this design adds one narrow projection view that returns signed `CellClass+0x11B` and raw
`CellClass+0x140 & 0x1180` from the same lookup. `src/map/bridge_facts.rs` names `0x1000` as
`BRIDGE_FLAG_FORWARD_SIDE` and `0x100` as `BRIDGE_FLAG_STRUCTURAL`, and active
`SetBridgeDirection` stamping preserves both. No new state or writer is needed.

Native `FUN_006D6410 @ 0x006D6410` performs two separate candidate lookups (first flags,
then signed level), starts at candidate centre plus `0x600` leptons on both axes, subtracts
eight leptons before every probe lookup, and therefore reads repeated cells far-to-near. Each
probe is looked up once, and its signed level and raw flags come from that same returned
CellClass. Only when candidate `0x1000` is set does probe `0x100` add four to the signed height
delta. The helper returns either the current probe cell after the projected-axis comparison or
the candidate after the later equality check. [Ghidra `0x006D641B..0x006D6588`;
[docs/research/PHASE3_AI_BASE_PLACEMENT_VECTOR_SELECTOR_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_AI_BASE_PLACEMENT_VECTOR_SELECTOR_005060B0_GHIDRA_REPORT.md)
§2.2, §9 Handoff B; [docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md)
§8.2, §11.1 item 13]

## Impact Analysis

- Change `src/sim/find_nearby_cell.rs` plus the narrow CellClass lookup seam in
  `src/map/resolved_terrain.rs`; do not change bridge facts, `PathGrid`, snapshots, hashing,
  public callers, or persistent state.
- Replace the threshold shortcut with a pure instruction-faithful projection kernel. Feed it
  one lookup closure so transcript/order tests and the runtime real-or-dummy view exercise the
  same arithmetic.
- Blast radius is every FNPC caller because the helper is shared, but observable change is
  gated by the exact candidate-`0x1000` plus projected-probe-`0x100` combination. Nonbridge
  terrain and structural probes without candidate forward-side remain bit-for-bit unchanged.
- The search consumes no RNG. Its CellClass lookups intentionally retain native shared-dummy
  coordinate side effects: every missed lookup stamps the packed requested coordinate once and
  returns the dummy's live signed level and flags. Candidate order, frame-modulo indexing, and
  same-tick determinism remain owned by existing code.

## Chosen Approach

Implement native lepton-to-cell truncation, wrapped 32-bit arithmetic, two candidate lookups,
and the subtract-eight projection loop exactly. Cache only the candidate's forward-side bit.
For each probe, consume one CellClass view; compute the signed level delta, conditionally add
`BRIDGE_LEVEL_RISE` (`4`) from that same view, perform the two projected-axis comparisons, then
the current-cell equality check in native order. The candidate is direct exactly when the
returned packed cell equals the candidate.

Ordinary collection invokes this classifier per accepted candidate. Bridge-aware collection
does not invoke it at all; accepted candidates retain only a diagnostic `direct = false` value
and arm early-stop. Final partition invokes the classifier independently for every stored
candidate in order and never reads cached `Candidate.direct`.

This is the smallest Rust-native translation: it changes the classifier at the exact point
native changes its height delta, uses the existing CellClass flag authority, and restores the
native two-site projection call pattern without changing candidate or selection ordering.

## Player-Experience Detail Ledger

- **MILESTONE-BLOCKING — exact two-bit gate:** probe `0x100` is ignored unless candidate
  `0x1000` is set; otherwise intact bridges can change direct rings and selected cells.
  [Ghidra `0x006D6473..0x006D6516`]
- **MILESTONE-BLOCKING — exact magnitude/order:** add four signed height levels to the probe
  delta before the existing isometric projection comparison, not to the candidate/base height
  and not after classification. [Ghidra `0x006D64F8..0x006D651D`]
- **COMPOUNDING — both semantic uses:** the classifier runs during ordinary collection
  early-stop and again during final direct/indirect pool partition, which can change pool length
  and `frame % pool_len` output. [selector report §2.2; exhaustive report §8.2]
- **COMPOUNDING — preserve bridge-aware asymmetry:** `bridge_aware_zone` collection performs
  zero projection/dummy lookups and any accepted candidate arms early-stop, while final selection
  still projects every stored candidate. [Ghidra FNPC collection branch and final partition]
- **COMPOUNDING — flag authority:** use modeled live CellClass flags through the existing
  resolved-terrain/shared-dummy seam; do not infer forward-side from path passability or add a
  writer. [current `bridge_facts.rs`, `resolved_terrain.rs`; exhaustive report §8.2]
- **MILESTONE-BLOCKING — determinism:** retain ring order, duplicate radius-zero seed,
  24-candidate cap, frame-modulo selection, and zero Scenario RNG. [current FNPC tests;
  selector report §2.2]
- **No exactification residual:** all behavior in this narrow correction is verified and its
  required Rust inputs already exist. Naval selection and ordinary base placement remain
  separate mechanisms, not approximations inside this slice.

## Design

### Components

- `ResolvedTerrainGrid::cellclass_projection_view`: one stamping MapClass lookup returning
  signed level and modeled raw flags together. A miss stamps exactly once and snapshots the live
  shared dummy after that stamp.
- `project_candidate_with_lookup`: exact pure `FUN_006D6410` kernel, including packed shorts,
  wrapped lepton arithmetic, duplicate candidate reads, repeated far-to-near probes, two-bit
  bridge correction, comparison order, and returned cell.
- Collection/final orchestration: inject one classifier internally so tests can prove invocation
  count/order. Bridge-aware collection skips it; final partition always invokes it once per stored
  candidate and ignores `Candidate.direct`.
- Test fixtures in the same leaf module: stamp raw flags directly on resolved terrain so each
  load-bearing branch is executable without adding production-only scaffolding.

### Interfaces / Contracts

No public API changes. The active runtime supplies resolved terrain and therefore uses the exact
real-or-dummy projection view. The existing no-terrain compatibility path retains its level
facade and zero flags; it has no CellClass authority to mutate.

### Data Flow

`MapClass-like real/dummy view -> exact projection kernel -> ordinary collection early-stop;
stored candidate order -> fresh exact projection -> preferred pool -> existing target/frame
choice`.

### Error Handling

No new failure state. Off-grid/unallocated projection reads stamp and expose the one live shared
dummy. Candidate/probe packed-short conversion and signed levels follow native instructions.

### Testing Strategy

Focused executable tests will prove:

1. the pure kernel performs two candidate reads followed by the exact far-to-near repeated probe
   transcript/count (179 lookups for flat candidate `(5,5)`);
2. a sparse allocated edge consumes live shared-dummy signed level/flags and leaves the dummy at
   the final requested coordinate;
3. ordinary collection and final partition invoke projection separately and in order;
4. bridge-aware collection performs no projection, while final partition still projects;
5. candidate-forward plus probe-structural adds exactly four, the probe bit is ignored without
   candidate-forward, and existing nonbridge results are unchanged.

## Architectural Decisions

- Keep one classifier implementation, but call it at native's collection and final-partition
  sites; do not duplicate its logic or reuse stale classification across those sites.
- Read CellClass flags from resolved terrain, not `PathGrid`: forward-side is a raw CellClass
  flag, while path passability exposes no equivalent authority.
- Add no persistent state, scheduling, RNG, or cross-layer dependency.
- Tech debt introduced: none.

## Alternatives Considered

1. **Chosen: exact lepton kernel plus one real-or-dummy CellClass view, invoked at both native
   sites.** Preserves native arithmetic, lookup order, dummy side effects, and one implementation.
2. **Fold the correction into generic `cell_level`.** Rejected because it would affect FNPC's
   separate seed-height gate and every caller, while native gates it only inside projection.
3. **Keep the current cached final classification.** Rejected: native re-runs the helper, and the
   authoritative CellClass flag accessor retains shared-dummy lookup side effects, so equivalence
   is no longer proven even though ordinary in-grid classification values are identical.
4. **Keep the six-cell threshold shortcut.** Rejected after critic review: it cannot reproduce
   repeated far-to-near misses, live dummy level/flags, or the exact return point.

## Autonomous Approval Record

**Review verdict after first critic: APPROVE REVISION.** The first builder revision fixed cached
final classification but the fresh critic found one P1: its six unique near-to-far probes skipped
native's repeated far-to-near lookups, forced misses to zero, skipped probe lookups when candidate
`0x1000` was clear, and still projected during bridge-aware collection. Cold assembly at
`0x006D6410..0x006D6588` confirms the finding. This revision resolves it with the exact kernel,
one real-or-dummy view, explicit collection skip, and mutation-resistant lookup transcripts.

Why approve: every behavior claim is grounded in the two cited exhaustive reports, the critic's
exact finding, current Rust at `2cef3ea2`, and a cold decompile/assembly read of `0x006D6410`; all
blocking details now have executable order/count/side-effect tests, with no new state, RNG,
scheduler, or cross-layer dependency.

What could still make ordinary skirmish wrong: changing ring order, the bridge-aware early-stop
asymmetry, direct-pool preference, or frame-modulo ordering. The design explicitly leaves those
owners unchanged and reruns the full existing FNPC module tests.

What could create expensive later rework: inventing a second bridge-flag authority or folding
the correction into generic height lookup. Both are rejected; the design consumes the existing
CellClass raw-flag seam at the native projection boundary. No open question or residual remains
for this narrow mechanism. Autonomous approval is recorded under the user's explicit authority.
