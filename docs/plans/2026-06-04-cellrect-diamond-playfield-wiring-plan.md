# CellRect Diamond-Playfield Wiring Implementation Plan (reworked)

> **For Claude:** Execute task-by-task; each is self-contained.
> **Reworked 2026-06-04 after `/review-plan`.** The original plan claimed two live callers and a
> building-placement parity win. The review found: (a) `find_nearby_cell.rs` is **test-only** (its live
> cousin is a separate impl not backed by this facade); (b) **building placement never calls the facade**;
> (c) only the **production spawn-fallback** path is a genuine live consumer — and even that may be
> **hash-neutral** because off-diamond cells are already clipped from the terrain/path grids at load.
> So this plan is scoped to the one real live consumer, **measures impact before wiring**, and parks the
> rest in an explicit Prerequisites section. Do not re-expand the scope without doing the adoption work.

**Goal:** Feed the real isometric-diamond playfield bounds into the one live consumer of `sim/cell_rect.rs`
(the production spawn-fallback cell search), replacing its rectangle fallback — and first measure whether
that changes any observable behavior, since load-time LocalSize clipping may already enforce the diamond.

**Architecture:** `sim/cell_rect.rs` is a read-only facade over existing grids. It has exactly **two**
consumer files: `find_nearby_cell.rs` (test-only — no live caller) and `production_spawn.rs` (live, via the
spawn-fallback ring search). This plan derives the diamond's five bound fields from the map header, holds one
`PlayfieldBounds` on `Simulation`, threads it into the production-spawn path, and proves the net effect.
Pure `sim/` — no render/ui/sidebar/audio/net dependency.

**Design Doc / source of truth:** [docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md)
(§5 C-GRID 1–3, §6 boundary, 2026-06-04 refresh "IMPLEMENTED-BUT-UNWIRED").

---

## Grounding Summary

- **Binary (verified this session).** `MapClass::Is_Cell_In_Playfield 0x00578460` / `IsRectInPlayfield
  0x00578390` decompiled and compared term-by-term to `cell_in_playfield_diamond` (cell_rect.rs:479):
  **bit-exact** (low/high sum band, both diagonals, the slope-byte `h += 1` bump, the inclusive four-corner
  rect, the never-null dummy). `base = MapClass+0xf4`, then `+0xfc/+0x100/+0x104/+0x108`. **Not yet known:**
  how the engine derives those five fields from `Size`/`LocalSize`. Two candidate writers ruled out:
  `FUN_006851f0` (zeroes the field at game reset), `FUN_00565b00` (cell-array realloc). The real setter
  writes via `this`-relative stores (no absolute xref). → **Task 1.**
- **Facade consumers (verified via grep + test-boundary check).** `cell_rect.rs` is referenced by only two
  files (plus `mod.rs:70 pub mod cell_rect`):
  - `find_nearby_cell.rs` — `#[cfg(test)] mod tests` begins at **:303**; the `NearbyQuery` builder (:384)
    and every `find_nearby_passable_cell` call (:468–583) are **inside that test module**. The FNPC used in
    a live skirmish is a **different** implementation: `miner_dock_sequence.rs:361`/`:411`, called from
    `miner_system.rs:1221`, `bunker_link.rs:222` — **not backed by `cell_rect.rs`**. → find-nearby is NOT a
    live consumer of this facade.
  - `production_spawn.rs` — **live**. Chain: `find_spawn_cell_for_owner:30` → `find_spawn_cell_near_structure:237`
    → `nearest_walkable_around:355` → `spawn_fallback_candidate_passable:506` → `check_occupancy_rect:537`
    (`playfield_bounds: None`). Candidates are clamped to `[0, grid.width()-1] × [0, grid.height()-1]`
    (:373–376), i.e. the full Size rectangle.
- **Building placement does NOT call the facade** (grep: no `check_*_rect` outside the two files above).
  The diamond cannot govern structure placement by flipping `None` — that needs facade adoption (Prerequisites).
- **Map data.** `MapHeader` (map_file.rs:103–119) parses `Size`→`width/height`, `LocalSize`→
  `local_left/top/width/height`. The sim currently receives only `width/height` (app_init.rs:893–894);
  **no `local_*` value reaches `src/sim/`** (verified). The app layer has a render-space `LocalBounds::from_header`
  (app_init.rs:470) — do NOT reuse it in `sim/`.
- **Possible no-op.** Cells outside LocalSize are clipped from the terrain grid at load (terrain.rs:557–696),
  and `check_passability_rect` already rejects a cell with neither terrain nor path cell (cell_rect.rs:301–303).
  So off-diamond cells may already be excluded on the production-spawn path → the diamond could be
  **hash-neutral**. **Measured in Task 4 before wiring.**
- **Unknown after grounding:** the Size/LocalSize→5-field formula (Task 1); whether the diamond changes any
  selected spawn cell vs today (Task 4).

## Key Technical Decisions

- **Scope = production spawn-fallback only.** It is the sole live facade consumer. **Confidence: high.**
  **Source:** grep + test-boundary verification (review).
- **Measure before wiring.** LocalSize clipping may already enforce the diamond, making this hash-neutral; a
  spike (Task 4) decides whether the wiring changes behavior or is correctness-made-explicit. **Confidence:
  high (method).** **Source:** terrain.rs clip + cell_rect.rs:301–303.
- **Derive the five fields from the engine's own setter, not a guess.** Field human-names are UNVERIFIED
  (cell_rect.rs:179–198); only the consuming formula is proven. **Confidence: low (formula unresolved).**
  **Source:** Ghidra (Task 1). Flag for re-review after Task 1.
- **find-nearby + building-placement are OUT of scope** (Prerequisites section) — they require routing those
  live systems through the facade first, which is a separate, larger effort. **Confidence: high.** **Source:**
  review.

## Open Questions

### Resolved During Review
- *Is `find_nearby_cell.rs` a live caller?* No — test-only (`mod tests` @ :303). Live FNPC is `miner_dock_sequence.rs`.
- *Does building placement use the facade?* No — facade has only two consumer files.
- *Does LocalSize reach the sim?* No — absent from `src/sim/` today (Task 3 must add it).
- *Is the diamond formula correct?* Yes — bit-exact vs `0x00578460`/`0x00578390`.

### Deferred to Implementation
- **The Size/LocalSize → 5-field formula** — needs the engine setter (Task 1).
- **Does the diamond change any live spawn-cell choice** vs the current rectangle, given terrain clipping —
  measured (Task 4), not assumed.

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Research | gamemd.exe (Ghidra) + `docs/research/` | Size/LocalSize → 5-field formula (Task 1) |
| Modify | `src/sim/cell_rect.rs` | `PlayfieldBounds::from_map_header` + tests (Task 2) |
| Modify | `src/sim/world/mod.rs` (`Simulation` :279) + sim map-load path | Add LocalSize in; hold `playfield_bounds` (Task 3) |
| Modify (temp) | `src/sim/production/production_spawn.rs` | Measurement spike (Task 4) |
| Modify | `src/sim/production/production_spawn.rs` (`:355`,`:237`,`:506`,`:546`) | Wire bounds into spawn-fallback (Task 5) |
| Modify | tests + state-hash baseline | Acceptance + regression (Task 6) |

## Interface Changes

- **New:** `PlayfieldBounds::from_map_header(&MapHeader) -> PlayfieldBounds`.
- **`Simulation`** gains `playfield_bounds: Option<PlayfieldBounds>` (map-derived; rebuilt on load — see Sim Checklist).
- **`spawn_fallback_candidate_passable`** + its caller chain (`nearest_walkable_around:355`,
  `find_spawn_cell_near_structure:237`) gain a `playfield_bounds: Option<PlayfieldBounds>` param threaded from
  the live entry that holds the `Simulation`/bounds. **No change** to `NearbyQuery` (test-only path; deferred
  to Prerequisites).

## Sim Checklist

- [ ] Diamond math is integer (`i32`/`i16`) — no float. ✔ by design.
- [ ] `Simulation.playfield_bounds` is map-derived; if `Simulation` is serialized wholesale, mark it
      `#[serde(skip)]` and rebuild in the post-load map-restore step; otherwise it must enter the state hash.
      Pick one, test it.
- [ ] No render/ui/sidebar/audio/net dependency (do NOT reuse app-layer `LocalBounds`).
- [ ] Tick ordering unaffected (spawn-cell selection is command-time).

## Risk Areas

- **Wrong formula → wrong playfield** (every spawn near a map edge). Mitigation: Task 1 Step 3 empirical
  cross-check vs the existing LocalSize clip on real maps.
- **Determinism.** A changed spawn-cell choice is state-hash relevant. Mitigation: Task 4 measures; Task 6
  gates on a full-skirmish replay hash (unchanged if Task 4 found nil; intentional re-baseline otherwise).
- **Threading miss.** Bounds must reach `spawn_fallback_candidate_passable` through 2–3 call levels; a missed
  level silently keeps the rectangle. Mitigation: thread from the live entry holding the `Simulation`.

## Parity-Critical Items

| Task | Item | Why it matters | Verification |
|------|------|----------------|--------------|
| Task 1 | Size/LocalSize → 5 diamond fields | Whole feature's correctness | Ghidra setter + empirical match to LocalSize clip on ≥2 maps |
| Task 2 | `from_map_header` field order/signs | Asymmetric diamond (+X east/+Y south, doubled extents) | Unit test: Dustbowl + square map, known in/out cells |
| Task 4 | Diamond-vs-rectangle divergence on the **live** spawn path | Decides hash impact AND whether the fix is observable at all | Shadow run on a non-rectangular map; count selected-cell differences |
| Task 6 | Off-diamond spawn-cell rejection on the live path | Player-visible: a unit produced near a corner with a blocked exit won't fall back onto an off-playfield cell. **Frequency: rare** (corner base + blocked primary cell) — and possibly already masked by clipping (Task 4 decides) | New test on the live `find_spawn_cell_*` chain (not the facade directly) + hash regression |

---

## Tasks

### Task 1: Extract the Size/LocalSize → 5-field diamond formula (RESEARCH — blocks all) — ✅ DONE 2026-06-04

**RESULT (binary-VERIFIED, [docs/research/CELLCLASS_PLAYFIELD_BOUNDS_FROM_LOCALSIZE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_PLAYFIELD_BOUNDS_FROM_LOCALSIZE_GHIDRA_REPORT.md)):**
`base (+0xf4) = Size.width` (= `MapHeader.width`), set unconditionally by `MapClass::Resize 0x00565c10`;
`+0xfc/+0x100/+0x104/+0x108 = LocalSize.{left,top,width,height}` verbatim, set by the map loader
`Read_Map_Section_And_IsoMapPacks 0x004ad76b` via `INIClass::ReadRect 0x00527cc0` (`sscanf "%d,%d,%d,%d"`).
**No transform at store time** — the iso transform is entirely in the consumer `Is_Cell_In_Playfield
0x00578460`. `Size.left/top` are zeroed; `Size.height (+0xf8)` is not a diamond field. Use the formula in
Task 2.

**Why (original):** `PlayfieldBounds` consumed five fields whose derivation from the map header was unknown;
the project bar forbids guessing a load-bearing formula.

**Files:** read-only Ghidra (gamemd.exe); findings → `docs/research/`.

**Step 1: Locate the setter.** Fields `MapClass+0xf4/+0xfc/+0x100/+0x104/+0x108` (singleton `g_Map 0x0087F7E8`)
are written via `this`-relative stores (no absolute xref). Approaches: `search_functions` for
`Set_Dimension`/`Read_INI`/`One_Time`/`Set_Map`; find the reader of the `"Size"`/`"LocalSize"` INI strings that
writes a MapClass field; `get_function_callers` of `MapClass__Init_Alloc 0x00565800`. **Ruled out (do not
revisit):** `FUN_006851f0` (reset-zero), `FUN_00565b00` (realloc).

**Step 2: Read off the 5 expressions** as functions of `Size=(sx,sy,sw,sh)` / `LocalSize=(lx,ly,lw,lh)`,
capturing sign and any `*2` doubling. Cite the setter address + decompile lines.

**Step 3: Empirical cross-check (mandatory).** Plug the formula's 5 values for Dustbowl
(`Size=70x76, LocalSize=2,8,65,62`, terrain.rs:1021) into the resolved `cell_in_playfield_diamond` math by hand
(flat, `h=0`) and confirm in/out of the playable rectangle corners matches what `terrain.rs`'s LocalSize clip
keeps vs clips. Repeat for one square map. Disagreement ⇒ formula wrong ⇒ iterate.

**Step 4: Record** the formula + Dustbowl worked example + setter citation to
[docs/research/CELLCLASS_PLAYFIELD_BOUNDS_FROM_LOCALSIZE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_PLAYFIELD_BOUNDS_FROM_LOCALSIZE_GHIDRA_REPORT.md) (+ one-line pointer from study §9).
Mark VERIFIED only after Step 3 passes.

**Step 5: Verify** the doc has all 5 expressions, the setter address, and the Dustbowl numbers with citations.
**Do not start Task 2 until VERIFIED.**

**Step 6: Commit** the research doc.

---

### Task 2: `PlayfieldBounds::from_map_header` + unit tests

**Why:** Turn Task 1's formula into the one constructor that builds bounds from a parsed map.

**Files:** Modify `src/sim/cell_rect.rs`.

**Pattern:** mirrors the `diamond_bounds()` test fixture (cell_rect.rs:934).

**Step 1: Implement** (body = Task 1's verified expressions — replace the `/*T1*/` slots; do NOT ship guesswork):
```rust
// src/sim/cell_rect.rs
use crate::map::map_file::MapHeader;

impl PlayfieldBounds {
    /// Build the engine's isometric-diamond playfield bounds from the parsed map
    /// header. Derivation verified against gamemd's MapClass dimensions setter
    /// (see docs/research/CELLCLASS_PLAYFIELD_BOUNDS_FROM_LOCALSIZE_GHIDRA_REPORT.md).
    pub fn from_map_header(h: &MapHeader) -> Self {
        // Verified 2026-06-04 (Task 1): base = Size.width; extents = raw LocalSize.
        // No transform — the iso transform lives in cell_in_playfield_diamond.
        Self {
            base: h.width as i32,         // MapClass+0xf4 = Size.width
            off_fc: h.local_left as i32,  // +0xfc  = LocalSize.left
            off_100: h.local_top as i32,  // +0x100 = LocalSize.top
            off_104: h.local_width as i32,// +0x104 = LocalSize.width
            off_108: h.local_height as i32,// +0x108 = LocalSize.height
        }
    }
}
```

**Step 2: Tests** — assert the verified Dustbowl fields and a pass/fail cell pair. Dustbowl header
(`width=70, local 2,8,65,62`) → `PlayfieldBounds { base: 70, off_fc: 2, off_100: 8, off_104: 65, off_108: 62 }`;
diamond is `86 < X+Y ≤ 212` and `-65 ≤ X-Y ≤ 63` (flat). Interior cell `(74,75)` (sum 149, diff −1) passes;
off-diamond corner `(0,0)` (sum 0) fails — assert both via `check_occupancy_rect` with `playfield_bounds:
Some(b)` (reuse the pattern at cell_rect.rs:958–976). Add one square-map case for a second data point.

**Step 3: Verify** `cargo test -p vera20k from_header -- --nocapture`; read the literal `test result:` line. PASS.

**Step 4: Commit.**

---

### Task 3: Thread LocalSize into the sim; hold `playfield_bounds` on `Simulation`

**Why:** The live spawn path needs one source for the bounds; LocalSize is not in `src/sim/` today, so it must
be threaded in alongside the existing width/height.

**Files:** Modify `src/sim/world/mod.rs` (`Simulation` :279 + the map-load entry the app feeds width/height into);
the app-side call site (app_init.rs near :893) to pass the LocalSize values or the `MapHeader`.

**Step 1: Confirm the sim map-load entry** (the one app_init.rs:893–894 feeds `width`/`height`). Extend its
signature to also receive the four LocalSize values (or the whole `&MapHeader` — `sim/` already depends on
`crate::map`, so `crate::map::map_file::MapHeader` is allowed). Document the choice.

**Step 2: Add the field + build it:**
```rust
// Simulation (world/mod.rs) — map-derived. If Simulation is serialized wholesale,
// add #[serde(skip)] and rebuild in the post-load map-restore step (see Sim Checklist).
pub playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
```
At the map-load entry (where width/height are set):
```rust
self.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds::from_map_header(&header));
```
Initialize to `None` in every other `Simulation` constructor (skirmish stub, tests).

**Step 3: Verify** `cargo check -p vera20k` compiles; add a focused test that a sim built from the Dustbowl
header has `playfield_bounds == Some(expected)`.

**Step 4: Commit.**

---

### Task 4: Measurement spike — does the diamond change any live spawn cell? (DECISION GATE)

**Why:** Off-diamond cells may already be excluded by terrain clipping. Measure before wiring so the rest of the
plan only proceeds if it changes behavior — and so any change is an intentional, documented re-baseline.

**Files:** temporary instrumentation in `src/sim/production/production_spawn.rs` (or a one-off harness test).

**Step 1: Instrument** `spawn_fallback_candidate_passable` (or `nearest_walkable_around`): behind
`#[cfg(debug_assertions)]` / an env flag (`PLAYFIELD_SHADOW`), compute the `check_occupancy_rect` result BOTH
ways for each candidate — once with `Some(bounds)` (pass the sim's bounds through), once with the current
`None`+`map_size` — and `log` every cell where they differ, plus whether the *selected* (returned) cell differs.

**Step 2: Run** a full skirmish replay on a non-rectangular map (e.g. Dustbowl) with production happening near
a map corner. Record: divergence count, the cells, and whether any *selected* spawn cell changed.

**Step 3: Decide.**
- **Zero selected-cell divergences** → wiring is hash-neutral. It is correctness-made-explicit, not a behavior
  change. **Surface this to the user**: the diamond is already enforced by clipping; ask whether to land the
  wiring anyway (explicitness/future-proofing) or stop here. Do not silently proceed.
- **Any selected-cell divergence** → real fix; the hash will change. Proceed; plan the re-baseline in Task 6.

**Step 4: Remove instrumentation** (or leave behind the env flag). **Commit** the measurement note.

---

### Task 5: Wire the diamond into the production spawn-fallback path

**Why:** The one live facade consumer; replace its rectangle fallback with the real diamond. (Only if Task 4
warranted proceeding, or the user opted to land the hash-neutral version.)

**Files:** Modify `src/sim/production/production_spawn.rs` — `spawn_fallback_candidate_passable:506` (call at
:546), `nearest_walkable_around:355`, `find_spawn_cell_near_structure:237`, and the live entry holding the
`Simulation`/bounds (from `find_spawn_cell_for_owner:30` down).

**Step 1: Add a `playfield_bounds: Option<PlayfieldBounds>` param** to `spawn_fallback_candidate_passable` (after
`resolved_terrain`) and use it (replace `playfield_bounds: None` at :546 with the param).

**Step 2: Thread it up** through `nearest_walkable_around` (add the same param; pass at the four call sites
:379/399/421/441) and `find_spawn_cell_near_structure` up to the live entry that has the `Simulation` — pass
`sim.playfield_bounds`. Update the test callers (:882,:922) to pass `None`.

**Step 3: Verify** `cargo test -p vera20k spawn -- --nocapture`. PASS (existing fixtures pass `None`).

**Step 4: Commit.**

---

### Task 6: Acceptance on the live path + hash regression + flip

**Why:** Prove the player-visible behavior on the actual spawn chain (not the facade in isolation) and lock
determinism.

**Files:** test(s) near `production_spawn.rs` tests; state-hash baseline if Task 4 said so.

**Step 1: Live-path acceptance test.** Build a sim from a non-rectangular header; produce a unit at a structure
near the diamond edge with its primary exit cell blocked, so `find_spawn_cell_*` falls back; assert the chosen
cell is inside the diamond and an off-diamond corner is never returned. Name the cells (from Task 1's worked
example). **This must exercise `find_spawn_cell_for_owner`/`find_spawn_cell_near_structure`, not
`check_occupancy_rect` directly.**

**Step 2: Existing tests.** `cargo test -p vera20k cell_rect`, `... spawn` — all pass.

**Step 3: Hash regression.** Full-skirmish replay state-hash: unchanged if Task 4 found nil; else re-baseline in
this commit with the parity-improving reason stated.

**Step 4: Verify vs gamemd.** `/fidelity-check` or in-game: produce near a corner with a blocked exit in both
engines; confirm neither spawns on an off-playfield cell. Record.

**Step 5: Commit.**

---

## Prerequisites for Full Coverage (OUT OF SCOPE — separate efforts)

The diamond cannot govern these by flipping `None` → `Some(bounds)`; each needs the live system routed through
`cell_rect.rs` first. Documented here so the scope cut is explicit, not silent.

- **A. Adopt `cell_rect::find_nearby_cell` into the live FNPC.** Today the live find-nearby-passable-cell is a
  separate implementation (`miner_dock_sequence.rs:361`/`:411`, used by miners + bunker link), NOT backed by the
  facade; `find_nearby_cell.rs` is test-only. Wiring the diamond into find-nearby means either replacing the
  miner FNPC with the facade or adding the diamond to it directly. Only after adoption does adding a
  `playfield_bounds` field to `NearbyQuery` (find_nearby_cell.rs:50) + passing it at `candidate_passes:265` have
  any live effect.
- **B. Route building-placement legality through the facade.** Structure-placement validation does not call
  `check_passability_rect`/`check_occupancy_rect` at all today (the facade has only two consumer files). Making
  the diamond reject off-playfield structure placement requires routing the placement path through the facade —
  a prerequisite, not a field flip. This is the more player-visible target (placing a building at a map corner)
  and is the natural follow-on once A/B-style adoption is on the table.

---

## Sources & References

- **Design doc:** [docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md) (§5 C-GRID 1–3, §6, 2026-06-04 refresh).
- **gamemd.exe (verified this session):** `Is_Cell_In_Playfield 0x00578460`, `IsRectInPlayfield 0x00578390`
  (diamond, bit-exact vs cell_rect.rs:479); `Get_CellClass 0x005657A0` (never-null). Diamond fields
  `MapClass+0xf4/+0xfc/+0x100/+0x104/+0x108` (singleton `0x0087F7E8`). Ruled-out writers: `FUN_006851f0`,
  `FUN_00565b00`.
- **Map header / INI:** `[Map] Size=`, `[Map] LocalSize=` → `MapHeader` (map_file.rs:103–119); Dustbowl
  `Size=70x76 LocalSize=2,8,65,62` (terrain.rs:1021); LocalSize clip (terrain.rs:557–696).
- **Facade:** `src/sim/cell_rect.rs` (`PlayfieldBounds` :179, `cell_in_playfield_diamond` :479,
  `rect_in_playfield` :430, `check_occupancy_rect` :221, diamond corner tests :958–995).
- **Live consumer (in scope):** `production_spawn.rs` chain `find_spawn_cell_for_owner:30` →
  `find_spawn_cell_near_structure:237` → `nearest_walkable_around:355` → `spawn_fallback_candidate_passable:506`
  → `check_occupancy_rect:537`.
- **NOT live consumers (Prerequisites):** `find_nearby_cell.rs` (test-only, `mod tests` @ :303); live FNPC is
  `miner_dock_sequence.rs:361`/`:411`. Building placement: no facade call anywhere.
- **App-layer analogue (do NOT reuse in sim):** `LocalBounds::from_header` (app_init.rs:470).
