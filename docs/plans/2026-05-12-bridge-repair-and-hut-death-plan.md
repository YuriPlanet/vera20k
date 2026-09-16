# Bridge Repair + Hut-Death-Destroys-Bridge Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained.

**Goal:** Wire engineer-enters-CABHUT to Destroyed→Healthy bridge transition
AND C4/demo-truck-on-CABHUT to Healthy→Destroyed collapse, both flowing
through the existing `zones_dirty → refresh_bridge_zones_if_dirty`
PathGrid rebuild.

**Architecture:** New reverse state machine in `bridge_state/mod.rs` parallels
the existing `body_cell_advance_state`. New `tick_bridge_repair_orders` in
`world_orders.rs` runs alongside `tick_capture_orders` and `tick_c4_plants`
in Phase 5. C4 destroy branches inside `apply_c4_damage_to_building`. The
existing `refresh_bridge_zones_if_dirty` handles the rebuild for both
directions transparently.

**Design Doc:** [docs/plans/2026-05-12-bridge-repair-and-hut-death-design.md](2026-05-12-bridge-repair-and-hut-death-design.md)

---

## Grounding Summary

**What the docs already tell us** — [BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md)
(Phases 1+2, completed earlier today). §12.4 has the complete overlay
state machine for all 4 repair walkers. §13 has the destruction tree.
§12.5: `FUN_00598030` is rejection-sampling RNG, not LAT. §12.6:
zones-rebuild gated on main-deck repair only. §12.7: radar-dirty only on
Destroyed→Healthy transitions. §13.4: vanilla `UpdateAdjacentBridges_High`
copy-paste bug. §15: vtable[0x160] is Iron Curtain (not Immune); C4
keystone lies upstream and is OUT OF SCOPE.

**What Ghidra verified** — all 28 ledger items in the design doc are
sourced to specific RE addresses. No remaining contradictions; Phase 3
items in §19 of the RE report are refinements (variant observability,
tag-trigger fires, etc.) deferred to a separate pass.

**Repo pattern mirrored** — the new code follows existing patterns:
- Reverse state machine: parallel to [`body_cell_advance_state`](../../src/sim/bridge_state/mod.rs#L756) (Healthy→Damaged→Destroyed).
- Phase-5 trigger: same shape as [`tick_capture_orders`](../../src/sim/world/world_orders.rs#L151) and [`tick_c4_plants`](../../src/sim/world/world_orders.rs#L228).
- Sound event: same shape as [`SimSoundEvent::C4Planted`](../../src/sim/world/mod.rs#L168) (sim→app conversion at [app_sim_tick.rs:496](../../src/app_sim_tick.rs#L496)).
- Type-flag check: `rules.object(interner.resolve(type_ref)).map_or(false, |t| t.bridge_repair_hut)` — same as widespread `bridge_repair_hut: false` test fixtures in [src/sim/movement/](../../src/sim/movement/).

**INI keys** — both already parsed:
- `BridgeRepairHut=yes` → `ObjectType.bridge_repair_hut` at [src/rules/object_type.rs:483](../../src/rules/object_type.rs#L483).
- `RepairBridgeSound=` → `BridgeRules.repair_sound: Option<String>` at [src/rules/ruleset.rs:700](../../src/rules/ruleset.rs#L700).

**Git-state re-verify** — files this plan touches have no commits AFTER
the design doc landed. Most recent commits are this branch's
already-committed bridge work (c336aff, 8ec90d8, 0263f1a — they ADD
`bridge_state_changed: bool` to TickResult, which this plan reuses).
Design premise still valid.

**What's still unknown after grounding** (carried as Deferred Open
Questions):
1. ~~Whether the variant byte we store on `DamageState::Healthy { variant }`
   is observed at render time.~~ **RESOLVED during review** — renderer at
   [src/app_instances/bridges.rs:71-75](../../src/app_instances/bridges.rs#L71-L75)
   applies Latin-square jitter only for `variant: 0`; for `variant: 1..=3`
   the renderer uses `variant` directly as the frame offset (still healthy
   frames per the SHP layout, 0..=3 EW / 9..=12 NS). For `variant: 4..=5` the
   frame falls into the damage progression range (4..=8 EW / 13..=17 NS) — so
   variants 4/5 are RESERVED for `update_ramp_perpendicular` to encode
   NS DamageA/B transition states (see
   [bridge_specs.rs:1303,1312](../../src/sim/bridge_specs.rs#L1303-L1312)).
   The repair walker MUST stay within `0..=3` to render as healthy.
2. Exact identity of the upstream Immune gate that blocks C4 placement on
   CABHUT (`project_c4_bridge_hut_followup` — out of scope for this plan).
3. Whether the demo-truck unit will reuse `apply_c4_damage_to_building`
   or get its own damage function — TBD when demo-truck lands.

---

## Key Technical Decisions

- **Engineer trigger reuses `capture_target` field, not a new field.** Branches at
  tick time on `target.bridge_repair_hut`. — **Confidence:** high.
  **Source:** Design doc §"Chosen Approach"; matches gamemd's mission-8 +
  `Type[0x16B6]` branch.
- **5×5 scan inclusive `[-2..=+2]`, NOT a hut→span registry.** Per-trigger
  scan matches gamemd geometry; anchor-span lookup converts scanned cells
  to unique spans. — **Confidence:** high.
  **Source:** [RE §3.1 step C](../../../ra2-rust-game-docs/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md), design doc §"Approach A".
- **RNG draw per main-deck cell, NOT per strip.** Simpler; deterministic
  across our Rust clients. Draws `rng.next_range_u32(4)` → variant `0..=3`
  (matches gamemd's `FUN_00598030(3)` healthy band exactly). — **Confidence:**
  high. **Source:** Design doc §"Tiny-Detail Ledger" item #9 + RE §12.5
  (FUN_00598030 limit=3) + renderer check at
  [src/app_instances/bridges.rs:65-86](../../src/app_instances/bridges.rs#L65-L86).
  Note: gamemd draws once per strip (3 cells share variant). RNG-advance-count
  diverges from gamemd, but the variant *distribution* across cells is
  statistically equivalent. Variant range MUST be `0..=3` because the
  renderer interprets `Healthy{variant: 4}` and `Healthy{variant: 5}` as
  encoded NS DamageA/B states (see
  [bridge_specs.rs:1303,1312](../../src/sim/bridge_specs.rs#L1303-L1312)) —
  those map to damage SHP frames per
  [BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md §5](../../../ra2-rust-game-docs/BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md).
- **`tick_bridge_repair_orders` runs BEFORE `tick_capture_orders` in
  Phase 5.** Despawns engineer first; capture sees no engineer for CABHUT
  targets. Plus explicit skip in `tick_capture_orders` as defense in depth.
  — **Confidence:** high. **Source:** Design doc §"Components".
- **Destroy hook: skip hut damage when `bridge_repair_hut`.** Mirrors
  gamemd `BuildingClass::Update` branch that does NOT call vtable[0x16C]
  for BridgeRepairHut. Hut survives the C4. — **Confidence:** high.
  **Source:** [RE §3.2](../../../ra2-rust-game-docs/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md#L321).
- **Destroy hook drives forward state machine to convergence.** Loops
  `body_cell_advance_state` per cell until Destroyed or NoChange. Single
  tick reaches final state regardless of starting state. — **Confidence:**
  high. **Source:** Existing forward state machine semantics.
- **No separate Low/High dispatcher pair in Rust.** `BridgeRuntimeCell`
  carries band info implicitly (via `axis` + `anchor_span_id`); we don't
  need parallel Low/High function trees. The walker-naming in gamemd
  exists because the binary writes overlay bytes per-band; we write
  `DamageState` enum values that are band-agnostic. — **Confidence:** high.
  **Source:** Design doc §"Architectural Decisions".

---

## Open Questions

### Resolved During Planning

- **Q:** Should `tick_capture_orders` get a `rules` parameter?
  **A:** Yes. Required to read `target.bridge_repair_hut`. Single call
  site at world/mod.rs:1204, trivial signature change.
- **Q:** How do we resolve entity → ObjectType in Rust?
  **A:** `rules.object(self.interner.resolve(entity.type_ref))` — pattern
  is used widely (app_commands.rs:677, ai.rs:596, etc.).
- **Q:** Does `SimSoundEvent::BridgeRepaired` need an `InternedId` for the
  sound name?
  **A:** No. App layer reads `rules.bridge_rules.repair_sound` directly
  at dispatch time. Variant just carries `{ rx, ry, owner }`.
- **Q:** Do we need to add Bridge-related logic to `world_commands.rs`
  (order-issuance)?
  **A:** No. Engineer→CABHUT clicks already set `capture_target` via the
  existing capture-order path. The new behavior branches at tick time, not
  order-issuance time.

### Deferred to Implementation

- **Q:** Exact RNG-draw sequence for the strip-iteration-order pin test
  (Task 4). Answer requires writing the implementation and observing.
- **Q:** Will the app-layer `GameSoundEvent::BridgeRepaired` arm need any
  additional state beyond what `SimSoundEvent::BridgeRepaired` carries?
  TBD when we write the app match arm in Task 7.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `src/sim/bridge_state/mod.rs` | Add `RepairOutcome` struct, `cells_in_5x5_scan` helper, `body_cell_repair_state` reverse state machine (draws `rng.next_range_u32(4)`, NOT `(6)` — see Task 3), `anchor_span_mut` helper, unit tests |
| Modify | `src/sim/world/mod.rs` | Add `SimSoundEvent::BridgeRepaired { rx, ry, owner }` variant; wire `tick_bridge_repair_orders` and updated `tick_c4_plants` outcome into `advance_tick` (line ~1205); pass `rules` to `tick_capture_orders` |
| Modify | `src/sim/world/world_orders.rs` | Add `tick_bridge_repair_orders` (new); add `bridge_repair_hut` skip-branch + `rules: &RuleSet` param to `tick_capture_orders`; widen `apply_c4_damage_to_building` return to `C4DamageOutcome { killed_building, bridge_state_changed }`; widen `tick_c4_plants` return to `C4TickOutcome`; add `bridge_repair_hut` branch in `apply_c4_damage_to_building` that calls `bridge_orchestrator::dispatch_bridge_collapse_from_hut` |
| Modify | `src/sim/world/bridge_orchestrator.rs` | Add `pub(crate) fn dispatch_bridge_collapse_from_hut(sim, rules, hut_center) -> bool` that drives forward state machine + BlowUpBridge cascade (reuses existing module-private `kill_ground_occupants_at`, `drop_in_bridge_deck_entities`, `spawn_bridge_debris`, `update_adjacent_bridges`, `notify_bridge_span_collapse`, `refresh_bridge_zones_if_dirty`) |
| Modify | `src/app_sim_tick.rs` | Add match arm for `SimSoundEvent::BridgeRepaired` (mirrors `C4Planted` arm at line 496); resolve `repair_sound` from rules + dispatch EVA if owner is local human |
| Create | New integration test file (or extend an existing `world_orders_tests.rs`-like) | Integration tests for repair flow (Tasks 9, 11) |

**No new files in `sim/`**; `bridge_state/mod.rs` is already large but
this addition stays cohesive (forward + reverse state machines belong
together). If the file crosses 1200 lines, defer a submodule split to a
follow-up — not part of this plan.

---

## Interface Changes

**`tick_capture_orders` signature change** (one call site at
`world/mod.rs:1204`):
```rust
// Before:
pub(crate) fn tick_capture_orders(&mut self) -> bool;
// After:
pub(crate) fn tick_capture_orders(&mut self, rules: &RuleSet) -> bool;
```
**Depends on it:** only the single call in `advance_tick`. No test code
references this directly (capture is tested via integration). Update the
call site in Task 8.

**`SimSoundEvent` enum** — additive variant; existing match arms unaffected
unless they're non-exhaustive (`_ => ...`). Existing matches at
`app_sim_tick.rs:496` use explicit per-variant arms; **MUST add** a
`BridgeRepaired` arm or compile fails. Update in Task 7.

**`BridgeRuntimeState` public API** — additive:
- `pub fn body_cell_repair_state(&mut self, scan_cells: &[(u16, u16)], rng: &mut SimRng) -> RepairOutcome`
- `pub fn cells_in_5x5_scan(center: (u16, u16)) -> impl Iterator<Item = (u16, u16)>`
- `pub fn anchor_span_mut(&mut self, id: u16) -> Option<&mut AnchorSpan>` (mirror of existing `anchor_span`; needed by Task 3 to sync the span's mirror `damage_state` after per-cell repair)

No existing callers affected.

**`apply_c4_damage_to_building` return type change** (private to
`world_orders.rs`):
```rust
// Before:
fn apply_c4_damage_to_building(...) -> bool;
// After:
fn apply_c4_damage_to_building(...) -> C4DamageOutcome;
```
Reason: must propagate `bridge_state_changed` from the BridgeRepairHut
collapse branch alongside the existing "killed building" signal. Single
caller (`tick_c4_plants`) updated in Task 11.

**`tick_c4_plants` return type change** (one call site at world/mod.rs:1206):
```rust
// Before:
pub(crate) fn tick_c4_plants(&mut self, rules: &RuleSet) -> bool;
// After:
pub(crate) fn tick_c4_plants(&mut self, rules: &RuleSet) -> C4TickOutcome;
```
Caller destructures both flags. Existing tests that pattern-match the
bool return (if any) must update — verify in Task 11 Step 6.

---

## Sim Checklist

- [x] All math uses `fixed`-point — no f32/f64 in new code (RNG draw,
      coord scan, state transitions are all `u16`/`u32`/`u8` integer math).
- [x] New state included in deterministic state hash — `BridgeRuntimeCell.damage_state`
      already hashed by existing path.
- [x] No dependencies on render/ui/sidebar/audio/net — all new sim code
      stays in `sim/`. App-layer code in `app_sim_tick.rs` reads sim output;
      sim does not depend on app.
- [x] Tick ordering impact noted — `tick_bridge_repair_orders` slots into
      Phase 5 BEFORE `tick_capture_orders` (defensive). All new mutation
      happens before this tick's `tick_combat`.
- [x] BTreeMap iteration order considered — engineer iteration uses
      `entities.keys_sorted()`; anchor-span lookup uses `BTreeSet` for
      deterministic ordering.

---

## Risk Areas

From impact analysis in the design doc:

1. **Strip-iteration order for RNG parity** — locked by the iteration-order
   pin test (Task 4 includes this). If anyone reorders `AnchorSpan.cells`
   later, the pin test fails loudly.
2. **C4-on-CABHUT integration test is `#[ignore]`'d** — blocked on
   upstream Immune fix (`project_c4_bridge_hut_followup`). Documented in
   the test attribute, not silently skipped.
3. **Variant byte at render time — RESOLVED.** The Rust renderer DOES read
   the variant for `variant: 1..=5`. Variants `0..=3` are the four healthy
   frames (matching gamemd's `FUN_00598030(3)` output); variants `4..=5` are
   encoded NS DamageA/B transition states. The repair walker MUST draw
   `rng.next_range_u32(4)` so all main-deck cells land on healthy frames.
   Using `..._u32(6)` would cause ~33% of repaired cells to render with
   damage-progression SHP frames.
4. **Vanilla `UpdateAdjacentBridges_High` copy-paste bug** — our zone
   rebuild is band-agnostic, so this is a no-op in Rust. Documented in
   the dispatch helper docstring (Task 10).
5. **Two engineers same tick on same CABHUT** — both trigger repair
   (gamemd has no per-tick guard). Idempotent state-wise, but causes 2x
   RNG draws and 2x sound events. Acceptable parity outcome.

---

## Parity-Critical Items

| Task # | Item | Why it matters | Verification |
|--------|------|----------------|--------------|
| Task 3 | 5×5 scan inclusive `[-2..=+2]` (25 cells) | Off-by-one in scan radius changes which bridges get found per trigger — observable on edge cases | RE §3.1 + unit test |
| Task 3 | Zones rebuild ONLY on main-deck repair, NOT bridgehead-only | Bridgehead-only repair must NOT trigger zone rebuild — observable as needless PathGrid churn | RE §12.6 + unit test 4 |
| Task 3 | Radar dirty ONLY on Destroyed→Healthy (not on Damaged→Healthy) | Minimap visual differs — observable on the radar pip pattern after repair | RE §12.7 + unit test 1 vs 2 |
| Task 3 | Bridgehead repair uses fixed base, NO RNG draw | RNG state advance count must match cell role — observable in replay determinism | RE §12.4-12.5 + unit test 3 |
| Task 3 | Already-healthy cells: no mutation, no RNG draw | Idempotency required for two-engineer same-tick case + replay determinism | RE §12.3 + unit test 4 |
| Task 3 | Variant range MUST be `0..=3` (`rng.next_range_u32(4)`) | Variants 4/5 encode NS DamageA/B and render as damage-progression SHP frames — `(6)` would visually mis-render ~33% of repaired cells | Renderer check at app_instances/bridges.rs:71-75 + bridge_specs.rs:1303,1312; defensive assertion in pin test |
| Task 4 | Strip-iteration order pin | Locks RNG draw sequence across changes — protects lockstep | New test; pinned bytes |
| Task 10 | Full BlowUpBridge cascade fires on hut death (not just state-machine progression) | Ground units must die, deck units must drop-in, debris/rim/zones must refresh — symmetric with `apply_bridge_damage_events` | RE §13.2 unconditional `UpdateBridgeZonesHelper` + cascade in `bridge_orchestrator.rs:71-150` |
| Task 11 | `bridge_state_changed` propagates through `C4DamageOutcome` → `C4TickOutcome` → caller | App needs the flag to rebuild PathGrid after bridge collapse; otherwise A* keeps treating the destroyed bridge as walkable | TickResult docs at world/mod.rs:85; symmetric with damage-side OR at world/mod.rs:1247 |
| Task 7 | Sound at building location (not engineer) | Spatial audio panning differs by position — observable | RE §3.1 step B + sound at building.rx/ry |
| Task 7 | EVA gated on local human (owner match) | EVA voice is per-player; non-local-human players should NOT hear "Bridge repaired" — observable | RE §3.1 step A + owner == local human check |
| Task 7 | Sound gated on `repair_sound.is_some()` | If `RepairBridgeSound=` not set in rules, no sound plays — RE confirms `RulesClass+0x248 != -1` gate | RE §3.1 step B |
| Task 8 | Trigger ordering EVA→sound→scan→mutation | Audio must fire BEFORE state mutation in same tick | RE §3.1 + sequence in `tick_bridge_repair_orders` |
| Task 8 | `bridge_state_changed = true` on repair | App needs this flag to rebuild PathGrid; symmetric with destroy | TickResult docs at world/mod.rs:85 |
| Task 10 | Hut survives C4 (skip damage) | The hut MUST NOT die in same call — observable as hut sprite present after explosion | RE §3.2 + integration test 12 (ignored) |
| Task 10 | Forward state machine drives Destroyed in same tick | Bridge must be fully Destroyed by end of tick — not Damaged-then-Destroyed-next-tick | Loop `body_cell_advance_state` to convergence + integration test |

---

## Tasks

### Task 1: Define `RepairOutcome` struct

**Why:** Types-first. The reverse state machine's return type encodes
which side-effects the caller must fire (zones rebuild, radar dirty).
Defining this first lets later code reference it.

**Files:**
- Modify: `src/sim/bridge_state/mod.rs` (add struct near `StateOutcome`
  around line 290–350; choose a placement adjacent to existing return-type
  definitions)

**Pattern:** Same shape as `StateOutcome` (existing enum with
side-effect data on variants).

**Step 1: Add struct + docstring**
```rust
/// Outcome of a single `body_cell_repair_state` call. Carries the
/// side-effects the caller must fire AFTER state mutation.
///
/// Mirrors gamemd's repair walker side-effects (RE §12.6, §12.7):
///   - `zones_dirty`: rebuild PathGrid + zone grid. Set only when a
///     **main-deck damaged or destroyed** cell was repaired —
///     bridgehead-only repairs do NOT trigger zones rebuild.
///   - `radar_cells`: mark these cells dirty in the minimap. Set only
///     for cells that transitioned **from Destroyed** to Healthy
///     (`overlay 0x64/0x65/0xE7/0xE8 → healthy variant` in gamemd).
///   - `repaired_cells`: total mutated cell count for caller's
///     `bridge_state_changed` decision and metrics.
#[derive(Debug, Clone, Default)]
pub struct RepairOutcome {
    pub zones_dirty: bool,
    pub radar_cells: Vec<(u16, u16)>,
    pub repaired_cells: u32,
}
```

**Step 2: Verify compile**
Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS (struct adds, no callers yet)

**Step 3: Commit**
Message: `sim/bridge_state: add RepairOutcome struct for reverse state machine`

---

### Task 2: Add `cells_in_5x5_scan` helper

**Why:** Used by both the repair trigger (Task 8) and the destroy hook
(Task 10). Stand-alone helper, no state. Pulling it out first prevents
inline-duplication later.

**Files:**
- Modify: `src/sim/bridge_state/mod.rs` (add free function, NOT a method
  on `BridgeRuntimeState`, since callers compute the center coord and
  don't need bridge state to enumerate the scan)

**Pattern:** Free function adjacent to other coord helpers in the module.

**Step 1: Add function**
```rust
/// Enumerate the 25 cells in a 5×5 inclusive `[-2..=+2]` scan around
/// `center`. Yields cell coordinates clamped to non-negative `(u16, u16)`
/// (cells with negative computed coords are skipped — they're off-map).
///
/// Mirrors gamemd's 5×5 scan in `InfantryClass::PerCellProcess` (RE §3.1
/// step C), `BuildingClass::Update` (RE §3.2), and `BombClass::Detonate`
/// (RE §3.7). Inclusive bounds `-2..=+2` produce exactly 25 cells when
/// the center is interior; off-map negative cells are silently dropped.
pub fn cells_in_5x5_scan(center: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
    let (cx, cy) = (center.0 as i32, center.1 as i32);
    (-2..=2i32).flat_map(move |dy| {
        (-2..=2i32).filter_map(move |dx| {
            let nx = cx + dx;
            let ny = cy + dy;
            if nx < 0 || ny < 0 || nx > u16::MAX as i32 || ny > u16::MAX as i32 {
                None
            } else {
                Some((nx as u16, ny as u16))
            }
        })
    })
}
```

**Step 2: Add a small unit test**
```rust
#[cfg(test)]
mod scan_tests {
    use super::cells_in_5x5_scan;

    #[test]
    fn cells_in_5x5_scan_interior_yields_25_cells() {
        let cells: Vec<(u16, u16)> = cells_in_5x5_scan((10, 10)).collect();
        assert_eq!(cells.len(), 25);
        // Spot-check corners and center
        assert!(cells.contains(&(8, 8)));    // -2,-2
        assert!(cells.contains(&(12, 12)));  // +2,+2
        assert!(cells.contains(&(10, 10)));  // 0,0
    }

    #[test]
    fn cells_in_5x5_scan_at_origin_clamps_negative() {
        let cells: Vec<(u16, u16)> = cells_in_5x5_scan((0, 0)).collect();
        // Only (0,0)..(2,2) range — 3×3 = 9 cells
        assert_eq!(cells.len(), 9);
        assert!(cells.contains(&(0, 0)));
        assert!(cells.contains(&(2, 2)));
        assert!(!cells.iter().any(|(x, _)| *x > 2));
    }

    #[test]
    fn cells_in_5x5_scan_at_edge_clamps_one_side() {
        let cells: Vec<(u16, u16)> = cells_in_5x5_scan((1, 5)).collect();
        // X range: [0..=3] = 4. Y range: [3..=7] = 5. Total = 20.
        assert_eq!(cells.len(), 20);
    }
}
```

**Step 3: Run tests**
Run: `cargo test --lib bridge_state::scan_tests`
Expected: 3 tests PASS.

**Step 4: Commit**
Message: `sim/bridge_state: add cells_in_5x5_scan helper for bridge triggers`

---

### Task 3: Implement `body_cell_repair_state` reverse state machine

**Why:** Core of the design. Pure on `BridgeRuntimeState` (no Simulation
borrow), testable in isolation with a seeded RNG. Reverses
Damaged/Destroyed/PartialCollapse{A,B} → Healthy{variant}.

**Files:**
- Modify: `src/sim/bridge_state/mod.rs` (add method on `BridgeRuntimeState`
  near `body_cell_advance_state` around line 756)

**Pattern:** Mirror `body_cell_advance_state`. Same RNG-injection
(via `&mut SimRng` param), same per-cell scan, same anchor-span
dereference. Returns `RepairOutcome` instead of `StateOutcome` because
the side-effect surface differs (no `set_bridge_direction` cascade on
repair; radar dirty list instead).

**Step 1: Add method**
```rust
/// Reverse counterpart to `body_cell_advance_state`. Repairs cells found
/// in `scan_cells`: collects unique `anchor_span_id`s, iterates each
/// span's cells (slots 0..5), and transitions Damaged/Destroyed/
/// PartialCollapse{A,B} → Healthy.
///
/// Mirrors gamemd's `RepairBridgeWalker_*` family (RE §12). The Rust
/// model uses anchor-span iteration in place of the binary's
/// 3-cell-perpendicular-strip walker — the cell-state mutations are
/// equivalent; the binary's RNG draw count differs (per-strip vs
/// per-cell), but is locked across our Rust clients by the iteration
/// order test.
///
/// **Side-effect gating** (per RE §12.6 + §12.7):
///   - `outcome.zones_dirty = true` iff at least one **main-deck**
///     (Anchor/Body/Tail role) damaged or destroyed cell was repaired.
///     Bridgehead-only repairs do NOT set this flag.
///   - `outcome.radar_cells` contains cells whose prior state was
///     `Destroyed`. Cells transitioning from Damaged or PartialCollapse
///     are NOT added.
///
/// **RNG draws** (locked for lockstep across Rust clients):
///   - Main-deck damaged/destroyed/partial-collapse → 1 draw per cell
///     (`rng.next_range_u32(4)` → variant `0..=3`). MUST stay in `0..=3`
///     because variants 4/5 are RESERVED for `update_ramp_perpendicular`
///     to encode NS DamageA/B (renders as damage-progression SHP frame
///     4 or 5 EW, 13 or 14 NS — see app_instances/bridges.rs:71-75 +
///     bridge_specs.rs:1303,1312). Matches gamemd `FUN_00598030(3)`.
///   - Bridgehead damaged → write `Healthy { variant: 0 }`, **0 draws**.
///   - Already-Healthy or non-bridge cells → skip, **0 draws**.
///
/// **Iteration order** (parity-critical, locked by test):
///   1. Anchor spans collected into `BTreeSet<u16>` for sorted iteration.
///   2. Within each span, cells iterated in slot order 0..=5
///      (anchor, +1, +2, +3, -1, fixed-offset).
///   3. `None` slots skipped.
pub fn body_cell_repair_state(
    &mut self,
    scan_cells: &[(u16, u16)],
    rng: &mut crate::sim::rng::SimRng,
) -> RepairOutcome {
    use std::collections::BTreeSet;
    let mut outcome = RepairOutcome::default();

    // Step 1: Collect unique anchor spans from scan cells.
    let mut spans: BTreeSet<u16> = BTreeSet::new();
    for &(rx, ry) in scan_cells {
        if let Some(cell) = self.cell(rx, ry) {
            if let Some(span_id) = cell.anchor_span_id {
                spans.insert(span_id);
            }
        }
    }

    // Step 2: Iterate each span; for each cell, transition damage_state.
    for span_id in spans {
        // Clone span cell list to avoid borrow conflict.
        let cells_list: [Option<(u16, u16)>; 6] = match self.anchor_span(span_id) {
            Some(span) => span.cells,
            None => continue,
        };

        for slot in 0..6 {
            let Some(cell_pos) = cells_list[slot] else { continue };
            let Some(prior_state) = self.cell(cell_pos.0, cell_pos.1).map(|c| c.damage_state) else { continue };
            let Some(role) = self.cell(cell_pos.0, cell_pos.1).map(|c| c.role) else { continue };

            // Classify and dispatch.
            let new_state: DamageState = match (role, prior_state) {
                // Already healthy: skip, no RNG draw.
                (_, DamageState::Healthy { .. }) => continue,

                // Bridgehead: fixed variant, no RNG.
                (BridgeCellRole::Bridgehead, _) => DamageState::Healthy { variant: 0 },

                // Main-deck (Anchor/Body/Tail) damaged/destroyed/partial: RNG variant.
                (BridgeCellRole::Anchor | BridgeCellRole::Body | BridgeCellRole::Tail,
                 DamageState::Damaged
                 | DamageState::Destroyed
                 | DamageState::PartialCollapseA
                 | DamageState::PartialCollapseB) => {
                    // Variant range MUST be 0..=3 (rng.next_range_u32(4));
                    // variants 4/5 encode NS DamageA/B in our render model
                    // and would draw damage-progression SHP frames.
                    let variant = rng.next_range_u32(4) as u8;
                    DamageState::Healthy { variant }
                }
            };

            // Apply mutation.
            if let Some(cell) = self.cell_mut(cell_pos.0, cell_pos.1) {
                cell.damage_state = new_state;
            }
            outcome.repaired_cells += 1;

            // Side-effect flags.
            let is_main_deck = matches!(
                role,
                BridgeCellRole::Anchor | BridgeCellRole::Body | BridgeCellRole::Tail
            );
            if is_main_deck {
                outcome.zones_dirty = true;
            }
            if matches!(prior_state, DamageState::Destroyed) {
                outcome.radar_cells.push(cell_pos);
            }
        }

        // Step 3: Sync the AnchorSpan's mirror `damage_state` field with
        // the anchor cell's new state (the span struct caches this for
        // queries; existing forward state machine does the same).
        let anchor_pos = self.anchor_span(span_id).map(|s| s.anchor);
        if let Some((arx, ary)) = anchor_pos {
            let new_anchor_state = self.cell(arx, ary).map(|c| c.damage_state);
            if let (Some(state), Some(span)) =
                (new_anchor_state, self.anchor_span_mut(span_id))
            {
                span.damage_state = state;
            }
        }
    }

    outcome
}
```

**Note:** Task 3 assumes `BridgeRuntimeState::anchor_span_mut(span_id)
-> Option<&mut AnchorSpan>` exists. If not, add it as a one-line helper
beside the existing `anchor_span` method. Check first via:
```
grep -n "anchor_span_mut\|fn anchor_span" src/sim/bridge_state/mod.rs
```
If `anchor_span_mut` is missing, add it before Task 3's function body
compiles.

**Step 2: Verify compile**
Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS.

**Step 3: Commit**
Message: `sim/bridge_state: add body_cell_repair_state reverse state machine`

---

### Task 4: Unit tests for `body_cell_repair_state`

**Why:** Lock the 10 behavioral invariants from the design's Testing
Strategy section, including the critical iteration-order pin that
protects RNG-draw determinism.

**Files:**
- Modify: `src/sim/bridge_state/mod.rs` (add `#[cfg(test)] mod
  repair_tests` inside the file)

**Pattern:** Mirror existing `body_cell_advance_state` tests structure
(`#[cfg(test)] mod tests { ... }` with `test_seed_cell` /
`test_seed_anchor_span` helpers).

**Step 1: Add test module**
```rust
#[cfg(test)]
mod repair_tests {
    use super::*;
    use crate::sim::rng::SimRng;

    /// Build a 1-cell-wide test span at (10,10..=14) along Y (NS bridge).
    /// Returns a fresh BridgeRuntimeState with one AnchorSpan and 5 body
    /// cells seeded to `state`.
    fn build_single_ns_span(state: DamageState) -> BridgeRuntimeState {
        let mut bs = BridgeRuntimeState::default();
        // 5-cell anchor span: anchor=(10,10), slots 1..=3 = (10,11), (10,12), (10,13);
        //                     slot 4 = (10,9); slot 5 = None.
        let span = AnchorSpan {
            id: 1,
            anchor: (10, 10),
            cells: [Some((10, 10)), Some((10, 11)), Some((10, 12)),
                    Some((10, 13)), Some((10, 9)), None],
            axis: Axis::NS,
            direction: Direction::S,
            damage_state: state,
            bridge_group_id: 1,
        };
        bs.test_seed_anchor_span(span);

        // Seed all 5 body cells with the given state.
        for &(rx, ry) in &[(10, 9), (10, 10), (10, 11), (10, 12), (10, 13)] {
            let role = if (rx, ry) == (10, 10) { BridgeCellRole::Anchor } else { BridgeCellRole::Body };
            bs.test_seed_cell(rx, ry, BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 0,
                bridge_group_id: Some(1),
                damage_state: state,
                axis: Some(Axis::NS),
                role,
                anchor_span_id: Some(1),
                overlay_byte: 0,
                damaged_variant: false,
            });
        }
        bs
    }

    fn seeded_rng() -> SimRng { SimRng::new_seeded(0x4242_4242_4242_4242) }

    #[test]
    fn repair_destroyed_main_deck_sets_zones_dirty_and_radar() {
        let mut bs = build_single_ns_span(DamageState::Destroyed);
        let mut rng = seeded_rng();
        let scan = vec![(10, 10)];
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        assert!(outcome.zones_dirty, "main-deck repair must set zones_dirty");
        // All 5 destroyed cells in span → all radar-dirty.
        assert_eq!(outcome.radar_cells.len(), 5);
        assert_eq!(outcome.repaired_cells, 5);
        // Every cell is now Healthy.
        for &(rx, ry) in &[(10, 9), (10, 10), (10, 11), (10, 12), (10, 13)] {
            let s = bs.cell(rx, ry).unwrap().damage_state;
            assert!(matches!(s, DamageState::Healthy { .. }),
                "cell ({rx},{ry}) state = {s:?}");
        }
    }

    #[test]
    fn repair_damaged_main_deck_zones_dirty_but_no_radar() {
        let mut bs = build_single_ns_span(DamageState::Damaged);
        let mut rng = seeded_rng();
        let scan = vec![(10, 10)];
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        assert!(outcome.zones_dirty);
        assert!(outcome.radar_cells.is_empty(),
            "Damaged → Healthy does NOT mark radar dirty");
        assert_eq!(outcome.repaired_cells, 5);
    }

    #[test]
    fn repair_bridgehead_no_rng_no_zones_no_radar() {
        // Override role to Bridgehead on all 5 cells (still in the span — Rust's
        // BridgeCellRole determines side-effect gating, not gamemd's overlay band).
        let mut bs = build_single_ns_span(DamageState::Damaged);
        for &(rx, ry) in &[(10, 9), (10, 10), (10, 11), (10, 12), (10, 13)] {
            bs.cell_mut(rx, ry).unwrap().role = BridgeCellRole::Bridgehead;
        }
        let mut rng = seeded_rng();
        let rng_state_before = rng.state();  // assumes SimRng exposes state for tests
        let scan = vec![(10, 10)];
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        assert!(!outcome.zones_dirty, "bridgehead-only repair must NOT set zones_dirty");
        assert!(outcome.radar_cells.is_empty());
        assert_eq!(outcome.repaired_cells, 5);
        // RNG state unchanged: bridgehead repair draws ZERO times.
        assert_eq!(rng.state(), rng_state_before, "bridgehead repair must not draw RNG");
        // All cells now Healthy { variant: 0 } (fixed).
        for &(rx, ry) in &[(10, 9), (10, 10), (10, 11), (10, 12), (10, 13)] {
            assert!(matches!(bs.cell(rx, ry).unwrap().damage_state, DamageState::Healthy { variant: 0 }));
        }
    }

    #[test]
    fn repair_healthy_cell_is_noop() {
        let mut bs = build_single_ns_span(DamageState::Healthy { variant: 3 });
        let mut rng = seeded_rng();
        let rng_state_before = rng.state();
        let scan = vec![(10, 10)];
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        assert!(!outcome.zones_dirty);
        assert!(outcome.radar_cells.is_empty());
        assert_eq!(outcome.repaired_cells, 0);
        assert_eq!(rng.state(), rng_state_before, "healthy cells must not draw RNG");
        // Cells unchanged.
        assert!(matches!(bs.cell(10, 10).unwrap().damage_state, DamageState::Healthy { variant: 3 }));
    }

    #[test]
    fn repair_partial_collapse_to_healthy() {
        let mut bs = build_single_ns_span(DamageState::PartialCollapseA);
        let mut rng = seeded_rng();
        let scan = vec![(10, 10)];
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        assert!(outcome.zones_dirty);
        assert!(outcome.radar_cells.is_empty(), "PartialCollapse → Healthy does NOT mark radar dirty");
        assert_eq!(outcome.repaired_cells, 5);
    }

    #[test]
    fn repair_no_bridge_in_scan_empty_outcome() {
        let mut bs = BridgeRuntimeState::default();
        // No anchor span seeded; scan finds nothing.
        let mut rng = seeded_rng();
        let rng_state_before = rng.state();
        let scan: Vec<(u16, u16)> = (0..25).map(|i| (i, 0)).collect();
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        assert!(!outcome.zones_dirty);
        assert!(outcome.radar_cells.is_empty());
        assert_eq!(outcome.repaired_cells, 0);
        assert_eq!(rng.state(), rng_state_before);
    }

    #[test]
    fn repair_determinism_same_seed_same_variants() {
        let mut bs_a = build_single_ns_span(DamageState::Destroyed);
        let mut bs_b = build_single_ns_span(DamageState::Destroyed);
        let mut rng_a = seeded_rng();
        let mut rng_b = seeded_rng();
        let scan = vec![(10, 10)];
        bs_a.body_cell_repair_state(&scan, &mut rng_a);
        bs_b.body_cell_repair_state(&scan, &mut rng_b);
        // Variants byte-equal across two runs.
        for &(rx, ry) in &[(10, 9), (10, 10), (10, 11), (10, 12), (10, 13)] {
            let va = bs_a.cell(rx, ry).unwrap().damage_state;
            let vb = bs_b.cell(rx, ry).unwrap().damage_state;
            assert_eq!(va, vb, "variant divergence at ({rx},{ry})");
        }
    }

    #[test]
    fn repair_strip_iteration_order_pin() {
        // Locks the RNG-draw sequence for a known 5-cell destroyed span.
        // If anyone reorders AnchorSpan.cells or changes the iteration
        // pattern, this test fails with diff-friendly output.
        let mut bs = build_single_ns_span(DamageState::Destroyed);
        let mut rng = seeded_rng();
        let scan = vec![(10, 10)];
        bs.body_cell_repair_state(&scan, &mut rng);

        // Capture variants in slot order from the span definition:
        //   slot 0 = (10,10) anchor
        //   slot 1 = (10,11), slot 2 = (10,12), slot 3 = (10,13)
        //   slot 4 = (10,9), slot 5 = None
        let variants: Vec<u8> = [(10,10),(10,11),(10,12),(10,13),(10,9)]
            .iter()
            .map(|&(rx, ry)| match bs.cell(rx, ry).unwrap().damage_state {
                DamageState::Healthy { variant } => variant,
                other => panic!("non-Healthy after repair: {other:?}"),
            })
            .collect();

        // Pinned reference. Run the test once; copy the output here.
        // If this test fails after a future change, verify the RNG-call
        // order shift is intentional before updating the pin.
        // EXPECTED_VARIANTS: filled in after first run; treat as "do not
        // edit without understanding the change".
        let expected = compute_pinned_variants(&mut seeded_rng());
        assert_eq!(variants, expected,
            "RNG-draw iteration order changed — verify span.cells slot order");

        // Defensive: every stored variant MUST be in 0..=3 (healthy range).
        // Variants 4/5 are reserved for NS DamageA/B encoding (see
        // app_instances/bridges.rs:71-75) and would render as damaged.
        for v in &variants {
            assert!(*v <= 3,
                "repair walker wrote variant {v} — must be 0..=3 (healthy)");
        }
    }

    /// Re-derive the pinned variants by replaying the exact RNG draw
    /// sequence: 5 cells, all main-deck damaged → 5 sequential
    /// `next_range_u32(4)` calls (matches `body_cell_repair_state`'s draw).
    fn compute_pinned_variants(rng: &mut SimRng) -> Vec<u8> {
        (0..5).map(|_| rng.next_range_u32(4) as u8).collect()
    }

    #[test]
    fn repair_two_overlapping_spans_processed_in_btreeset_order() {
        // Span 1 at (10..=14, 10); span 2 at (10..=14, 11). Scan covers both.
        // Verify both spans get processed and outcome aggregates correctly.
        let mut bs = build_single_ns_span(DamageState::Destroyed);
        // Add a second span.
        let span2 = AnchorSpan {
            id: 2,
            anchor: (10, 11),
            cells: [Some((10, 11)), Some((11, 11)), Some((12, 11)),
                    Some((13, 11)), Some((9, 11)), None],
            axis: Axis::NS,
            direction: Direction::S,
            damage_state: DamageState::Destroyed,
            bridge_group_id: 1,
        };
        bs.test_seed_anchor_span(span2);
        for &(rx, ry) in &[(9,11),(10,11),(11,11),(12,11),(13,11)] {
            bs.test_seed_cell(rx, ry, BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 0,
                bridge_group_id: Some(1),
                damage_state: DamageState::Destroyed,
                axis: Some(Axis::NS),
                role: if (rx, ry) == (10, 11) { BridgeCellRole::Anchor } else { BridgeCellRole::Body },
                anchor_span_id: Some(2),
                overlay_byte: 0,
                damaged_variant: false,
            });
        }
        let mut rng = seeded_rng();
        let scan: Vec<(u16, u16)> = cells_in_5x5_scan((10, 10)).collect();
        let outcome = bs.body_cell_repair_state(&scan, &mut rng);
        // Two spans × 5 cells each = 10 repaired (but (10,10) shared if overlap; here disjoint).
        // Span 1: 5 cells; Span 2: 5 cells; Note: (10,11) is in BOTH cell lists,
        // but the Rust cell mutation is keyed on (rx,ry), not span — the second
        // span's iteration will see (10,11) already Healthy from span 1 and skip.
        // So total repaired = 5 + 4 = 9 (span 2's slot 0 is now Healthy).
        // EXPECTED: 9 (verify in test output).
        assert!(outcome.zones_dirty);
        assert_eq!(outcome.repaired_cells, 9,
            "overlap cell (10,11) repaired once by span 1, skipped by span 2");
    }
}
```

**Note:** If `SimRng::state()` and `SimRng::new_seeded()` don't expose the
exact API used here, adjust the test fixture to use whatever the existing
RNG type provides for seed-control and state inspection. Existing usage
elsewhere should provide the pattern; check `sim/rng.rs` first.

**Step 2: First test run (expect the pin test to compute its expected on first call)**
Run: `cargo test --lib bridge_state::repair_tests -- --nocapture`
Expected: All tests PASS. The pin test compares actual variants against
`compute_pinned_variants` re-derived from same seed — should match on
first run.

**Step 3: Commit**
Message: `sim/bridge_state: unit tests for body_cell_repair_state (10 cases incl. iteration-order pin)`

---

### Task 5: Add `SimSoundEvent::BridgeRepaired` variant

**Why:** Sim emits this when a bridge is repaired; app layer translates
to spatial sound + EVA. Sim-side variant must exist before
`tick_bridge_repair_orders` (Task 8) can push it.

**Files:**
- Modify: `src/sim/world/mod.rs` (add variant in the `SimSoundEvent`
  enum around line 95–169)

**Pattern:** Mirror `SimSoundEvent::BuildingComplete` (which carries
`owner: InternedId`) and `SimSoundEvent::C4Planted` (which carries
positional `rx, ry`). New variant combines both.

**Step 1: Add variant**
Add this variant inside the `pub enum SimSoundEvent` block in
`src/sim/world/mod.rs`, immediately after the `C4Planted` variant
at line 168:
```rust
    /// An engineer entered a `BridgeRepairHut` and triggered bridge
    /// repair. Played at the BUILDING's cell, NOT the engineer's.
    /// `owner` is the engineer's house — app layer plays
    /// `EVA_BridgeRepaired` only if `owner` is the local human player.
    /// App layer plays the spatial `[BridgeRepaired]` sound for everyone
    /// in range, gated on `rules.bridge_rules.repair_sound.is_some()`.
    /// Source: BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md §3.1 steps A+B.
    BridgeRepaired { rx: u16, ry: u16, owner: InternedId },
```

**Step 2: Verify compile**
Run: `cargo check -p ra2-rust-game --lib`
Expected: ERROR — exhaustive match in `app_sim_tick.rs:496` doesn't
cover the new variant. **This is expected**; Task 7 adds the app match arm.

**Step 3: Commit**
Message: `sim/world: add SimSoundEvent::BridgeRepaired variant`

---

### Task 6: Add `body_cell_repair_state` smoke-test through the sim

**Why:** Quick mid-plan integration sanity-check that the new state
machine round-trips through the public bridge_state API and the
state-hash. Catches any borrow-checker / API-shape issues BEFORE we
wire the trigger.

**Files:**
- Modify: `src/sim/world/world_hash.rs` (no changes needed — verify the
  field is already hashed via existing `BridgeRuntimeCell.damage_state`)
- Run a one-shot manual test from the existing bridge_state test infrastructure

**Step 1: Verify state hash includes damage_state**
Run:
```
grep -n "damage_state" src/sim/world/world_hash.rs src/sim/bridge_state/mod.rs
```
Expected: `damage_state` appears in the hash via `BridgeRuntimeCell`'s
field iteration (struct fields hashed in declaration order). Confirm
by reading 5 lines around any hit in `world_hash.rs`.

If `damage_state` is NOT in the hash path, add it. Pattern: mirror how
other `BridgeRuntimeCell` fields are hashed.

**Step 2: No code change expected; document finding**
If state-hash coverage is already present (likely — forward state
machine writes the same field), no code change. Add a one-liner in
this plan's `Open Questions` section if anything unexpected surfaces.

**Step 3: Commit (no-op if no change)**
Skip commit if no file modified. Otherwise:
Message: `sim/world: ensure BridgeRuntimeCell.damage_state included in state hash`

---

### Task 7: Wire app-layer dispatch for `SimSoundEvent::BridgeRepaired`

**Why:** Without this, sim emits the event but no sound/EVA fires.
Required to compile (exhaustive match). Defined behavior per the
parity ledger items "sound at building location", "EVA local-human",
"sound gated on `repair_sound.is_some()`".

**Files:**
- Modify: `src/app_sim_tick.rs` (add match arm around line 496, after
  the `SimSoundEvent::C4Planted` arm)

**Pattern:** Mirror `SimSoundEvent::C4Planted` arm at line 496–502.
For EVA, mirror whatever existing arm emits EVA for an owner-gated
event (check `BuildingComplete` if it has app-side EVA handling).

**Step 1: Locate the GameSoundEvent definition**
Run:
```
grep -n "pub enum GameSoundEvent\|enum GameSoundEvent" src/app_sim_tick.rs src/app_audio.rs src/app_*.rs
```
Identify the file owning `GameSoundEvent`. Add a `BridgeRepaired` variant
there following the pattern of `GameSoundEvent::C4Planted` (line 498
shows the existing shape: `{ sound_id: String, screen_pos: Option<(i32, i32)> }`).

**Step 2: Add `GameSoundEvent::BridgeRepaired` variant**
```rust
    /// Played at the building's cell when a bridge repair fires.
    /// `sound_id` is the resolved `[BridgeRepaired]` sound name from
    /// `rules.bridge_rules.repair_sound`. `screen_pos` is None if the
    /// repair_sound was unset (no audio dispatch).
    /// `play_eva` is true when the engineer's house matches the local
    /// human player — app layer plays EVA_BridgeRepaired.
    BridgeRepaired {
        sound_id: String,
        screen_pos: Option<(i32, i32)>,
        play_eva: bool,
    },
```

**Step 3: Add the match arm in `app_sim_tick.rs` around line 502**
Insert after the existing `SimSoundEvent::C4Planted` arm (line 496–502):
```rust
                    SimSoundEvent::BridgeRepaired { rx, ry, owner } => {
                        // Resolve repair_sound from rules; if unset, no spatial
                        // sound — RE §3.1 step B gate (`RulesClass+0x248 != -1`).
                        let sound_id = state
                            .rules
                            .bridge_rules
                            .repair_sound
                            .clone()
                            .unwrap_or_default();
                        let screen_pos = if sound_id.is_empty() {
                            None
                        } else {
                            Some(crate::map::terrain::iso_to_screen(rx, ry, 0))
                        };
                        // EVA gate: local-human owner only — RE §3.1 step A.
                        let play_eva = owner == state.local_player_id;  // adjust to whatever the existing local-player accessor is
                        GameSoundEvent::BridgeRepaired { sound_id, screen_pos, play_eva }
                    }
```

**Note:** `state.local_player_id` is a placeholder for whatever app
state exposes the local human's `InternedId`. Check the existing
`SimSoundEvent::BuildingComplete` or similar local-human-gated arm for
the correct accessor. If none of the existing arms gate on local-human
at the sim→app layer, the gating may happen later inside the EVA
dispatcher — in that case, set `play_eva: true` unconditionally and let
the dispatcher gate.

**Step 4: Add EVA dispatch in the app audio layer**
Find where `GameSoundEvent::BuildingComplete` (or similar EVA event)
is consumed by the audio system. Add a parallel handler for
`GameSoundEvent::BridgeRepaired`: play `EVA_BridgeRepaired` if
`play_eva` is true; play `sound_id` at `screen_pos` if some.

If the audio layer doesn't yet have an EVA dispatch path, this task
adds a TODO comment and `play_eva` becomes a no-op slot for a future
EVA-system task. Document this clearly.

**Step 5: Verify compile**
Run: `cargo check -p ra2-rust-game --bin ra2-rust-game`
Expected: PASS (exhaustive match satisfied).

**Step 6: Commit**
Message: `app: wire BridgeRepaired sound + EVA dispatch on sim event`

---

### Task 8: Add `tick_bridge_repair_orders` to Simulation

**Why:** The trigger function. Iterates engineers pointing at CABHUTs;
on adjacency, fires EVA + sound + state mutation + despawn.

**Files:**
- Modify: `src/sim/world/world_orders.rs` (add new method after
  `tick_capture_orders`, around line 209)

**Pattern:** Mirror `tick_capture_orders` structurally:
1. Snapshot `(engineer_id, target_id, owner)` of engineers with
   `capture_target.is_some()` and matching condition.
2. Iterate per-engineer with adjacency check.
3. Fire side effects in order.

**Step 1: Add the function**
```rust
    /// Tick bridge-repair orders: any engineer with `capture_target`
    /// pointing at a `BridgeRepairHut=yes` building, Chebyshev-≤-1
    /// adjacent, triggers bridge repair on the bridge cells in a 5×5
    /// scan around the engineer.
    ///
    /// Per RE §3.1 + §12, the gamemd flow is:
    ///   1. Play EVA + RepairBridgeSound (local-human / RulesClass+0x248 gates)
    ///   2. 5×5 scan finds bridge cells; dispatch low-or-high
    ///   3. Walker mutates each main-deck cell to Healthy{variant}
    ///   4. Listener callback registry (NOT YET implemented in Rust — RE §19 Q6)
    ///   5. Engineer is consumed (Limbo/Destroy)
    ///
    /// Returns `true` if any repair fired (caller ORs into
    /// `TickResult.bridge_state_changed`).
    pub(crate) fn tick_bridge_repair_orders(&mut self, rules: &RuleSet) -> bool {
        use crate::sim::bridge_state::cells_in_5x5_scan;

        // Snapshot eligible engineers.
        let candidates: Vec<(u64, u64, crate::sim::intern::InternedId)> = self
            .entities
            .keys_sorted()
            .into_iter()
            .filter_map(|sid| {
                let e = self.entities.get(sid)?;
                if e.dying || e.capture_target.is_none() {
                    return None;
                }
                Some((sid, e.capture_target.unwrap(), e.owner))
            })
            .collect();

        let mut any_repair = false;

        for (engineer_id, building_id, engineer_owner) in candidates {
            // Resolve target type; must be BridgeRepairHut.
            let target_bridge_hut = self
                .entities
                .get(building_id)
                .and_then(|b| {
                    let type_str = self.interner.resolve(b.type_ref).to_string();
                    rules.object(&type_str).map(|t| t.bridge_repair_hut)
                })
                .unwrap_or(false);
            if !target_bridge_hut {
                continue;  // Not our target — tick_capture_orders handles it
            }

            // Target alive + still a Structure.
            let target_alive = self
                .entities
                .get(building_id)
                .is_some_and(|b| b.category == EntityCategory::Structure && !b.dying);
            if !target_alive {
                if let Some(e) = self.entities.get_mut(engineer_id) {
                    e.capture_target = None;
                }
                continue;
            }

            // Chebyshev-≤-1 adjacency (parity drift documented at
            // tick_capture_orders:264-266: engineer stands NEXT to building
            // because pathing treats footprint as blocked).
            let eng_cell = self.entities.get(engineer_id).map(|e| (e.position.rx, e.position.ry));
            let bld_cell = self.entities.get(building_id).map(|b| (b.position.rx, b.position.ry));
            let Some((erx, ery)) = eng_cell else { continue };
            let Some((brx, bry)) = bld_cell else { continue };
            let dx = (erx as i32 - brx as i32).abs();
            let dy = (ery as i32 - bry as i32).abs();
            if dx > 1 || dy > 1 {
                continue;  // walk-up still in progress
            }

            // ---- Trigger fires this tick ----

            // Step A: emit SimSoundEvent::BridgeRepaired (RE §3.1 step A+B).
            // Sound plays at the BUILDING's cell (not engineer's).
            self.sound_events.push(SimSoundEvent::BridgeRepaired {
                rx: brx,
                ry: bry,
                owner: engineer_owner,
            });

            // Step B: 5×5 scan from engineer cell + repair dispatch.
            let scan: Vec<(u16, u16)> = cells_in_5x5_scan((erx, ery)).collect();
            let outcome = self
                .bridge_state
                .as_mut()
                .map(|bs| bs.body_cell_repair_state(&scan, &mut self.rng))
                .unwrap_or_default();

            if outcome.zones_dirty || outcome.repaired_cells > 0 {
                any_repair = true;
            }

            // Step C: propagate radar-dirty cells (existing render
            // dirty-cell propagation path; reuse the same call as
            // forward damage if available).
            for cell in &outcome.radar_cells {
                // Adjust to the project's radar-dirty API; if no such API
                // exists yet, leave a TODO comment referencing the
                // BRIDGE_REPAIR_AND_HUT_DEATH report §12.7 and skip.
                let _ = cell;
                // TODO: self.render_dirty.mark_radar_cell(*cell);
            }

            // Step D: engineer consumed (RE §3.1 step G).
            self.despawn_entity(engineer_id);
        }

        any_repair
    }
```

**Note on adjacency check direction:** the scan center is the
ENGINEER's cell, matching gamemd's `PerCellProcess` (the scan is
relative to the engineer who just arrived). The sound is played at
the BUILDING's cell. These two coords are different (engineer is
adjacent, not on, the building).

**Step 2: Verify compile**
Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS.

**Step 3: Commit**
Message: `sim/world: add tick_bridge_repair_orders (no callers yet)`

---

### Task 9: Wire `tick_bridge_repair_orders` into `advance_tick` + capture-skip + signature change

**Why:** Connect the trigger to the per-tick pipeline. Place BEFORE
`tick_capture_orders` so engineer is despawned first. Add explicit
skip in `tick_capture_orders` as defense-in-depth against future
ordering changes.

**Files:**
- Modify: `src/sim/world/mod.rs` line 1204
- Modify: `src/sim/world/world_orders.rs` `tick_capture_orders` signature + body

**Step 1: Add the wire-up in `advance_tick`**
Find the current Phase-5 trigger zone (around `world/mod.rs:1204`):
```rust
            spawned_entities |= self.tick_capture_orders();
            destroyed_structure |= self.tick_c4_plants(rules);
```

Replace with:
```rust
            // tick_bridge_repair_orders runs BEFORE tick_capture_orders so
            // that engineers targeting BridgeRepairHut buildings are
            // consumed by repair, not by capture. tick_capture_orders has
            // an explicit BridgeRepairHut skip as defense in depth.
            let bridge_repaired = self.tick_bridge_repair_orders(rules);
            spawned_entities |= self.tick_capture_orders(rules);
            destroyed_structure |= self.tick_c4_plants(rules);
            bridge_state_changed |= bridge_repaired;
```

**Note:** the wire-up site uses the LOCAL `bridge_state_changed: bool` declared
at world/mod.rs:1003, NOT a `tick_result` struct (which is constructed only at
the end of `advance_tick`, line ~1491). Existing precedent at line ~1247:
`bridge_state_changed |= apply_bridge_damage_events(...)`. The local is folded
into `TickResult` at the function tail.

**Step 2: Add `rules: &RuleSet` parameter to `tick_capture_orders`**
Change signature in `world_orders.rs:151`:
```rust
// Before:
pub(crate) fn tick_capture_orders(&mut self) -> bool {
// After:
pub(crate) fn tick_capture_orders(&mut self, rules: &RuleSet) -> bool {
```

**Step 3: Add the BridgeRepairHut skip in the per-engineer loop**
Inside `tick_capture_orders`, right after the snapshot's `for` loop
unpacking at world_orders.rs:161 (after `let building_ok = ...`
check around line 167), insert:
```rust
            // Defense in depth: tick_bridge_repair_orders runs first and
            // despawns engineers on BridgeRepairHut targets. If for any
            // reason the engineer survived (e.g., not yet adjacent), do
            // NOT treat the click as a capture — gamemd never captures
            // CABHUTs. Skip; the engineer keeps pathing or eventually
            // triggers repair when adjacent.
            let target_bridge_hut = self
                .entities
                .get(building_id)
                .and_then(|b| {
                    let type_str = self.interner.resolve(b.type_ref).to_string();
                    rules.object(&type_str).map(|t| t.bridge_repair_hut)
                })
                .unwrap_or(false);
            if target_bridge_hut {
                continue;
            }
```

**Step 4: Verify compile**
Run: `cargo build -p ra2-rust-game`
Expected: PASS.

**Step 5: Run existing capture/C4/bridge tests to catch regressions**
Run: `cargo test --lib world_orders bridge_state -- --nocapture`
Expected: All previously passing tests still pass.

**Step 6: Commit**
Message: `sim/world: wire tick_bridge_repair_orders into Phase 5 + capture skip`

---

### Task 10: Add `dispatch_bridge_collapse_from_hut` helper

**Why:** Shared destruction-side dispatch reachable from both
`apply_c4_damage_to_building` (Task 11) and the future demo-truck path.
MUST drive the full BlowUpBridge cascade (kill ground occupants, drop-in
bridge-deck entities, debris, rim refresh, zone rebuild) — not just
state-machine progression. Lives in `bridge_orchestrator.rs` to reuse
the existing module-private cascade helpers.

**Files:**
- Modify: `src/sim/world/bridge_orchestrator.rs` (add new `pub(crate)`
  function adjacent to `apply_bridge_damage_events`, reusing
  `kill_ground_occupants_at`, `drop_in_bridge_deck_entities`,
  `spawn_bridge_debris`, `update_adjacent_bridges`,
  `notify_bridge_span_collapse`, `refresh_bridge_zones_if_dirty`)

**Pattern:** Mirror `apply_bridge_damage_events`'s post-dispatch cascade
phase. The hut-death path SKIPS the damage-side state-machine dispatcher
(no warhead, no BridgeStrength gate) — instead it drives every scanned
bridge cell directly to `Destroyed` via `body_cell_advance_state`, then
runs the same cascade with the resulting `StateOutcome::Collapsed`
payloads.

**Step 1: Add the function**
```rust
/// Bridge-collapse dispatch from a CABHUT death event (C4 timer
/// expired, demo-truck explosion). Mirrors gamemd's
/// `DestroyBridge_*_MapInit` flow (BRIDGE_REPAIR_AND_HUT_DEATH §13.2):
///   1. 5×5 scan around `hut_center` finds bridge cells.
///   2. For each cell with `anchor_span_id`, drive forward state
///      machine until `Destroyed` or `NoChange`. Collect every
///      `StateOutcome::Collapsed` for the cascade phase.
///   3. Run the BlowUpBridge cascade exactly as
///      `apply_bridge_damage_events` does:
///      - kill ground occupants at BlowUpBridge cells (C4Warhead semantics)
///      - drop-in bridge-deck entities at destroyed cells
///      - spawn debris (50% MetallicDebris + 1 BridgeExplosion per cell)
///      - rim refresh on `adjacent_bridges_dirty` cells
///      - TriggerEvent 31 broadcast
///      - zone-graph rebuild via `refresh_bridge_zones_if_dirty`
///
/// Returns `true` if any bridge cell transitioned (caller ORs into
/// `bridge_state_changed` so the app rebuilds PathGrid).
///
/// **gamemd parity note** (§13.4): the binary's
/// `DestroyBridge_Low_MapInit` mistakenly calls
/// `UpdateAdjacentBridges_High` (copy-paste bug); our `update_adjacent_bridges`
/// is band-agnostic, so the bug is a no-op in Rust. No behavior to replicate.
pub(crate) fn dispatch_bridge_collapse_from_hut(
    sim: &mut Simulation,
    rules: &RuleSet,
    hut_center: (u16, u16),
) -> bool {
    use crate::sim::bridge_state::{cells_in_5x5_scan, StateOutcome};

    let scan: Vec<(u16, u16)> = cells_in_5x5_scan(hut_center).collect();

    // Phase 1: drive every scanned bridge cell to convergence; collect
    // Collapsed outcomes. Sorted (BTreeSet then deterministic iteration)
    // so cascade order is replay-stable.
    let mut outcomes: Vec<StateOutcome> = Vec::new();
    {
        let Some(bs) = sim.bridge_state.as_mut() else { return false };
        for cell_pos in &scan {
            let has_span = bs
                .cell(cell_pos.0, cell_pos.1)
                .map_or(false, |c| c.anchor_span_id.is_some());
            if !has_span {
                continue;
            }
            // Drive forward state machine to convergence in this tick.
            // Healthy → Damaged → Destroyed = 2 transitions max;
            // PartialCollapse{A,B} → Destroyed = 1 transition.
            loop {
                let outcome = bs.body_cell_advance_state(
                    cell_pos.0,
                    cell_pos.1,
                    /* is_high_bridge */ false,
                );
                match outcome {
                    StateOutcome::NoChange => break,
                    other => outcomes.push(other),
                }
            }
        }
    }

    if outcomes.is_empty() {
        return false;
    }

    // Phase 2: aggregate destroyed cells + BlowUpBridge cells from outcomes
    // (same shape as apply_bridge_damage_events lines 71-91).
    let mut destroyed_set: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut blow_up_cells: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut rim_cells: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut any_zones_dirty = false;
    for outcome in &outcomes {
        if let StateOutcome::Collapsed {
            destroyed_cells,
            set_bridge_direction,
            adjacent_bridges_dirty,
            zones_dirty,
        } = outcome
        {
            destroyed_set.extend(destroyed_cells.iter().copied());
            for (cell, _slot, action) in &set_bridge_direction.actions {
                if matches!(action, crate::sim::bridge_specs::CellAction::BlowUpBridge) {
                    blow_up_cells.insert(*cell);
                    destroyed_set.insert(*cell);
                }
            }
            rim_cells.extend(adjacent_bridges_dirty.iter().copied());
            any_zones_dirty |= *zones_dirty;
        }
    }

    // Phase 3: cascade. Resolve C4Warhead's InfDeath once outside the kill
    // loop so the inner block doesn't hold `&sim.interner` while we need
    // `&mut sim` for the kills. Same shape as apply_bridge_damage_events
    // lines 102-106.
    let c4_inf_death: u8 = {
        let c4_id = rules.c4_warhead_id();
        let name = sim.interner.resolve(c4_id);
        rules.warhead(name).map(|wh| wh.inf_death).unwrap_or(1)
    };
    for &(rx, ry) in &blow_up_cells {
        kill_ground_occupants_at(sim, rx, ry, c4_inf_death);
    }
    for &(rx, ry) in &destroyed_set {
        drop_in_bridge_deck_entities(sim, rx, ry);
    }
    spawn_bridge_debris(sim, rules, &destroyed_set);
    update_adjacent_bridges(sim, &rim_cells);
    notify_bridge_span_collapse(sim, &destroyed_set);
    refresh_bridge_zones_if_dirty(sim, any_zones_dirty);

    !destroyed_set.is_empty()
}
```

**Notes:**
- Function lives in `bridge_orchestrator.rs` (sibling of `world_orders.rs`)
  so it can call the module-private cascade helpers directly. No
  visibility changes needed on the helpers.
- `is_high_bridge: false` is safe per the forward state machine docstring
  at bridge_state/mod.rs:753-755 — state transitions are identical for
  HIGH and LOW.
- The Phase 1 loop scope `{ let Some(bs) = sim.bridge_state.as_mut() ... }`
  ends before Phase 3 to release the `&mut bs` borrow; Phase 3 needs
  `&mut sim` for the cascade helpers.

**Step 2: Verify compile**
Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS.

**Step 3: Commit**
Message: `sim/world/bridge_orchestrator: add dispatch_bridge_collapse_from_hut with cascade`

---

### Task 11: Branch in `apply_c4_damage_to_building` for BridgeRepairHut targets

**Why:** Wires the destruction-side trigger. C4 detonating on a CABHUT
now causes bridge collapse instead of damaging the hut. Demo-truck
path is a TODO with the same call.

Must propagate `bridge_state_changed` to the app so PathGrid rebuilds
after the collapse. `apply_c4_damage_to_building` currently returns a
single `bool` (killed_building); we widen to a struct so the bridge
signal travels alongside.

**Files:**
- Modify: `src/sim/world/world_orders.rs`:
  - `apply_c4_damage_to_building`: return type `bool` → `C4DamageOutcome`
  - `tick_c4_plants`: return type `bool` → `C4TickOutcome`; collect both
    flags across the per-plant loop
  - Define both small structs at top of file (or adjacent to
    `apply_c4_damage_to_building`)
- Modify: `src/sim/world/mod.rs` line 1206: destructure the new
  `C4TickOutcome` and OR both flags into the local accumulators
  (`destroyed_structure`, `bridge_state_changed`)

**Pattern:** Mirror how `apply_bridge_damage_events` returns its
"state changed" signal that the caller ORs into `bridge_state_changed`
at world/mod.rs:1247.

**Step 1: Define outcome structs**
At an appropriate location in `world_orders.rs` (top of impl block or
free-function area):
```rust
/// Result of one `apply_c4_damage_to_building` call.
pub(crate) struct C4DamageOutcome {
    /// HP reached 0; building marked dying this tick.
    pub killed_building: bool,
    /// The C4 hit a BridgeRepairHut, the hut survived, and the connected
    /// bridge collapsed. PathGrid needs rebuild.
    pub bridge_state_changed: bool,
}

/// Result of `tick_c4_plants` across all per-tick plants + detonations.
pub(crate) struct C4TickOutcome {
    pub destroyed_structure: bool,
    pub bridge_state_changed: bool,
}
```

**Step 2: Change `apply_c4_damage_to_building` return type**
Update the signature at world_orders.rs:469:
```rust
// Before:
fn apply_c4_damage_to_building(
    &mut self,
    building_id: u64,
    damage: u16,
    warhead_id: crate::sim::intern::InternedId,
    attacker_id: Option<u64>,
    rules: &RuleSet,
) -> bool {
// After:
fn apply_c4_damage_to_building(
    &mut self,
    building_id: u64,
    damage: u16,
    warhead_id: crate::sim::intern::InternedId,
    attacker_id: Option<u64>,
    rules: &RuleSet,
) -> C4DamageOutcome {
```

Update existing `return false` / `return true` sites in the function
to return `C4DamageOutcome { killed_building: ..., bridge_state_changed: false }`.
Existing terminal:
```rust
// Before:
true / false at lines 487, 493, 503, 509, 518, 520 (rough — verify
positions when editing)
// After:
C4DamageOutcome { killed_building: false, bridge_state_changed: false }
or
C4DamageOutcome { killed_building: true, bridge_state_changed: false }
```

**Step 3: Add the BridgeRepairHut branch**
Insert AFTER the IronCurtain invulnerability check (after the
`return false` if invulnerable, before the warhead-resolution block):
```rust
        // BridgeRepairHut target: skip damaging the hut and trigger
        // bridge collapse instead. Mirrors gamemd
        // BuildingClass::Update's Type[0x16B6] branch (RE §3.2) which
        // skips vtable[0x16C] (damage application) and dispatches
        // DestroyBridge_*_MapInit. The hut survives the explosion.
        //
        // NOTE: in vanilla YR, this code path is unreachable because
        // CABHUT's `Immune=yes` is enforced upstream of PerCellProcess
        // (RE §15.2; tracked as `project_c4_bridge_hut_followup`). When
        // that fix lands, this branch becomes live.
        //
        // DEMO TRUCK TODO: when the demo-truck unit is implemented, its
        // damage path should call
        // `bridge_orchestrator::dispatch_bridge_collapse_from_hut` directly
        // (BombClass::Detonate bypasses field_0x6DF per RE §3.7).
        let target_bridge_hut = self
            .entities
            .get(building_id)
            .and_then(|b| {
                rules
                    .object(self.interner.resolve(b.type_ref))
                    .map(|t| t.bridge_repair_hut)
            })
            .unwrap_or(false);
        if target_bridge_hut {
            let bld_center = self
                .entities
                .get(building_id)
                .map(|b| (b.position.rx, b.position.ry));
            let bridge_state_changed = match bld_center {
                Some(center) => {
                    crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut(
                        self, rules, center,
                    )
                }
                None => false,
            };
            let _ = attacker_id; // hut survives — no `last_attacker_id` update
            return C4DamageOutcome {
                killed_building: false,
                bridge_state_changed,
            };
        }
```

**Step 4: Update `tick_c4_plants` return shape**
At world_orders.rs:228, change signature and aggregation:
```rust
// Before:
pub(crate) fn tick_c4_plants(&mut self, rules: &RuleSet) -> bool {
    ...
    let mut destroyed_structure = false;
    ...
    destroyed_structure |= self.apply_c4_damage_to_building(...);
    ...
    destroyed_structure
}
// After:
pub(crate) fn tick_c4_plants(&mut self, rules: &RuleSet) -> C4TickOutcome {
    ...
    let mut destroyed_structure = false;
    let mut bridge_state_changed = false;
    ...
    let dmg = self.apply_c4_damage_to_building(...);
    destroyed_structure |= dmg.killed_building;
    bridge_state_changed |= dmg.bridge_state_changed;
    ...
    C4TickOutcome { destroyed_structure, bridge_state_changed }
}
```

**Step 5: Update the call site in `advance_tick`**
At world/mod.rs:1206 (now adjacent to Task 9's `bridge_repaired` wire):
```rust
// Before (from Task 9):
let bridge_repaired = self.tick_bridge_repair_orders(rules);
spawned_entities |= self.tick_capture_orders(rules);
destroyed_structure |= self.tick_c4_plants(rules);
bridge_state_changed |= bridge_repaired;
// After:
let bridge_repaired = self.tick_bridge_repair_orders(rules);
spawned_entities |= self.tick_capture_orders(rules);
let c4_outcome = self.tick_c4_plants(rules);
destroyed_structure |= c4_outcome.destroyed_structure;
bridge_state_changed |= bridge_repaired | c4_outcome.bridge_state_changed;
```

**Step 6: Verify compile**
Run: `cargo build -p ra2-rust-game`
Expected: PASS. (Existing `tick_c4_plants` tests may need adjustment if
they assert on the `bool` return — update to assert on
`outcome.destroyed_structure`.)

**Step 7: Commit**
Message: `sim/world: route bridge_state_changed through C4 hut-collapse path`

---

### Task 12: Integration tests — engineer repair + C4 destroy (ignored)

**Why:** Lock end-to-end behavior. Engineer-repair tests are reachable
in vanilla YR; C4-on-CABHUT test is `#[ignore]`'d pending the upstream
Immune fix.

**Files:**
- Create or modify (depending on existing test file conventions):
  `src/sim/world/world_orders_tests.rs` (or similar — check
  `src/sim/world/tests/` if it exists; place tests alongside existing
  `tick_capture_orders` / `tick_c4_plants` tests for cohesion)

**Pattern:** Mirror the existing C4-plant integration test
(`a52603e sim/world: integration tests for C4 plant lifecycle +
determinism`).

**Step 1: Locate the existing integration test file**
Run:
```
grep -rln "tick_capture_orders\|tick_c4_plants" src/sim/world/ --include="*tests*"
```

**Step 2: Add these tests**
```rust
#[test]
fn engineer_enters_cabhut_repairs_bridge() {
    // Setup: 1 CABHUT building + 1 engineer adjacent + 1 destroyed bridge span.
    // Tick once. Assert:
    //   - engineer despawned (not in entities)
    //   - bridge cell damage_state == Healthy
    //   - SimSoundEvent::BridgeRepaired present in sound_events
    //   - TickResult.bridge_state_changed == true
    //   - is_bridge_walkable(rx,ry) == true for the repaired cells
    let mut sim = build_test_simulation_with_destroyed_bridge_and_engineer();
    let rules = test_rules_with_repair_sound();
    let tick_result = sim.advance_tick_for_test(&rules);

    assert!(tick_result.bridge_state_changed);
    assert!(sim.entities.get(test_engineer_id).is_none(),
        "engineer must be despawned after repair");
    // Bridge cells now Healthy with variants in 0..=3 (healthy SHP frames).
    // Variants 4/5 would render as damage-progression frames per
    // app_instances/bridges.rs:71-75 — regression guard for Issue #2.
    let bs = sim.bridge_state.as_ref().unwrap();
    for (rx, ry) in test_bridge_cells() {
        let state = bs.cell(rx, ry).unwrap().damage_state;
        match state {
            DamageState::Healthy { variant } => assert!(variant <= 3,
                "cell ({rx},{ry}) variant={variant} — must be 0..=3 (healthy)"),
            other => panic!("cell ({rx},{ry}) = {other:?} (expected Healthy)"),
        }
        assert!(bs.is_bridge_walkable(rx, ry));
    }
    // Sound event emitted.
    assert!(sim.sound_events.iter().any(|e| matches!(e,
        SimSoundEvent::BridgeRepaired { .. })));
}

#[test]
fn engineer_at_intact_cabhut_emits_sound_no_mutation() {
    let mut sim = build_test_simulation_with_intact_bridge_and_engineer();
    let rules = test_rules_with_repair_sound();
    let tick_result = sim.advance_tick_for_test(&rules);

    // Engineer consumed; sound emitted; but no bridge change.
    assert!(sim.entities.get(test_engineer_id).is_none());
    assert!(sim.sound_events.iter().any(|e| matches!(e,
        SimSoundEvent::BridgeRepaired { .. })));
    assert!(!tick_result.bridge_state_changed,
        "intact bridge: no zone rebuild");
}

#[test]
fn two_engineers_both_repair_same_tick() {
    let mut sim = build_test_simulation_with_destroyed_bridge_and_two_engineers();
    let rules = test_rules_with_repair_sound();
    sim.advance_tick_for_test(&rules);

    // Both engineers despawned.
    assert!(sim.entities.get(test_engineer_a).is_none());
    assert!(sim.entities.get(test_engineer_b).is_none());
    // Two sound events emitted.
    let repair_events: Vec<_> = sim.sound_events.iter()
        .filter(|e| matches!(e, SimSoundEvent::BridgeRepaired { .. }))
        .collect();
    assert_eq!(repair_events.len(), 2);
    // Bridge fully repaired (idempotent — second engineer is a no-op
    // because all cells are Healthy after first dispatch).
    let bs = sim.bridge_state.as_ref().unwrap();
    for (rx, ry) in test_bridge_cells() {
        assert!(matches!(bs.cell(rx, ry).unwrap().damage_state,
            DamageState::Healthy { .. }));
    }
}

#[test]
fn engineer_far_from_bridge_at_cabhut_no_mutation() {
    // CABHUT placed in middle of nowhere; engineer arrives.
    let mut sim = build_test_simulation_with_cabhut_no_bridge_nearby();
    let rules = test_rules_with_repair_sound();
    sim.advance_tick_for_test(&rules);

    // Engineer despawned; sound emitted; zero bridge mutation; no panic.
    assert!(sim.entities.get(test_engineer_id).is_none());
    assert!(sim.sound_events.iter().any(|e| matches!(e,
        SimSoundEvent::BridgeRepaired { .. })));
}

#[test]
#[ignore = "blocked on project_c4_bridge_hut_followup — upstream Immune gate"]
fn c4_on_cabhut_destroys_bridge_when_upstream_immune_lifted() {
    // SETUP NOTE: this test requires the upstream Immune-gate fix to land
    // before C4 placement on CABHUT is reachable. Until then, the C4 plant
    // path rejects the target before reaching apply_c4_damage_to_building.
    //
    // Test shape (when unblocked):
    //   1. Spawn SEAL/Tanya with c4_plant.target_building_id = CABHUT.
    //   2. Place a ground infantry on a bridge cell adjacent to the CABHUT.
    //   3. Place a tank ON the bridge deck (OnBridge=true).
    //   4. Run tick_c4_plants for c4_delay_ticks ticks.
    //   5. Assert: CABHUT entity still alive (hut survives, RE §3.2).
    //   6. Assert: adjacent bridge cells now Destroyed.
    //   7. Assert: TickResult.bridge_state_changed == true (Issue #4 guard
    //      — without this, PathGrid wouldn't rebuild after collapse).
    //   8. Assert: the ground infantry on a BlowUpBridge cell is dying or
    //      despawned (Issue #3 guard — kill_ground_occupants_at fired).
    //   9. Assert: the deck tank is at ground level with OnBridge=false
    //      (Issue #3 guard — drop_in_bridge_deck_entities fired).
    //  10. Assert: zone_grid was rebuilt (Issue #3 guard — zones cascade
    //      fired, e.g., check via a known unreachability query that
    //      previously succeeded).
    let _placeholder = ();
}
```

**Step 3: Implement the `build_test_simulation_*` helpers**
Match existing patterns from the C4-plant integration test
(commit a52603e). Likely uses `Simulation::default()` + manual entity
seeding + `bridge_state.test_seed_*` from Task 3.

**Step 4: Run integration tests**
Run: `cargo test --lib world_orders -- --include-ignored`
Expected: 4 non-ignored tests PASS, 1 ignored test compiled but not run.

**Step 5: Commit**
Message: `sim/world: integration tests for engineer bridge repair (+ ignored C4 hook test)`

---

### Task 13: Full regression run + final cleanup commit

**Why:** Verify nothing else broke. Address any test failures that
surface across the full suite. Final commit pulls in any small
docstring updates discovered along the way.

**Step 1: Run full test suite**
Run: `cargo test`
Expected: All tests PASS.

**Step 2: Run `cargo clippy`**
Run: `cargo clippy --all-targets`
Expected: No new warnings introduced by this plan's changes.

**Step 3: Run `cargo fmt`**
Run: `cargo fmt`
Expected: No diff (or only the new files reformatted).

**Step 4: Final docstring sweep**
Verify these docstring updates landed somewhere visible:
- `capture_target` on `GameEntity` (game_entity.rs:201): note the
  "engineer-arrival intent; resolution branches on `target.bridge_repair_hut`"
  overload. Add a one-line note to the existing docstring.
- `tick_capture_orders`: comment that BridgeRepairHut is skipped here
  and handled by `tick_bridge_repair_orders`.
- `dispatch_bridge_collapse_from_hut`: reference to RE §13.4 vanilla
  copy-paste bug and `project_c4_bridge_hut_followup` already present.

**Step 5: Commit any remaining diff**
Message: `sim/bridge: docstring sweep for capture_target overload + tick_capture_orders skip`

If no diff, skip this commit.

---

## Sources & References

- **Design doc:** [docs/plans/2026-05-12-bridge-repair-and-hut-death-design.md](2026-05-12-bridge-repair-and-hut-death-design.md)
- **Investigation plan:** [docs/plans/2026-05-12-bridge-repair-system-investigation-plan.md](2026-05-12-bridge-repair-system-investigation-plan.md)
- **Ghidra reports:**
  - [BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md) (Phases 1 + 2, completed 2026-05-12)
  - [BRIDGE_SYSTEM.md](../../../ra2-rust-game-docs/BRIDGE_SYSTEM.md) (corrected for the dispatcher-identity finding inline)
  - [HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md) (forward state machine docs)
  - [MAPCLASS_ZONES_RAMPS_HUT_REGISTRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_ZONES_RAMPS_HUT_REGISTRY_GHIDRA_REPORT.md) (UpdateBridgeZonesHelper internals)
- **gamemd.exe addresses (kept here, NOT in Rust comments):**
  - `0x519630` — InfantryClass::PerCellProcess (engineer-repair trigger)
  - `0x43FB20` — BuildingClass::Update (C4-timer-expired branch)
  - `0x438720` — BombClass::Detonate (demo-truck path)
  - `0x57F200` / `0x57F440` — RepairBridge_Low/High (direction-dispatcher)
  - `0x57F6A0` / `0x57FBC0` / `0x5800D0` / `0x580600` — RepairBridgeWalker_NS/EW_Low/High (overlay state machine)
  - `0x570050` / `0x573540` — ProcessBridgeDestruction_Low/High (engineer-repair entry)
  - `0x574000` / `0x574C20` — DestroyBridge_*_MapInit (hut-destruction entry)
  - `0x598030` — FUN_00598030 (random pick with retry)
  - `0x41BF40` — TechnoClass::IsIronCurtainActive (vtable[0x160], confirmed NOT Immune)
- **INI keys** (already parsed; no changes):
  - `rulesmd.ini [CABHUT] BridgeRepairHut=yes` → `ObjectType.bridge_repair_hut`
  - `rulesmd.ini [AudioVisual] RepairBridgeSound=BridgeRepaired` → `BridgeRules.repair_sound`
- **Related code:**
  - [src/sim/bridge_state/mod.rs](../../src/sim/bridge_state/mod.rs) (forward state machine, anchor spans)
  - [src/sim/world/bridge_orchestrator.rs](../../src/sim/world/bridge_orchestrator.rs) (refresh_bridge_zones_if_dirty)
  - [src/sim/world/world_orders.rs](../../src/sim/world/world_orders.rs) (capture, C4 plant patterns)
  - [src/app_sim_tick.rs:496](../../src/app_sim_tick.rs#L496) (SimSoundEvent dispatch)
- **Project memory:**
  - `project_c4_bridge_hut_followup` (open bug: C4 on CABHUT does nothing in-game; keystone is upstream of PerCellProcess per RE §15.2)
