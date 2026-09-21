# Bridge reachability diagnosis — why bridges do not work for the player

Synthesis of three lanes, all read this session:
`gamemd.md` (binary, outranks on what YR does), `refusal.md` (read-only Rust trace),
`repro.md` (live Bay of Pigs run, OBSERVED data outranks reasoning).
All three lanes present; nothing invented to fill a gap.

Repro lane ran on `bf9a3a0f`, which **already contains** the height fixes `5c67724c` and
`5e1cbe08` (VERIFIED: `git merge-base --is-ancestor 5c67724c bf9a3a0f` → true). Everything the
repro observed is post-height-fix behaviour.

---

## 1. VERDICT

**The root cause is a hierarchy corridor gate that VERA applies to bridge-deck cells and gamemd
does not.** On Bay of Pigs an ordinary `Command::Move` across the span is **refused outright**:
`HierarchyGate::allows` (`src/sim/pathfinding/core.rs:381-386`) is a ground-plane
`zone_at(x, y)` lookup with no layer term, deck cells answer level-0 zone `0`, that zone is never
on the coarse route, so every deck neighbour is rejected at `core.rs:1354-1360` and the A* exhausts
(VERIFIED, repro §1/§3: four cross-bridge orders refused, `No path from (111,134) to (111,152)`;
gate-off ablation returns a 19-node route with all 17 deck cells on `layer=Bridge`).
gamemd exempts exactly this candidate class — `JZ 0x00429F04` at `0x00429EAF` inside
`AStar_main_loop @ 0x00429A90` jumps a bridge-layer neighbour past the corridor test entirely
(VERIFIED, disassembly read in two independent lanes: `gamemd.md` §5.2, `repro.md` §7).

**The repro lane did NOT observe a unit routing under the span, and did not observe a unit falling
off a deck it was driving on. The height model was not the bug for the reported crossing.** The
tank does not move at all (VERIFIED, repro §1: 40 frames, `(111,134)` → `(111,134)`). Under an
ordinary order the deck is simply unreachable; there is no under-span alternative on this map
either, because the ground plane beneath the deck is zone `0` (VERIFIED, repro §2: gate-off search
from a riverbed cell also returns `None`).

The reported wording is real but belongs to two other mechanisms, ranked in §4 — neither is the
reason bridges do not work today.

---

## 2. WHAT THE PLAYER SEES AND WHY

What the player sees **now**, and what "bridges do not work" actually is: you right-click across
the bridge, the unit says "Moving out" — the voice is played client-side at click time, before the
sim decides anything (`src/app/input/context_order.rs:165-174`, VERIFIED by trace) — and the tank
does not move. Not one step. The engine silently drops the order and logs `No path from …`
(`src/sim/movement/movement_commands.rs:580-592`, VERIFIED). A player reads that as the bridge
being broken, or as the unit ignoring them.

The exception the player will have found by accident: clicking **one cell at a time** works. The
unit's own body stamps its eight neighbours in the blocker map, and a stamped cell is the gate's
only escape hatch, so exactly the next deck cell opens and nothing beyond it (VERIFIED, repro §3
sweep: goal `(111,135)` count 1 → path found; every cell past it count 0 → `None`). Nineteen
successive clicks do cross the bridge cleanly.

**"Falls to the bottom" / "through it"** — the literal wording — matches two things, neither of
them today's ordinary crossing:

* **Before `5c67724c` (committed today), a unit that did get onto the deck was dropped to the
  riverbed mid-span.** Deck-to-deck steps re-derived the unit's height from the A* path layer, so a
  planner layer of `Ground` put the body at riverbed height halfway across while it still counted
  as on the bridge (VERIFIED from `5c67724c`'s own commit body, which describes this symptom in the
  user's words). Combined with the one-cell-at-a-time crossing above, this is the most likely thing
  the user actually watched. It is fixed: on `bf9a3a0f` all 17 deck cells report `z=5`,
  `on_bridge=true`, deck occupancy 5, and a clean clear on exit (VERIFIED, repro §4).
* **A unit *placed* on a deck cell is put at riverbed height with `on_bridge` clear.**
  `spawn_object` (`src/sim/world/world_spawn.rs:344`) has no bridge-deck handling at all: spawning
  a Grizzly at `(111,143)` gives `z=1, on_bridge=false` on a cell whose deck is at 5, and the unit
  can then path nowhere (VERIFIED, repro §6). That is literally "at the bottom, under the bridge".
  Bay of Pigs places no units on a deck, so it is not what was seen on this map, but it is real and
  it fires wherever a map or a production/eject path puts an object on a deck cell.

---

## 3. THE MECHANISM GAP

| | gamemd | VERA |
|---|---|---|
| Coarse pre-pass exists? | **Yes.** `Zone_precheck` 0x0042C290, 3 levels, best-first over a zone graph (VERIFIED, `gamemd.md` §3) | Yes. `zone_precheck_flat`, `src/sim/pathfinding/zone_hierarchy.rs:424-461` (VERIFIED) |
| Deck admitted to the coarse graph how? | Explicitly injected bridge edges from `BridgeRecord`s, base layer + all 3 levels; three cell pairs per record (`MapClass__RegisterBridgeOrTubeHierarchyPairs` 0x00582D70) (VERIFIED) | Same three pairs, `register_bridge_hierarchy_edges_for_record`, `src/sim/pathfinding/zone_build.rs:719-759` (VERIFIED). The port is faithful here |
| Does the coarse route mark the cells *between* the endpoints? | No — one endpoint-to-endpoint edge jumps the span (VERIFIED, `gamemd.md` §2c) | No — identical (VERIFIED, `refusal.md` §2). Level-0 block size is 2, so a 17-cell span is ~9 unmarked zones |
| How is that compensated? | **In the fine A*: a bridge-layer neighbour skips the corridor test entirely.** Layer byte at `0x00429E54`–`0x00429E7A` is 0 iff `Flags & 0x100` **and** `abs(Pathfinder+0x30 − cell[+0x11B]) > 1`; `0x00429EAF` `JZ 0x00429F04` bypasses the stamp check, the `+0x122` escape and `allowHS` together (VERIFIED, disassembly) | **Not compensated. The branch is absent.** `HierarchyGate::allows(x, y)` takes no layer, no height, no bridge term (`core.rs:381-386`), and `core.rs:1354-1360` applies it to every compass neighbour (VERIFIED) |
| Can the hierarchy refuse an order? | **No.** A failed `Zone_precheck` logs `"Hierarchical findpath failure…"`, clears the HS flag and runs the **unrestricted** A* anyway (`AStar_pathfind_search` 0x0042C900, 0x0042CB22 region). The only hard refusal is `GetZoneID(src) != GetZoneID(dst)` — and `GetZoneID` 0x0056D230 is **bridge-aware**, resolving a deck cell through its bridge record (VERIFIED) | **Yes.** `src/sim/pathfinding/zone_search.rs:810` returns `None` with no A* at all when the precheck fails and zones differ; and the gated A* itself can exhaust (VERIFIED) |
| Per-plane bookkeeping | Two closed sets / two g-cost arrays on `PathfinderClass` (+0x18/+0x24 ground, +0x1C/+0x20 deck), two object lists on `CellClass` (+0xE4/+0xE8) (VERIFIED) | Layer-split closed lists and `MovementLayer` — right shape (VERIFIED) |

**Did VERA invent a gate gamemd lacks?** Not the gate itself — gamemd has the corridor filter.
VERA invented its **scope**: it applies the filter to a candidate class the binary explicitly
exempts. Per `AGENTS.md`, that is an invented gate in `sim/` and the fix is to delete its
over-reach, not to add a bridge special case beside it. VERA also invented the *refusal* semantics
at `zone_search.rs:810` (gamemd downgrades, never refuses) — a second, separate divergence that is
not what fires on Bay of Pigs (see §4, cause 1b).

The repo already knows about the missing branch: `src/sim/pathfinding/zone_search.rs:29-36`
documents the exemption verbatim as "a **second exemption**" and it is simply not implemented
(VERIFIED, read this session).

---

## 4. RANKED CAUSES

### 1. Hierarchy corridor gate applied to bridge-deck neighbours — **CONFIRMED ROOT CAUSE**
*For:* gate-off ablation flips `None` → `Some(19)` with the only changed input being
`blocker_neighbor_counts`; per-goal sweep shows reachability tracking the per-cell blocker count
exactly (VERIFIED, repro §2/§3). gamemd's bypass at `0x00429EAF` verified in two lanes.
*Against:* nothing.
*Settled already.* The `(A) precheck-refused-before-A*` vs `(B) gate-rejected-inside-A*` ambiguity
that `refusal.md` §1 left open is **closed by the repro sweep**: with the gate ON, goal `(111,135)`
(blocker count 1) returns `Some(2)` while goal `(111,136)` (count 0) returns `None`. A `zone_search.rs:810`
refusal cannot vary with the goal cell's blocker count — the precheck does not read it. So the
precheck **Passed** and the failure is inside the gated A*: **(B)**. VERIFIED by deduction from
repro §3's sweep table.

### 2. Mid-span deck height re-derived from the A* path layer — **REAL, ALREADY FIXED**
Explains the literal report wording; does not explain "bridges do not work".
*For:* `5c67724c`'s commit body describes exactly "appeared to fall to the bottom of the bridge or
through it"; the one-cell-at-a-time crossing gives a route by which a player reaches mid-span even
with cause 1 active.
*Against:* on `bf9a3a0f` the height holds on all 17 deck cells every frame (VERIFIED, repro §4).
*Cheapest check:* already done — the repro step-walk is the check.

### 3. `spawn_object` places a unit on a deck cell at riverbed height — **REAL, OPEN**
*For:* directly observed, `z=1 on_bridge=false layer=Ground` on a deck cell with deck level 5;
the unit is then unpathable in both gate states (VERIFIED, repro §6).
*Against:* Bay of Pigs places no units on a deck, so it is not the reported map's cause.
*Cheapest check:* grep the map corpus for pre-placed units whose cell carries
`bridge_structural`; one pass over `.mmx` load output, no cargo needed.

### 4. Cliff gate blocks a mover mid-span — **REAL, INTRODUCED BY `5c67724c`, DORMANT ON THIS MAP**
`movement_step.rs:2050-2056` compares the mover's `position.z` (now deck height, 5) against
`next_cell.effective_cell_z_for_layer(next_layer)`, and its bridge escape is
`is_bridge_transition_cell() || is_elevated_bridge_cell()` — the latter routed through
`bridge_deck_level_if_any()` = `bridge_walkable.then_some(...)` (`core.rs:1755-1762`, VERIFIED).
So a **structural** deck cell with `bridge_walkable` clear has no escape, `diff = 4 >= 3`, and the
mover is blocked mid-span.
*For:* the code path is verified above; `5e1cbe08`'s commit body records it as a known regression.
*Against:* on Bay of Pigs every deck cell has `bridge_walkable = 1` and `transition = 1`, so it
never fires there (VERIFIED, repro span table).
*Cheapest check:* count cells with `bridge_structural && !bridge_walkable` across the map corpus
during load; zero everywhere ⇒ dormant, non-zero ⇒ fix before merge.

### 5. `zone_search.rs:810` refuses where gamemd downgrades to unrestricted A* — **REAL, NOT THIS BUG**
*For:* gamemd's `0x0042CB22` region provably falls back rather than refusing (VERIFIED).
*Against:* the precheck **Passed** on Bay of Pigs (cause 1's deduction), so this line was never
reached here.
*Cheapest check:* a `trace_sink` collector on a map where the precheck fails; deferred.

### 6. A* routes the tank along the riverbed under the span — **REFUTED for this map**
*Refuted by:* repro §2 — the ground plane beneath the deck is level-0 zone `0`, there is no
under-span route to take, and the gate-off search from a riverbed cell returns `None` (VERIFIED).
Also refuted structurally: `HierarchyGate::allows` is layer-agnostic, so closing the deck
necessarily closes the ground plane at the same `(x, y)` — it cannot produce a lower route
(VERIFIED, `refusal.md` verdict). The mechanism nonetheless exists in principle wherever the gate
is off and a mover can reach riverbed height (`terrain_cost.rs:104-105` gives a deck cell
`COST_NORMAL` on the ground layer) — recorded as a residual, not as this bug.

### 7. Render-side height — **UNCHECKED**
Every `z` in the repro is the sim value; the render lane was not run. If the user sees a unit
visibly at the bottom of the river while the sim says `z=5`, this is the remaining candidate.
*Cheapest check:* one screenshot of a unit mid-span with the sim `z` logged alongside.

---

## 5. FIX PLAN

**One change. It deletes gate scope; it adds no special case.**

**Target:** `src/sim/pathfinding/core.rs:1354-1360`.

```rust
if let Some(gate) = options.hierarchy_gate {
    // AStar_main_loop @ 0x00429A90: the layer byte at [ESP+0x60]
    // (0x00429E54-0x00429E7A) is 0 iff the candidate carries Flags & 0x100 AND
    // abs(Pathfinder+0x30 - cell[+0x11B]) > 1; 0x00429EAF then JZ 0x00429F04,
    // skipping the corridor stamp, the +0x122 escape and allowHS together.
    let deck_exempt = neighbor_cell.has_structural_bridge()
        && current.height.abs_diff(neighbor_cell.ground_level) >= BRIDGE_HEIGHT_THRESHOLD;
    if !deck_exempt && !gate.allows(nx, ny) {
        trace_step.rejected_reason = Some("hierarchy_gate_blocked");
        emit_astar_trace(options, trace_step);
        continue;
    }
}
```

*gamemd source:* `AStar_main_loop @ 0x00429A90`, layer byte `0x00429E54`–`0x00429E7A`, bypass
`JZ 0x00429F04` at `0x00429EAF` (VERIFIED in both `gamemd.md` §5.2 and `repro.md` §7).

**Do NOT reuse `neighbor_use_bridge` from `core.rs:1128`.** It is `is_at_bridge_level`
(`core.rs:463-465`), which reads `bridge_walkable` where the native byte reads the `0x100`
structural bit — the mismatch already recorded in the provenance block at `core.rs:446-462`.
Reusing it would additionally exempt every ramp and bridgehead cell from the corridor, a wider
divergence than the bug (VERIFIED reasoning, `refusal.md` §4). Build the exemption on
`has_structural_bridge()` (`core.rs:1764-1766`). Resolving the `bridge_walkable` /
structural mismatch in `is_at_bridge_level` itself is a separate slice with its own fixture — do
not fold it in.

**Test that pins it.** In `src/sim/movement/movement_bridge_retail_tests.rs`, a case that loads
`BayOPigs.mmx`, spawns one MTNK at `(111,134)`, issues **one ordinary `Command::Move`** to
`(111,152)` through `SimRuntime::advance_frame` with `blocker_neighbor_counts` passed exactly as
production passes it (**nothing disabled — no gate ablation**), and asserts:
the order is accepted; the unit arrives at `(111,152)`; every one of the 17 deck cells on the
recorded path is `layer=Bridge`; and the unit's `z` is 5 with `on_bridge=true` on each of them.
The existing ablation at `movement_bridge_retail_tests.rs:622-664` passes `None` and must stay
separate — it disables the gate and therefore cannot pin this.

**Also delete before handing on** (repro lane's own note): the throwaway harness
`src/sim/movement/bridge_repro_tmp_tests.rs` and its two `#[cfg(test)] mod` lines in
`src/sim/movement/mod.rs`.

**Not in this slice:** cause 3 (`spawn_object` deck placement), cause 5 (`zone_search.rs:810`
refusal vs gamemd's downgrade), and the `is_at_bridge_level` flag mismatch. Each needs its own
fixture and none of them is what refuses the Bay of Pigs order.

---

## 6. WHERE THIS LEAVES THE HEIGHT FIX (`5c67724c`, `5e1cbe08`)

**KEEP both. Do not revert. Amend one line for the regression.**

* **`5c67724c` — keep, VERIFIED correct.** The repro ran on a HEAD containing it and the step-walk
  shows `z=5`, `on_bridge=true`, deck occupancy 5 on all 17 deck cells for every frame, with a
  clean clear at `(111,152)`. That is the behaviour `FootClass::Set_Height_On_Bridge 0x005F5FA0`
  specifies (terrain level + 4 exactly when the mover's own OnBridge byte is set). It removed two
  inputs gamemd does not have — a stored per-cell deck level behind an `Option`, and the A* path
  layer. Reverting would restore a mover height derived from the planner's layer, which has no
  native counterpart, and would re-open the literal "falls to the bottom" symptom on the
  one-cell-at-a-time crossing that players are currently forced into.
* **`5e1cbe08` — keep.** Its own commit body already withdraws the claim that it fixes the tank
  case, and that self-correction is right: no Drive mover's Z has ever come from
  `resolved_track_endpoint`. It stands on its own merits — it deletes an A*-layer dependence gamemd
  lacks on the ship endpoint. It earns no credit for the bridge bug and should not be cited for it.
* **The cliff-gate regression — fix now, in the same slice, one line.** `movement_step.rs:2050-2056`
  reads terrain height while the mover now carries deck height, and its bridge escape
  (`is_elevated_bridge_cell()` → `bridge_walkable.then_some(...)`, `core.rs:1755-1762`) misses a
  structural deck cell whose `bridge_walkable` is clear, blocking the mover mid-span. Widening the
  escape to `|| next_cell.has_structural_bridge()` is smaller and safer than reverting a fix that
  is verified correct on real map data. It is dormant on Bay of Pigs (every deck cell there has
  `bridge_walkable = 1`), so it is not urgent — but it costs one line and it is a known live edge,
  so it should not be left open behind a merge.
  **The honest longer answer:** the whole cliff heuristic is a VERA invention —
  `CLIFF_HEIGHT_THRESHOLD` is documented `VERA-internal, gamemd equivalent UNCHECKED` at
  `src/sim/movement/mod.rs:234-238`, with no address and no verified owner. gamemd's per-step
  legality is `CheckBridgeTraversal 0x004D9C60` (virtual, slot 0x1B0, returns 0 = allowed /
  7 = blocked), which VERA already ports as `check_bridge_traversal` in the same expansion. Per
  `AGENTS.md`, deleting the invented gate beats patching it — but that is a separate slice with its
  own evidence, and patching it here is the smaller, safer move today.

---

## 7. RESIDUALS

| # | Residual | Trigger | Player effect | Ordinary-play frequency |
|---|---|---|---|---|
| R1 | `spawn_object` has no bridge-deck handling (`world_spawn.rs:344`) | any object placed directly on a structural deck cell — map-placed units, and any other direct-placement path | unit sits at riverbed height under an intact span with `on_bridge` clear, and cannot path anywhere at all | UNCHECKED across the map corpus; zero on Bay of Pigs (VERIFIED) |
| R2 | Cliff gate misses structural deck cells with `bridge_walkable` clear (`movement_step.rs:2050-2056`) | driving onto such a cell while carrying deck height | mover stops dead mid-span and reports blocked | zero on Bay of Pigs (VERIFIED); UNCHECKED elsewhere |
| R3 | `is_at_bridge_level` reads `bridge_walkable`, native reads the `0x100` structural bit (`core.rs:446-465`) | any ramp or bridgehead cell | closed-list selection and carried height take the deck branch where gamemd takes ground; a bridge approach can be planned on the wrong plane | every bridge approach on every bridge map (per the existing provenance block) |
| R4 | `zone_search.rs:810` refuses; gamemd logs, clears HS and runs the unrestricted A* (`0x0042CB22`) | precheck fails **and** the two coarse zone ids differ | unit refuses an order retail accepts | UNCHECKED — did not fire on Bay of Pigs (VERIFIED) |
| R5 | Under-deck ground routing is admissible wherever the gate is off (`terrain_cost.rs:104-105` gives a deck cell `COST_NORMAL` on the ground layer) | a mover that reaches riverbed height on a map where the riverbed is passable | unit drives visibly under an intact span | UNCHECKED; impossible on Bay of Pigs, whose riverbed is zone 0 (VERIFIED) |
| R6 | Corridor-test ordering: native tests the stamp at `0x00429EA4` **before** `Can_Enter_Cell` (`0x00429F54`); VERA tests it at `core.rs:1354`, after walkability and entity blocks | any cell failing both tests | none on outcome; changes which `rejected_reason` a trace reports | every gated search (diagnostic only) |
| R7 | Render-side deck height not examined by any lane | — | if it diverges, a unit at sim `z=5` could still be drawn at the riverbed | UNCHECKED |
| R8 | "Almost every map" is a prediction, not a measurement | — | — | repro established both Bay of Pigs bridges only; the gate runs on every path build, so generalisation is expected but unmeasured (stated as such in repro §9) |

**Not verified anywhere in this synthesis:** the concrete level-0 zone ids of deck cells on maps
other than Bay of Pigs; writers of `CellClass::Flags` bits `0x100` / `0x200` in the binary
(`gamemd.md` §8, UNKNOWN); rows 4..12 of `g_nMovementZonePassabilityMatrix`; and
`Zone_precheck`'s marked-set semantics as read by the repro lane, which took them from the VERA
implementation rather than the binary (the `gamemd.md` lane did verify them at `0x0042C290`).
