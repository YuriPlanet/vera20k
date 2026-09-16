# Bridge Locomotor Layer Correctness Design

## Goal

Replace the reactive Z-based bridge-layer heuristic with gamemd.exe's exact cell-flag predicate, and tighten A*'s bridge-entry gates, so that drive-locomotor units transition between ground and bridge layers at the same cell boundaries — and on the same exact conditions — as the original engine.

## Scope

Closes four gaps from [docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md](../gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md):

- **G2** — Drive locomotor uses reactive height heuristic, ignores pathfinder layer hints.
- **G3** — A* doesn't gate bridge entry on the bridgehead flag.
- **G4** — A* accepts height-diff 2 and 3 between adjacent ground cells.
- **G6** — Layer-aware `Can_Enter_Cell` switches occupancy lists mid-check; Rust pre-decides.

Per the gap-scan §286-289, G3/G4 share fix locus with G2/G6. Bundling them avoids cross-cutting fixes during implementation.

## Architecture Context

### How layer decisions flow today (Rust)

```
A* (core.rs::astar_search)
   produces  Vec<LayeredPathStep> { rx, ry, layer }
                      │
                      ▼
MovementTarget { path, path_layers } in components.rs
                      │
                      ▼
movement_step::process_step_for_entity
   reads next_layer = target.layer_at(target.next_index)   ← used for walkability + cliff
                      │
                      ▼
occupancy.move_entity (position now in dst cell)
                      │
                      ▼
resolve_cell_transition_bridge_state (movement_bridge.rs:43)
   ┌──── IGNORES _next_layer ────┐
   │ uses abs(unit.z - cell.z)≥2 │   ← THE G2 BUG
   └──────────────────────────────┘
                      │
                      ▼
apply_pending_bridge_render_state
   updates loco.layer, on_bridge, BridgeOccupancy
```

### How layer decisions flow in gamemd.exe

```
PathfinderClass::AStar_main_loop (0x429A90)
   uses dual closed lists, layer per step depends on path_height vs cell_height
                      │
                      ▼
FootClass moves through path
                      │
                      ▼
DriveLocomotionClass::Process_Drive_Track (0x4B0F20)
   at each cell boundary, reads src and dst CellClass flags:
     entry  if dst.+0x11B == src.+0x11B - 4 AND dst.flags & 0x100  → on_bridge=1
     exit   if !(dst.flags & 0x100) AND src.flags & 0x100          → on_bridge=0
   (two INDEPENDENT conditions, runtime cell-flag predicate)
                      │
                      ▼
FootClass+0x79 (analogous to our loco.layer) updated; bridge-aware zone lookups follow
```

### Why our heuristic is wrong

- `abs(unit.z - cell.ground_level) >= 2` depends on the unit's incrementally-updated Z, which lags or leads the actual cell-flag-defined transition by one tick.
- The lag/lead causes the `target_layer` parameter to `cell_entry::check_terrain` to read the wrong occupancy list on the boundary tick — that's G6.
- The `apply_bridge_lookahead_if_needed` function exists to mask this lag by pre-claiming bridge occupancy before the transition. It's a band-aid for the wrong primary mechanism.

### Why A* needs G3/G4 in addition

- `compute_neighbor_height` Case 3 ([core.rs:144-152](../../src/sim/pathfinding/core.rs#L144-L152)) accepts any `bridge_walkable` cell at height-diff [2,4] — but gamemd's `CheckBridgeTraversal` only accepts diff exactly 4 for bridge entry, and requires the bridgehead flag (0x200, `transition` in our `PathCell`).
- A* returns paths that step onto bridge body cells from any side; runtime then blocks them via the layer-walkable gate at [movement_step.rs:506-510](../../src/sim/movement/movement_step.rs#L506-L510); unit repaths every approach. That's G3.
- Diff 2 and 3 are ALWAYS blocked in gamemd; Rust accepts them in A*. That's G4.

## Impact Analysis

**Files modified:**
- [src/sim/movement/movement_bridge.rs](../../src/sim/movement/movement_bridge.rs) — replace heuristic body of `resolve_cell_transition_bridge_state`; delete `apply_bridge_lookahead_if_needed`; introduce `compute_bridge_transition` helper.
- [src/sim/movement/movement_step.rs](../../src/sim/movement/movement_step.rs) — plumb src cell coords (`old_rx, old_ry`) to the resolver; remove `apply_bridge_lookahead_if_needed` call site.
- [src/sim/pathfinding/core.rs](../../src/sim/pathfinding/core.rs) — `compute_neighbor_height` Case 3: tighten to `diff == 4` AND require `transition` flag; `astar_search` neighbor walkability (line 440): gate Ground→Bridge crossings on `transition`.
- [src/sim/pathfinding/cell_entry.rs](../../src/sim/pathfinding/cell_entry.rs) — update TODO(RE) comment; no logic change (`target_layer` parameter is now correct once G2 is fixed).

**What depends on what we're changing:**
- Every drive-locomotor entity tick crossing a bridge cell boundary.
- A* result determinism: replays from before the fix will diverge on bridge maps. Acceptable (no published replays).
- `BridgeOccupancy`, `on_bridge`, and `loco.layer` are already in the deterministic state hash — no new hashed state.

**Risk areas:**
- Bridge maps' existing test coverage — the truncated layered-path fallback in [movement_path.rs:277](../../src/sim/movement/movement_path.rs#L277) may produce shorter `path_layers` for the same path; verify tests still pass.
- Bump/scatter onto body cell from the side: cell-flag predicate handles it (independent conditions, not path-dependent).
- Diagonal moves between two body cells: src and dst both have 0x100; predicate's exit condition fires `!(dst.0x100) AND src.0x100` → false; entry condition fires `dst_h == src_h - 4 AND dst.0x100` → both heights are 0, no entry. So NoChange — unit stays on bridge. Correct.

**Determinism / sim/ checklist:**
- All math uses `i8` signed arithmetic (one place: the height comparison). No floats.
- No new state added to hash.
- No render/ui/audio/net dependencies introduced.
- Tick ordering: predicate runs at the SAME point in the tick as the current heuristic (after `move_entity`, before next-tick layer query). No ordering shift.
- `BTreeMap` iteration not affected.

## Chosen Approach

**Approach A** (selected over B and C — see Alternatives Considered).

Add a pure helper `compute_bridge_transition(src: &PathCell, dst: &PathCell) -> BridgeTransition` that returns `Enter { deck_level }`, `Exit`, or `NoChange`. Mirrors gamemd's two independent conditions exactly. The current resolver function calls it and applies the result to `position.z`, the returned layer, and the bridge state update.

The anticipatory `apply_bridge_lookahead_if_needed` function is **deleted** — gamemd has no anticipatory layer change, and the lookahead exists only as a workaround for the broken primary mechanism.

A* tightening is local: one comparison change (`(2..=4)` → `== 4`) and one flag check (require `transition` for Ground→Bridge crossings) in `core.rs`. No structural changes.

## Tiny-Detail Ledger

Every detail below must survive in the implementation. Each cites its source.

| # | Detail | Source |
|---|--------|--------|
| 1 | Entry predicate: `dst.height_level == src.height_level - 4 AND dst.flags & 0x100` → on_bridge = 1 | [GHIDRA 0x4B1812] / BRIDGE_SYSTEM.md §"Bridge Ramp Detection" |
| 2 | Exit predicate: `!(dst.flags & 0x100) AND src.flags & 0x100` → on_bridge = 0. **Independent of entry**, not else-branch | AUDIT_LOG entry 2026-05-11 (scoped audit) |
| 3 | Height read at +0x11B is **signed i8** via MOVSX. Rust must use `i8` arithmetic; values >= 128 (rare/malformed maps) interpret as negative | AUDIT_LOG entry 2026-05-11 |
| 4 | Predicate runs at cell-boundary crossing (not mid-cell, not anticipatory) | GHIDRA 0x4B0F20 control flow |
| 5 | Bridge deck = **exactly 4 height-levels** above ground in world Z (`g_BridgeZOffset_Drive = ftol(g_DriveHeightStep * 4)`), independent of +0x11B byte values | [GHIDRA 0x4AF4A0] |
| 6 | Set_Destination Z bump fires on **destination cell's** 0x100 flag — already correct in our Set_Destination analog | [GHIDRA 0x4AFDDD] |
| 7 | Bridgehead flag (0x200) is **REQUIRED** for Ground→Bridge entry. Body cells (only 0x100, no 0x200) cannot be entered from ground level | BRIDGE_SYSTEM.md §"CheckBridgeTraversal" |
| 8 | Height diffs: 0 (passable), 1 (per SlopeIndex), **4 only** (bridge ramp). 2/3/5+ **always blocked** | BRIDGE_SYSTEM.md §"CheckBridgeTraversal" + GHIDRA 0x4D9C60 |
| 9 | Bit 0x80 (= `bridge_walkable` flag analog in PathCell) marks body cells; set during map initialization (upstream of SetBridgeDirection — specific load path not traced), NOT by SetBridgeDirection's direction-parity logic at runtime | AUDIT_LOG entries 2026-05-11 |
| 10 | A* dual closed lists: a cell can be visited at BOTH ground and bridge levels with different parents/costs. Layer determined by `is_at_bridge_level(current.height, neighbor)` | BRIDGE_SYSTEM.md §"A* Pathfinding — Dual-Layer Bridge Support" |
| 11 | gamemd's `Can_Enter_Cell` is two-pass with mid-check layer switch. Our pre-decided `target_layer` matches this output IF the layer pre-decision is correct (fixed by G2) | [UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md) §"Phase 6" |
| 12 | `SetBridgeDirection_NESW` (0x47E040) and `SetBridgeDirection_NWSE` (0x47E470) are byte-identical — irrelevant to this design; no Rust SetBridgeDirection port exists | AUDIT_LOG entry 2026-05-11 |
| 13 | Predicate's two conditions are mutually exclusive in retail data (both require contradictory dst.flag state) but the implementation MUST evaluate them independently — the audit caught a doc pseudocode bug where they were structured as if/else | AUDIT_LOG entry 2026-05-11 |
| 14 | Diagonal body-to-body transition (both have 0x100): entry doesn't fire (heights equal, not `src - 4`); exit doesn't fire (dst has 0x100). `NoChange` — unit stays on bridge | derived from #1 + #2 |
| 15 | When path data is unavailable (rare — spawn, off-map paths, init), the resolver returns `(fallback_layer, Unchanged)` without applying any predicate. `fallback_layer` is the caller's `active_layer` | error-handling design (see Error Handling section) |

## Design

### Components

**`BridgeTransition` enum** in `movement_bridge.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeTransition {
    Enter { deck_level: u8 },
    Exit,
    NoChange,
}
```

**`compute_bridge_transition` helper** in `movement_bridge.rs`:

```rust
pub(super) fn compute_bridge_transition(src: &PathCell, dst: &PathCell) -> BridgeTransition {
    let src_h = src.ground_level as i8;
    let dst_h = dst.ground_level as i8;

    let entry = dst_h == src_h - 4 && dst.bridge_walkable;
    let exit = !dst.bridge_walkable && src.bridge_walkable;

    if entry {
        return BridgeTransition::Enter {
            deck_level: dst.bridge_deck_level_if_any().unwrap_or(dst.ground_level),
        };
    }
    if exit {
        return BridgeTransition::Exit;
    }
    BridgeTransition::NoChange
}
```

The two conditions evaluate independently (ledger #13). Entry takes precedence in the return order; in retail data the two are mutually exclusive so order doesn't affect output, but the explicit structure guards against future regression.

**`BridgeStateUpdate` enum** replacing the awkward `Option<Option<u8>>`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeStateUpdate {
    Unchanged,
    Set(u8),  // bridge_occupancy = Some(BridgeOccupancy { deck_level })
    Clear,    // bridge_occupancy = None
}
```

### Interfaces / Contracts

**Critical parity invariant:** `on_bridge` and `loco.layer` are **distinct concepts** in gamemd and must be in our port too:

- `loco.layer` (= gamemd's A* closed-list selection / per-step layer) → **follows A*'s path_layers**. Drives walkability, cell_entry occupancy reads, cliff detection.
- `on_bridge` (= gamemd's `FootClass+0x79` analog) → **follows the cell-flag predicate**. Drives bridge-aware zone lookups, AoE layer routing (G1), and BridgeOccupancy state.

These DIFFER on ramp cells:
- Going up (Ground→Ramp→Body): on the ramp tick, `loco.layer = Bridge` (path), but `on_bridge = false` (predicate hasn't fired Enter until Ramp→Body next tick).
- Going down (Body→Ramp→Ground): on the ramp tick, `loco.layer = Ground` (path; ramp coming-off is on ground closed list per `is_at_bridge_level`), but `on_bridge = true` (predicate doesn't fire Exit until Ramp→Ground next tick).

The current code at [movement_bridge.rs:90](../../src/sim/movement/movement_bridge.rs#L90) collapses them with `*on_bridge = active_layer == Bridge` — this is the second G2 bug under the surface. Fixing it is part of this design.

**New `resolve_cell_transition_bridge_state` signature:**

```rust
pub(super) fn resolve_cell_transition_bridge_state(
    position: &mut Position,
    path_grid: Option<&PathGrid>,
    src: (u16, u16),
    dst: (u16, u16),
    next_layer: MovementLayer,
) -> BridgeStateUpdate
```

The resolver does NOT return a layer — the caller continues to use `next_layer` from A*'s path_layers for `loco.layer` / `active_layer`. The resolver's job is purely to apply the predicate, update `position.z`, and report the `BridgeStateUpdate` so `on_bridge` and `BridgeOccupancy` get updated correctly.

Body:

```rust
let Some(grid) = path_grid else {
    return BridgeStateUpdate::Unchanged;
};
let (Some(src_cell), Some(dst_cell)) = (grid.cell(src.0, src.1), grid.cell(dst.0, dst.1)) else {
    return BridgeStateUpdate::Unchanged;
};

match compute_bridge_transition(src_cell, dst_cell) {
    BridgeTransition::Enter { deck_level } => {
        position.z = deck_level;
        BridgeStateUpdate::Set(deck_level)
    }
    BridgeTransition::Exit => {
        position.z = dst_cell.ground_level;
        BridgeStateUpdate::Clear
    }
    BridgeTransition::NoChange => {
        position.z = dst_cell.effective_cell_z_for_layer(next_layer);
        BridgeStateUpdate::Unchanged
    }
}
```

**`apply_pending_bridge_render_state` signature change:**

```rust
pub(super) fn apply_pending_bridge_render_state(
    locomotor: &mut Option<LocomotorState>,
    bridge_occupancy: &mut Option<BridgeOccupancy>,
    on_bridge: &mut bool,
    active_layer: MovementLayer,        // = path_layer for loco.layer
    bridge_update: BridgeStateUpdate,   // = predicate result for on_bridge + BridgeOccupancy
    _diag_entity_id: u64,
)
```

Body (NEW — decoupled):

```rust
if let Some(loco) = locomotor {
    loco.layer = active_layer;       // path-driven (unchanged from before)
}
// on_bridge and BridgeOccupancy are predicate-driven (NEW).
// REMOVED: *on_bridge = active_layer == MovementLayer::Bridge;
match bridge_update {
    BridgeStateUpdate::Set(level) => {
        *on_bridge = true;
        *bridge_occupancy = Some(BridgeOccupancy { deck_level: level });
    }
    BridgeStateUpdate::Clear => {
        *on_bridge = false;
        *bridge_occupancy = None;
    }
    BridgeStateUpdate::Unchanged => {
        // on_bridge and bridge_occupancy retain their previous values
    }
}
```

**A* changes in `core.rs`:**

```rust
// compute_neighbor_height Case 3 (was lines 144-151):
// Case 3: Parent is NOT bridge, neighbor IS bridge. Bridge entry.
// gamemd CheckBridgeTraversal: diff == 4 AND neighbor has bridgehead (transition).
let diff = parent_height as i16 - neighbor_cell.ground_level as i16;
if diff == 4 && neighbor_cell.transition {
    neighbor_cell.bridge_deck_level
} else {
    neighbor_cell.ground_level
}
```

```rust
// astar_search neighbor walkability (was line 440):
// Ground→Bridge layer crossings additionally require the bridgehead (transition) flag.
let neighbor_passable = if neighbor_use_bridge {
    let prev_on_bridge = is_at_bridge_level(current.height, cur_cell);
    if prev_on_bridge {
        // already on bridge → any bridge_walkable cell is fine (body-to-body diagonal allowed)
        grid.is_walkable_on_layer(nx, ny, MovementLayer::Bridge)
    } else {
        // ground → bridge crossing → require bridgehead
        grid.is_walkable_on_layer(nx, ny, MovementLayer::Bridge)
            && neighbor_cell.transition
    }
} else {
    is_cell_passable_for_mover(grid, nx, ny, options.movement_zone, options.resolved_terrain)
};
```

**Caller change in `movement_step.rs` around line 664-678:**

```rust
let bridge_update = resolve_cell_transition_bridge_state(
    &mut position,
    path_grid,
    (old_rx, old_ry),    // NEW: src coords (currently only dst was passed)
    (nx, ny),            // dst coords
    next_layer,          // path_layer, for the NoChange fallback height
);
pending_bridge_update = bridge_update;
// next_layer / active_layer are NOT modified by the resolver — they continue
// to follow A*'s path_layers. loco.layer follows them.
if let Some(loco) = locomotor {
    loco.layer = next_layer;
}
active_layer = next_layer;
```

The `apply_bridge_lookahead_if_needed` call site is removed entirely. The function and its tests go with it.

### Data Flow

```
Tick N, drive entity processing:
  ┌──────────────────────────────────────────────────────────────────────────┐
  │ 1. next_layer = target.layer_at(target.next_index)                       │  ← A*
  │ 2. walkability check uses next_layer (G3/G4 enforced inline here + A*)   │
  │ 3. cliff check uses next_layer                                           │
  │ 4. occupancy.move_entity(old_rx,old_ry → nx,ny)                          │
  │ 5. bridge_update = resolve_cell_transition_bridge_state(src,dst,nxt)     │  ← G2
  │      └→ compute_bridge_transition(src_cell, dst_cell)                    │
  │      └→ returns BridgeStateUpdate { Set(level) | Clear | Unchanged }     │
  │      └→ writes position.z directly                                       │
  │ 6. apply_pending_bridge_render_state                                     │
  │      └→ loco.layer = next_layer   (A*-driven, walkability source)        │
  │      └→ on_bridge, BridgeOccupancy from bridge_update (predicate-driven) │
  │ 7. active_layer = next_layer  ← input to next tick's walkability         │  ← G6
  └──────────────────────────────────────────────────────────────────────────┘
```

The G6 fix is implicit: step 2's walkability check uses `next_layer` from A*'s `path_layers`, which is gamemd's post-two-pass output (the same layer the binary's two-pass switch would resolve to). No structural code change in `cell_entry.rs` beyond updating the TODO comment.

The G2 fix decouples `on_bridge` from `loco.layer`: the predicate's `Enter`/`Exit`/`NoChange` result drives `on_bridge` and `BridgeOccupancy` independently of A*'s path-layer assignment. This is the load-bearing parity fix — without it, `on_bridge` flickers wrong on ramp ticks (true going up, false going down) per the analysis in the "Critical parity invariant" subsection above.

### Error Handling

- `path_grid.cell(x, y)` returns `None` for out-of-bounds. The resolver returns `(fallback_layer, Unchanged)` and does NOT touch `position.z`. Out-of-bounds at the boundary-crossing point indicates a path bug elsewhere — the resolver is not the place to recover.
- `path_grid` itself is `None` only in synthetic tests and the pre-init paths. Same fallback path.
- `i8` overflow on `src_h - 4` when `src_h == i8::MIN`: in practice retail map heights are 0-15, so no overflow. Defensive: use wrapping subtraction (`src_h.wrapping_sub(4)`) to match gamemd's signed integer arithmetic exactly. If a malformed map carried `+0x11B = -128`, the wrap to 124 still produces a defined result, never a panic.

### Testing Strategy

**Unit tests for `compute_bridge_transition`** in `movement_bridge.rs::tests`:

1. `entry_from_ramp_to_body` — src: ground=4, bridge_walkable=false; dst: ground=0, bridge_walkable=true, bridge_deck_level=4. Expect `Enter { deck_level: 4 }`.
2. `exit_from_body_to_ground` — src: ground=0, bridge_walkable=true; dst: ground=0, bridge_walkable=false. Expect `Exit`.
3. `body_to_body_no_change` — src and dst both ground=0, bridge_walkable=true. Expect `NoChange`.
4. `ground_to_ground_no_change` — both bridge_walkable=false. Expect `NoChange`.
5. `ground_to_bridgehead_no_change` — src ground=0, dst ground=4, bridge_walkable=true. dst_h - src_h = 4, NOT src_h - 4. Predicate's entry requires `dst == src - 4`, fails. Expect `NoChange`. (Going UP onto a ramp is NOT an on_bridge transition; the unit is on the ramp at height 4 but not yet on the deck.)
6. `cliff_drop_off_bridge_ramp` — src ground=4, bridge_walkable=true; dst ground=0, bridge_walkable=false. Exit fires (independent of height-diff). Verifies the audit-flagged edge case (ledger #2, #13).
7. `signed_height_arithmetic` — src ground=-4 cast from u8 0xFC, dst ground=-8 cast from u8 0xF8. Verify `src_h - 4 == dst_h` works with i8 (`-4 - 4 == -8` ✓).
8. `entry_without_bridge_walkable_no_change` — height diff matches but dst.bridge_walkable=false. Expect `NoChange`.

**Unit tests for `resolve_cell_transition_bridge_state`** verifying integration:

9. Fallback when path_grid is None: returns `Unchanged`, position.z untouched.
10. Fallback when either cell is out-of-bounds: returns `Unchanged`, position.z untouched.
11. Enter case: sets position.z to deck_level, returns `Set(deck_level)`.
12. Exit case: sets position.z to dst.ground_level, returns `Clear`.
13. NoChange case with next_layer=Bridge: sets position.z to dst.bridge_deck_level (effective for Bridge).
14. NoChange case with next_layer=Ground: sets position.z to dst.ground_level.

**Decoupling tests for `apply_pending_bridge_render_state`**:

15. `on_bridge_decoupled_from_loco_layer` — call with `active_layer=Bridge` and `bridge_update=Unchanged`; verify `on_bridge` retains its prior value (does NOT get set to true just because layer is Bridge). Regression guard for the old `*on_bridge = active_layer == Bridge` line.
16. `ramp_going_up_keeps_on_bridge_false` — simulate Ground→Ramp transition: `active_layer` flips to Bridge (path), `bridge_update=Unchanged` (predicate doesn't fire). Verify `loco.layer == Bridge` AND `on_bridge == false`.
17. `ramp_going_down_keeps_on_bridge_true` — simulate Body→Ramp transition: `active_layer` flips to Ground (path; ramp coming-off is on ground closed list), `bridge_update=Unchanged`. Verify `loco.layer == Ground` AND `on_bridge == true` (still on bridge structurally).

**A* regression tests** in `core_tests.rs`:

18. `astar_blocks_height_diff_2` — adjacent ground cells at heights 0 and 2, no bridge. Expect no path through that step (G4).
19. `astar_blocks_height_diff_3` — similar at heights 0 and 3. Expect no path (G4).
20. `astar_allows_height_diff_4_with_bridgehead` — ground at height 0, bridge cell at height 0 with `transition=true, bridge_walkable=true, bridge_deck_level=4`. Path expected, layer should switch to Bridge (G3).
21. `astar_blocks_height_diff_4_without_bridgehead` — same but `transition=false`. Path must NOT route Ground→Bridge through this cell (G3).
22. `astar_allows_body_to_body_diagonal` — two adjacent body cells (bridge_walkable, no transition). Unit already on Bridge layer can move between them (regression: G3 must not over-tighten).

**Integration tests** in `movement_tests.rs`:

23. `on_bridge_fires_at_ramp_to_body_only` — step a unit Ground→Ramp→Body. Assert `on_bridge` is `false` on the ramp tick AND becomes `true` on the body tick exactly (predicate timing, not anticipatory).
24. `on_bridge_clears_at_ramp_to_ground_only` — step a unit Body→Ramp→Ground. Assert `on_bridge` is `true` on the body tick AND on the ramp tick (descending), and becomes `false` only on the Ground tick (predicate exit timing).
25. `no_bridge_lookahead_pre_claim` — verify that the deleted lookahead does NOT prematurely set `BridgeOccupancy`. (Regression test: ensures the deletion didn't reintroduce the old behavior via some other path.)

**Determinism / parity verification:**

26. After implementation, run an existing replay on a bridge map; verify that the new on_bridge timing matches gamemd.exe behavior via `/fidelity-check bridge-crossing` (or equivalent). State hash should be deterministic across runs but is allowed to diverge from pre-fix replays.

## Architectural Decisions

**Pattern followed:** pure-function predicate helpers in `movement_bridge.rs` with explicit input/output types. Mirrors how `bump_crush.rs` and `cell_entry.rs` structure their internal helpers — small, testable, no hidden state.

**Pattern deviation:** none. The lookahead deletion removes an existing pattern (anticipatory state update) but that pattern was a workaround, not a deliberate design.

**Tech debt addressed:** removes `apply_bridge_lookahead_if_needed`, the `_next_layer` underscore-prefixed-unused parameter, and the `Option<Option<u8>>` return type.

**Tech debt introduced:** none.

**Determinism:** `compute_bridge_transition` is pure; `i8` arithmetic is deterministic; `wrapping_sub` is deterministic. No floats touch the predicate.

## Known Parity Boundary

Items NOT addressed by this design — explicitly out of scope, but flagged here so the parity bar is honest about what remains.

1. **Diff-1 SlopeIndex passability.** gamemd's `CheckBridgeTraversal` for height diff 1 reads `cell+0x11C` (SlopeIndex byte) — non-zero SlopeIndex means a terrain ramp (passable), zero means a cliff (blocked). Our `PathCell` doesn't expose SlopeIndex. Today this only manifests on user-made maps with stepped terrain; retail maps are unaffected (terrain ramps in retail always carry the standard SlopeIndex). Listed as a candidate follow-up; not a G2/G3/G4/G6 concern.

2. **True two-pass `Can_Enter_Cell`.** gamemd's `Can_Enter_Cell` runs the occupancy check on one layer, then re-runs on the other layer if `prevFacing == cell.height + 4 AND cell has 0x100`. Our `cell_entry::check_terrain` is single-pass with `target_layer` pre-decided from A*'s `path_layers`. The pre-decision matches the post-switch output of gamemd's two-pass for the cell transitions we care about (ground→bridgehead→body, body→bridgehead→ground, body-to-body). Strictly identical mechanism would require a refactor to `cell_entry`; the observable output should match in retail behavior.

3. **Exact .TMP-data +0x11B byte values.** Whatever specific heights individual ramp/body tiles carry in the retail `.TMP` files is not asserted by this design. The design holds as long as the 4-level invariant (verified at gamemd 0x4AF4A0) holds — which is itself a runtime engine guarantee, not a tile-data guarantee.

4. **G1 (AoE layer routing) and G5 (bridge cost shaping)** from the gap-scan. Separate scope; G1 fires every match with bridge AoE weapons and is the gap-scan's only "HIGH severity must-fix"; tracked as a follow-up brainstorm.

5. **`SetBridgeDirection_NESW` / `_NWSE`.** No Rust port exists; bridge-flag state in `PathCell` is set at map-load time (the specific gamemd load path that populates bit 0x80 was not traced — only the post-condition that cells carry the bit before SetBridgeDirection's construction call runs). The audit-verified byte-identity of NESW/NWSE (AUDIT_LOG 2026-05-11) is informational only — it doesn't affect this design.

## Alternatives Considered

**Approach B — Keep lookahead as degraded-mode fallback.** Retains `apply_bridge_lookahead_if_needed` for cases where `path_grid` is unavailable mid-tick. Rejected: introduces two parallel mechanisms that can disagree; diverges from gamemd which has no anticipatory layer change. The `path_grid is None` case is rare and naturally handled by the resolver returning `(fallback_layer, Unchanged)`.

**Approach C — Move predicate onto `PathCell` as a method.** `PathCell::transition_from(&self, src: &PathCell) -> BridgeTransition`. Trivial to unit-test in isolation. Rejected: `PathCell` methods are queries (return cell properties), not state-transition computers. Mixing concerns. The standalone helper in `movement_bridge.rs` is just as testable and keeps `PathCell` focused.

**Approach A-with-flag — Disable lookahead behind a `cfg`.** Defensive programming for the case where the new path breaks something. Rejected: adds a dead code path and an untested feature flag; if the new path is wrong, fix it, don't gate it.

## Sources & References

- **Gap-scan:** [docs/gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md](../gap-scans/2026-05-11-disparity-scan-bridge-pathfinding.md) (G2 §64-79, G3 §81-99, G4 §100-112, G6 §134-152)
- **Verified gamemd invariants:** [ra2-rust-game-docs/AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md) entries dated 2026-05-11
- **Bridge system reference:** [ra2-rust-game-docs/BRIDGE_SYSTEM.md](../../../ra2-rust-game-docs/BRIDGE_SYSTEM.md) §"Bridge Ramp Detection", §"CheckBridgeTraversal", §"A* Pathfinding — Dual-Layer Bridge Support", §"RecalcAttributes Bridge Correction"
- **Can_Enter_Cell two-pass:** ra2-rust-game-docs/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md §"Phase 6"
- **gamemd.exe addresses (in audit log, not in code):**
  - 0x4B0F20 — `DriveLocomotionClass::Process_Drive_Track` (on_bridge predicate)
  - 0x4B1812 / 0x4B1830 / 0x4B184A — predicate asm sites (SUB EAX,4; flag tests)
  - 0x4AF4A0 — `DriveLocomotionClass::ComputeBridgeZOffset` (formula `ftol(HeightStep * 4)`)
  - 0x4AFD40 / 0x4AFDDD / 0x4AFDE8 — `Set_Destination` dst.0x100 Z bump
  - 0x429A90 — `PathfinderClass::AStar_main_loop` (dual closed lists)
  - 0x4D9C60 — `CheckBridgeTraversal` (height-diff rules)
- **Repo code:**
  - [src/sim/movement/movement_bridge.rs](../../src/sim/movement/movement_bridge.rs)
  - [src/sim/movement/movement_step.rs:467-686](../../src/sim/movement/movement_step.rs#L467-L686)
  - [src/sim/pathfinding/core.rs:120-152](../../src/sim/pathfinding/core.rs#L120-L152)
  - [src/sim/pathfinding/core.rs:425-470](../../src/sim/pathfinding/core.rs#L425-L470)
  - [src/sim/pathfinding/cell_entry.rs:85-130](../../src/sim/pathfinding/cell_entry.rs#L85-L130)
  - [src/sim/components.rs:200-303](../../src/sim/components.rs#L200-L303) — `MovementTarget.path_layers` + `layer_at`
