# Phase 3 ordered BasePlan state and lifecycle design

**Date:** 2026-08-26

**Phase/GSI ownership hypothesis:** Phase 3, GSI-04.05

**Native evidence:**

- [docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md), especially sections 7.6.1, 7.6.2, 7.6.8, 7.6.9, 7.6.10, 9, 11.1 item 10, and 11.2.
- Active-retail `BaseClass`/House/Building routines cited there: `0x0042E6F0`, `0x0042EBE0`, `0x0042ED60`, `0x0042F180`, `0x0042F260`, `0x0042F380`, `0x00440580`, `0x00443C60`, and `0x0050A490`.

## Verdict

Rust has no native-equivalent ordered House BasePlan. The ready-building queue
cannot own this state: native scenario loading, AI planning, Building exit,
successful Unlimbo, placement failure, and Building Limbo all observe or mutate
the same ordered 16-byte node vector. The next bounded mechanism establishes
that authoritative state and the independently callable lifecycle writers. It
does not claim that ordinary production planning or site selection is connected
yet; those remain explicit open mechanisms under GSI-04.05.

## Player-experience ledger

| Native behavior | Rust result required by this mechanism |
|---|---|
| Campaign BasePlan nodes load in numbered order | Map house parsing preserves `PercentBuilt`, signed type/control, and signed-narrowed packed cells in source-number order |
| Planned sites and filled/retry state survive save/load | House state owns the ordered nodes; current snapshots and the current state hash include every semantic field |
| A successfully placed AI Building satisfies its plan node | Successful Building Unlimbo marks the exact type/cell node first, or the first unfilled same-type node only for an `UndeploysInto` type, and resets retries |
| Removing a Building invalidates related cached sites | Building Limbo clears other nodes sharing the cell and applies the exact nonzero-mode `IsBaseDefense` replacement conversion |
| Repeated normalized placement failure eventually abandons one node in skirmish | Retry increments with signed wrapping arithmetic, campaign never evicts, and nonzero modes remove only after strict `new_count > MaximumBuildingPlacementFailures` |
| A failed ordinary coordinate is not retried forever | The state authority can clear every node whose packed site equals the failed coordinate without changing its other fields |
| Native zero/empty/invalid cells are one bit pattern | Every state boundary uses packed `(0,0)`; no semantic `Option` variants are serialized or hashed |

## Authoritative representation

Add a small `sim::base_plan` module and make `HouseState` own one
`BasePlanState`:

```text
BasePlanState {
    percent_built: i32,
    nodes: Vec<BasePlanNode>,
}

BasePlanNode {
    type_or_control: i32,
    packed_cell: u32,
    filled: bool,
    retry_count: i32,
}
```

`type_or_control >= 0` is the native BuildingType registry index, not an
interned-name surrogate. Negative planner controls remain literal signed values.
Packing uses signed 16-bit X in the low word and signed 16-bit Y in the high
word. `(0,0)` is the only stored empty/invalid-site value.

The native scenario writer and `BaseClass::CalculateChecksum` fold only node
count, type/control, X, and Y. Provide a focused native-checksum helper with that
scope if useful for verification. This does **not** reduce Rust's authoritative
world hash: current-schema hashing must include `PercentBuilt`, order, every
type/control, packed cell, filled latch, and retry counter because all affect
future deterministic behavior.

## Scenario population

Extend the ordered map-house representation rather than reparsing raw INI in
sim. For every named house section:

1. Read signed `PercentBuilt=` with the representation's current/default value
   and signed `NodeCount=` with default zero.
2. Visit `000` through `NodeCount-1` in numeric order.
3. When the first value byte is `-`, parse the first comma token with signed
   `atoi` semantics as the control value. Otherwise resolve the first token
   case-insensitively to its BuildingType registry index.
4. Parse X and Y with signed `atoi`, narrow each to signed 16 bits, and pack the
   two words literally.
5. Append the node with deterministic `filled=false`, `retry_count=0`. Rust
   allocation failure is fatal rather than a recoverable branch; no speculative
   rollback or alternate semantics are added.

Install the parsed plan when `initialize_map_roster_houses` constructs each
named scenario House, before map Buildings unlimbo. Generated skirmish Houses
start with an empty vector and zero percent; their native `0x005054B0`
population belongs to a later mechanism.

## Lifecycle writers

Fresh Buildings need immutable, snapshot-safe type facts at the rules-to-entity
boundary: exact BuildingType registry index, `IsBaseDefense`, and whether
`UndeploysInto` is non-null. Do not infer any of them from names or categories.

After successful Building Mark/Unlimbo for a non-human owner:

1. Scan nodes from zero for the first exact type index **and** packed-cell match.
   The filled latch does not gate this exact search.
2. Only if none matched and the placed type has non-null `UndeploysInto`, scan
   again for the first same-type node with `filled == false`; ignore its cell.
3. Set the selected node's filled latch to true and retry count to zero. Change
   nothing when neither search succeeds.

At the Building Limbo seam, before the common Techno Limbo loses the committed
type/cell facts and only outside map-editor mode:

1. Find the first exact type-index/cell node. If none exists, change nothing.
2. Clear the packed cell of every *other* node sharing that cell, leaving those
   nodes' type/control, filled, and retry fields unchanged.
3. Leave the matched node unchanged for `IsBaseDefense=no` or campaign mode.
4. For `IsBaseDefense=yes` in a nonzero mode, write type/control `-1` and packed
   `(0,0)` to the matched node, retaining its filled/retry fields.

Expose exact state mutations for the later Building-exit integration:

- clear all sites equal to one failed packed coordinate, without touching other
  fields;
- on normalized final result `1`, increment a referenced node's retry counter
  first with signed wrapping arithmetic; campaign retains it; a nonzero mode
  stable-removes the complete node only when the new count is strictly greater
  than the signed rule value; a missing node has no effect.

Parse signed `[General] MaximumBuildingPlacementFailures=` with constructor
default `5`, preserving negative mod values. The active retail override is `3`.

## Integration and ordering constraints

- Preserve the current ready queue unchanged; BasePlan is separate authority.
- Scenario nodes must exist before map-object Unlimbo so map Buildings can fill
  them at the native lifecycle boundary.
- Put the Limbo writer before common concealment/pointer-expiry can erase or
  transfer the needed owner/type/cell facts.
- The lifecycle functions must use vector order and stable `Vec::remove` shifts.
- Bump the snapshot schema once. Current-schema hashing includes the new House
  and immutable entity fields; historical hash layouts remain gated exactly as
  existing versioned fields are.
- Do not run the full `cargo test -p vera20k --lib` suite in this mechanism.

## Evidence-backed exclusions and open mechanisms

The following are **not** silently approximated here and keep GSI-04.05 open:

- fresh-skirmish `AI_RecalcBuildOptions` BasePlan generation;
- runtime refinery/weapons insertion, wall expansion, projected-power splice,
  ComputerTakeover population, and planned current/next-node site writes;
- wildcard/explicit satisfaction lookup and the filled-node recycling policy;
- BasePlan-center recentering and AI ConstructionYard-deploy center writes;
- the strategy timer, AI-hate/Super/emergency chain, eight-frame chooser, and
  economy mode transitions that schedule native planning;
- influence-grid construction and defense type/category/quadrant selection;
- ordinary `0x005060B0` site selection, cached-site connectivity/reselection,
  and downstream placement-result classification;
- wall/base-perimeter execution at `0x005082C0`, upgrade placement, the two
  Team convoy removals, and runtime MapClass resize adjustment.

The last two excluded native count-store families are evidence-backed separate
callers per report section 7.6.10; they are not guessed inactive.

## Acceptance

Focused tests must prove at least:

- scenario `PercentBuilt` and numbered nodes preserve numeric order;
- negative control parsing and signed-i16 cell narrowing are literal;
- scenario nodes normalize native undefined filled/retry bytes to false/zero;
- native BaseClass checksum excludes filled/retry while Rust state hash includes
  them and node order;
- current snapshot round-trips all fields and rejects the preceding schema;
- successful non-human Building Unlimbo exact-match priority, undeploy fallback,
  filled-node skip, cell ignore, and retry reset;
- human Building Unlimbo does not mutate BasePlan;
- Limbo clears all other same-cell sites and applies the exact campaign,
  non-defense, and skirmish-defense tails;
- failure-site clearing preserves type/filled/retry;
- retry post-increment, equality retention, fourth-failure retail eviction,
  campaign no-eviction, negative-maximum first-failure eviction, and stable
  ordered removal.

The builder must run only focused `--lib` filters, report literal output, commit
the coherent slice, and leave Cargo idle. A fresh read-only critic must receive
this design, the native report, the complete diff, and all validation output.
