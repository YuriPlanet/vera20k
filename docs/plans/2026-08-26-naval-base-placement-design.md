# Naval Base Placement Design

**Scope:** Close the stock-active `type+0xCCE != 0` naval branch of
`HouseClass__AI_FindBasePlacement @ 0x005060B0` and its exact supporting Rules and owned-yard
state. Ordinary non-naval BasePlan/perimeter selection remains a separate open mechanism.

## Evidence and prerequisites

Primary evidence:

- [docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md) section 8;
- [docs/research/PHASE3_NAVAL_BASE_PLACEMENT_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_NAVAL_BASE_PLACEMENT_LIFECYCLE_GHIDRA_REPORT.md);
- [docs/research/PHASE3_AI_BASE_PLACEMENT_VECTOR_SELECTOR_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_AI_BASE_PLACEMENT_VECTOR_SELECTOR_005060B0_GHIDRA_REPORT.md) for the shared
  nearby-cell selector;
- completed prerequisite design and critic closure in
  `docs/plans/2026-08-26-fnpc-forward-bridge-projection-design.md`;
- completed alternate-cell prerequisite in
  `docs/plans/2026-08-26-alternate-base-cell-waypoint-actions-design.md`.

The active branch bypasses all ordinary perimeter, influence-grid, BasePlan-center, qsort,
orientation, connectivity, and duplicate-traversal logic. Its only player-visible output is the
candidate cell supplied to the already authoritative ready-building placement command.

## Exact behavior

For a ready BuildingType whose native naval byte at `+0xCCE` is nonzero:

1. Call the exact source-ordered `FirstBuildableFromArray` selector over `[General] Shipyard=`
   independently for width and height. Use the first result's foundation width plus two and the
   second result's foundation height plus two. Do not reuse generic build-option eligibility.
2. Choose `HouseState.alternate_base_center` unless its packed value is exactly `(0,0)`;
   otherwise use `HouseState.base_center`. Rust's absent `Option` representation of the native
   primary field maps to its constructor bits `(0,0)` rather than short-circuiting before the
   MapClass/FNPC call. Packed `(0,0)` is the sole invalid sentinel.
3. Invoke `find_nearby_passable_cell` with the literal native query:
   - `SpeedType::Float` (`5`);
   - required zone disabled (`-1`);
   - `MovementZone::Normal` (`0`);
   - bridge-aware zone false;
   - selected footprint width/height plus two;
   - reject-overlay false;
   - height gate false;
   - current-cell obstacle gate false;
   - structural bridge cells allowed;
   - invalid zero reference, so selection uses current binary frame modulo the chosen pool;
   - ring-side skip zero;
   - final rectangle occupancy false.
4. Preserve the exact returned candidate or fail on packed `(0,0)`. Do not search again, choose
   nearest, or pre-run final building placement.
5. If the owning House's ordered `BuildConst` vector is empty, return the candidate unchanged.
   Otherwise take only its first live stable ID, read that Building's exact
   `BuildingClass::GetCoords` coordinate,
   get the candidate CellClass center coordinate using the verified 104-lepton terrain Z at subcell
   `(128,128)`, subtract with wrapping i32 semantics, and apply
   `native_x87::distance_3d_leptons`. Reject only when distance is strictly greater than signed
   `AINavalYardAdjacency << 8`; equality passes.

The AI ready-placement caller branches on the ready object's `ObjectType.naval`. Naval objects
use only this helper; non-naval objects remain on the currently open ordinary path until that
separate mechanism is parity-closed. Failure emits no placement command and leaves the ready
object queued, matching the existing deferred placement contract.

Active retail always has MapClass, path, resolved CellClass terrain, map-size, and playfield
authority. If a headless/compatibility caller lacks any input required for the exact FNPC
projection or CellClass-center Z, naval selection returns failure; it must not use FNPC's
no-terrain compatibility projection or invent a flat CellClass surface.

## Exact shipyard selector

Scan the resolved `[General] Shipyard=` IDs in authored source order. Resolve each entry through
the category-specific BuildingType registry, not the broad cross-category name winner. For each
candidate Building type, apply only these gates, in order:

1. Resolve the House's canonical country index. Resolve every candidate `Owner=` token through
   the native source-order HouseType `Name=`-alias-then-registry-ID lookup, build the native
   32-bit mask using `index & 31`, and require the House bit. `TechnoTypeClass` construction at
   `0x00711193` initializes the Owner mask to zero, and reader block
   `0x007149E1..0x007149F5` preserves that current zero as the missing-key default. Therefore an
   absent or represented-empty `Owner=` list rejects the candidate; an explicit resolved matching
   House bit is required. Unknown list tokens contribute no bit.
2. Treat an absent/represented-empty `RequiredHouses=` list as native mask `-1` and accept it;
   otherwise resolve the same way and require the House country/type bit.
3. Treat an absent/represented-empty `ForbiddenHouses=` list as native mask `-1`/no exclusions;
   otherwise resolve the same way and reject when the House country/type bit occurs.
4. Accept `AIBasePlanningSide == -1`; otherwise require exact signed equality with
   `HouseState.side_index`.
5. When `GameOptions.super_weapons` is true, accept immediately.
6. With superweapons disabled, accept immediately when the candidate has no primary
   `SuperWeapon=`. Ignore `SuperWeapon2=`.
7. Otherwise accept when the candidate type occurs case-insensitively in source-ordered
   `[AI] BuildTech=`.
8. Otherwise resolve the primary registered SuperWeaponType and accept only when its
   `DisableableFromShell` is false. Missing registered type fails safely; active retail has none.

The active retail shipyard `Owner=` lists are explicit and contain fewer than 32 registered
countries. The current `ObjectType` vectors do not distinguish a missing key from a deliberately
empty custom key, but both forms exactly map to the native zero Owner mask and reject. This Owner
default is independent of `RequiredHouses=` and `ForbiddenHouses=`, whose separately verified
absent/represented-empty defaults remain mask `-1` as described above.

Do not test TechLevel, lower-bound TechLevel, prerequisites, factory presence, BuildLimit, cost,
credits, stolen tech, production category, AIBuildThis, or runtime superweapon instance state.
The selector consumes no RNG and has no retry/fallback list. A null result makes naval placement
fail deterministically; retail playable sides always resolve one, while native malformed custom
null dereference/crash behavior is evidence-backed out of active-retail scope.

## Rules and immutable type data

Extend `RuleSet` with the exact source-ordered lists and scalar:

- `[General] Shipyard=`;
- `[AI] BuildConst=` (`RulesClass__ReadAI @ 0x00672AE0`, binding
  `0x00672B14..0x00672C01`; there is no `[General]` fallback);
- `[AI] BuildTech=`;
- signed `[General] AINavalYardAdjacency=` with constructor default `20`.

Extend `ObjectType` with signed `AIBasePlanningSide=`, default `-1`. After all object registries
are built, resolve the `BuildConst` list case-insensitively through the BuildingType registry and
stamp an immutable
`build_const_eligible` flag on matching Building types. Copy that flag to each constructed
`GameEntity`, just as the lifecycle authority already copies Rules-derived cell/scoring facts.
The immutable flag is serialized and state-hashed with the entity because rule-less lifecycle
calls must not re-resolve a RuleSet and the flag can change a later ordered-vector mutation.

The lists remain immutable match input and do not join mutable state hashing. The scalar and type
fields are likewise Rules input. Mutable vector membership below is future-affecting state.

## Ordered owned-`BuildConst` state

Add `HouseState.build_const_order: Vec<u64>`, default empty. It is authoritative mutable state,
serialized and folded into `state_hash` with a length delimiter and IDs in stored order.

Update only the existing lifecycle chokepoints:

- after a structure successfully completes Reveal/Mark and is alive, append its stable ID when
  `build_const_eligible`, guarding against duplicate append;
- on the successful non-already-limbo Conceal/Limbo path, stable-remove the ID before the object
  becomes unavailable; a failed/no-op conceal does not mutate it;
- in `change_owner_impl`, when the entity is BuildConst-eligible, stable-remove it from the old
  House before `EntityStore::change_owner`, then append it to the new House after the swap. Do
  this regardless of the optional RuleSet argument because eligibility is already immutable.

The vector must not be reconstructed from `EntityStore` stable-ID order, owner indexes, or the
global Logic vector. Capture and re-entry change native append order. Stale IDs are invariant
violations; the naval read may fail safely rather than silently scan for another yard, because
native reads exactly the first pointer.

Adding the House field and entity flag changes serialized layout. Bump `SNAPSHOT_VERSION` once
for this slice, update its named version test, and prove round-trip/hash order. Extend the existing
`state_hash_with_schema` compatibility switch with a current-v109 gate so the historical pre-v28
and pre-v29 provenance probes omit both new folds; current `state_hash` includes both. No other
state versioning is needed.

## Coordinate and arithmetic contract

- Candidate X/Y: `cell * 256 + 128`, with native signed-cell/wrapped-i32 semantics at the kernel
  boundary.
- Candidate Z: the common `ground_height_leptons` authority at center subcell, the active-runtime
  verified 104-lepton CellClass terrain surface. See
  [PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md).
- Yard X/Y: start from the live entity's stored north-west anchor coordinate and apply
  `BuildingClass::GetCoords` foundation centering, `(width - 1) * 128` and
  `(height - 1) * 128`, with wrapped i32 arithmetic. Yard Z preserves the live object's exact
  coordinate/ground-domain behavior. Reuse or factor the already verified coordinate derivation
  from the naval factory-exit, radar, animation-owner, lifecycle, or combat consumers; do not use
  the stored north-west anchor as the distance point.
- Delta: candidate minus yard, component-wise wrapped i32.
- Distance: `native_x87::distance_3d_leptons`.
- Cap: signed `AINavalYardAdjacency.wrapping_shl(8)` equivalent; accept `distance <= cap`.

## Player-experience ledger

- **Milestone-blocking, every naval AI building:** selecting Float/Normal passability and the
  source-side shipyard footprint determines whether a water site exists.
- **Compounding, every candidate pool with more than one entry:** zero reference selects by
  current-frame modulo; nearest-origin selection would change deterministic AI layouts.
- **Milestone-blocking near bridges:** the already closed forward-side/structural projection runs
  both during collection and final partition for this exact query.
- **Frequent after expansion or capture:** only the first acquisition-ordered BuildConst yard owns
  the cap; nearest-yard or stable-ID order changes whether remote naval expansion is permitted.
- **Terrain-sensitive:** exact 3D CellClass-center-to-object distance can differ from a 2D cell
  radius on elevation and at subcell/object Z boundaries.
- **Option-sensitive custom mechanism:** with shell superweapons disabled, only the primary SW,
  BuildTech exemption, and DisableableFromShell tail participate. Generic build gates would reject
  valid candidates.

## Explicit exclusions

- Ordinary non-naval BasePlan population, perimeter selection, influence grids, connectivity,
  production chooser scheduling, wall expansion, projected power insertion, retry/filled-node
  lifecycle, and the later `AI_ScanBasePerimeter` are not called by this branch and remain open.
- The stock-inactive CloakGenerator legacy branch is not part of naval selection.
- `ConstructionYard=yes` is not `BuildConst` membership and cannot substitute for it.
- Final `PlaceReadyBuilding` admission remains the shared placement authority. The selector's
  native FNPC occupancy flag is false; do not add early final-footprint occupancy rejection.
- Do not emulate native invalid-memory behavior for malformed custom Rules with no selectable
  shipyard. Deterministic failure is the evidence-backed safe boundary; active retail cannot
  reach it.

## Acceptance tests

1. Rules parsing proves source order, case-insensitive object resolution, default/negative
   `AINavalYardAdjacency`, default/override `AIBasePlanningSide`, and retail Allied/Soviet/Yuri
   shipyards all yield independent `6x6` width/height results.
2. Selector truth-table tests prove Owner, RequiredHouses, ForbiddenHouses, exact side, enabled
   superweapons, absent primary SW, BuildTech exemption, non-disableable primary SW, rejection of
   disableable primary SW, ignored `SuperWeapon2`, and deliberate absence of every generic build
   gate. Earlier failing entries must fall through in source order; no candidate returns failure.
3. Naval query transcript proves Float, zone `-1`, Normal, bridge-aware false, all literal false
   gates, bridge allow true, zero reference, zero skip, zero final occupancy, and current-frame
   modulo output. Recheck the completed bridge projection fixtures through this caller.
4. Origin tests prove nonzero alternate overrides primary, packed zero falls back to primary, and
   a failed search emits no command without dequeuing the ready item.
5. BuildConst lifecycle tests prove successful Reveal append, failed Reveal no append, duplicate
   Reveal no append, successful Conceal stable removal, already-limbo no-op, old-owner removal and
   new-owner tail append on capture, and re-entry tail append. A stable-ID/owner-order counterexample
   must leave a different first yard and show the stored order wins.
6. Distance tests prove no-yard bypass; first-yard-only behavior; exact CellClass 104-Z and object
   `BuildingClass::GetCoords` foundation centering and object Z; equality acceptance;
   strict-greater rejection; signed/negative scalar behavior; and an elevation case differing
   from a 2D/cell-radius shortcut.
7. AI integration proves only ready `Naval=yes` Buildings use the helper and non-naval placement is
   unchanged. The emitted cell must still pass through the existing authoritative placement command.
8. Snapshot round trip preserves both House vector order and entity membership; swapping vector
   order changes `state_hash`; the named current snapshot-version assertion reflects the one bump.

Focused validation uses scoped `cargo test -p vera20k --lib <filter>` commands for RuleSet/object
parsing, naval AI selection, lifecycle ownership, world hash, and snapshots. The phase-wide full
`cargo test -p vera20k --lib` remains reserved for final Phase 3 certification.
