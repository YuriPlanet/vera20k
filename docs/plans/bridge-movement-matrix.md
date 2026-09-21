# Bridge movement coverage matrix — frozen ledger

**Status: FROZEN.** This is the ledger of an active autonomous program. Adopt it; do not rebuild it.

Freeze point: the three inventory lanes read the tree at HEAD `e3d17a28` on
`feature/bridge-movement-parity`. Every `file:line` below is that HEAD. Every citation in this
document is **second-hand from those three lanes** — no line was re-opened while writing the
ledger, and **no lane ran a crossing**. Where a lane wrote "pinned by", it had read an assertion
body, not observed a pass.

---

## 0. Operating rules for this program

A fresh session must read these before touching a row.

1. **The bar is gamemd, read through Ghidra.** Every behavioural claim carries a live decompile
   citation or a named research doc, or it is written `UNCHECKED`. Prose never upgrades a status.
   A VERA-internal gate with "gamemd has no equivalent" in its own provenance block is a
   divergence from the bar whether or not it currently produces a wrong pixel.
2. **A fix counts only when an ordinary undisabled order crosses on a real retail map.** Not when
   it compiles, not when a gate is bypassed by a test helper, not when a synthetic grid agrees.
   "Ordinary undisabled" means: a normal `Command::Move` (or the row's named order source) issued
   with every production gate ON, on a map loaded from the retail root.
3. **Unit tests on synthetic grids prove self-consistency, never parity.** They are regression
   ratchets. They can move a row out of `KNOWN-BROKEN`; they can never move a row into
   `VERIFIED-OK`.
4. **The ratchet comes before any row is worked.** The ratchet is the retail harness plus a bridge
   replay golden. Until both exist and are green, no row is worked — a fix landed without the
   ratchet cannot be shown not to have broken the two crossings that already sort-of work.
5. **One row per slice.** A slice touches exactly one matrix row, names it by id, and ends with
   the literal `test result:` line in its report.
6. **A fresh critic judges each slice.** The critic reads the diff and the literal test output.
   The critic does not read, and is not given, the builder's reasoning. A slice the critic cannot
   verify from the diff and the output is not landed.
7. **Re-entry is idempotent.** A session picking this up adopts this ledger as-is, re-derives the
   frontier from each row's own evidence column, and **re-traces nothing**. If a row says
   UNCHECKED, that is the finding — do not re-investigate to reconfirm it, measure it.

**`OI-31` — RESOLVED, was never another session's.** The inventory lane observed this repository
mid-edit and reported the retail harness as not compiling, because
`issue_crossing_order_without_hierarchy_gate` had been deleted while a second callsite still
referenced it. That edit was this program's own ratchet work and it landed at `7b66b073`: the
helper is gone deliberately, both the crossing and the control move now go through
`issue_ordinary_move`, and the harness compiles and passes. Nothing is blocked on it. Recorded
rather than deleted because the lesson generalises — an inventory lane reading a working tree
sees whatever is uncommitted at that instant, so a "live blocker" it reports is a claim about a
moment, not about the branch.

---

## 1. Dispositions

| Disposition | Meaning |
|---|---|
| `UNCHECKED` | No evidence either way. **The default.** Every row starts here. |
| `VERIFIED-OK` | An ordinary undisabled order was **observed** to behave correctly on a real retail map, with the citation. The only green state. Unit tests alone can never produce it. |
| `KNOWN-BROKEN` | A defect is confirmed, with citation and linked open-item id. |
| `RESIDUAL` | Recorded and deliberately not fixed. Must carry trigger, player effect, ordinary-play frequency. |
| `NOT-APPLICABLE` | Cannot occur in stock YR. One line of why. Lives in §6, not in the matrix. |

**Two rows are `VERIFIED-OK`: T1-03 and T1-04.** Both were settled on 2026-08-27 at `7b66b073`,
after this section was first written and while it still said zero.

The correction matters, so here is what changed rather than just the number. When the inventory
lanes read the tree, `tank_crosses_bay_of_pigs_high_bridge_at_deck_height` and
`tank_crosses_hills_high_bridge_at_deck_height` did issue their orders through a
hierarchy-gate-disabling helper, which rule 2 excludes — the lane was right. `7b66b073` deleted
that helper along with the defect it worked around: both the crossing and the control move now
go through `issue_ordinary_move`, and a refused order panics instead of silently re-issuing a
widened search. The tests were then run against the real install with nothing disabled —
`test result: ok. 4 passed; 0 failed` — which is the first crossing this project has recorded
under rule 2.

They remain `#[ignore]`d and require `RA2_DIR`; that is a property of needing retail assets, not
a disabled order, and it is why the always-on guard is the golden
(`bridge_parity_harness_tests.rs`, `606b4435`) rather than these.

---

## 2. Collapsed dimensions

Stated once so no slice re-derives them and no row is duplicated.

| Collapsed | Evidence |
|---|---|
| **High concrete × high wood** → one row each | Both resolve through `high_bridge_stamp_for_overlay` (`src/map/bridge_facts.rs:225-233`) to the same stamp; `BridgeStampFamily::{Nesw,Nwse}` names the two native setters, not the materials, and every consumer only tests `family != None`. Concrete/wood separate solely in theater tileset windows (`bridge_topology.rs:186-210`) and the CABHUT repair predicate (`resolved_terrain.rs:2280-2281`) — neither is movement. Fixtures happen to cover both anyway: BayOPigs is concrete, Hills is wood. |
| **`Nesw` × `Nwse` stamp family** → not a relation | Same evidence; `apply_modeled_cellclass_bridge_slot` is family-agnostic. |
| **Low wood × low urban/concrete** → one row each | Same `TubeClass` movement path; they differ only in the overlay damage bands (`bridge_specs.rs:96-146`). Concrete-low has no loose fixture anyway (§5). |
| **Walk order-source rows** → folded into the Drive order-source rows | Walk and Drive share the planner entry: both are admitted by `supports_layered_bridge_pathing` (`movement_path.rs:91-95`) and both reach `resolve_cell_transition_bridge_state`. Order source is selected above that. **Hover now folds too** — `53695936` admitted it to the same builder, so it shares the planner entry with Drive and Walk. It keeps its own attack-move row (T2-04) only because attack-move goal selection is itself unexercised, not because Hover takes a different branch. |
| **Teleport warp-onto-deck × Drive-piggyback warp** → one row | `set_destination_for_teleporter_entity` (`movement_commands.rs:180-206`) converts the piggyback case into an ordinary Drive move, so it inherits the Drive rows wholesale; only the pure warp arm is distinct. |
| **`onto` × `off` for low spans** → one `along` row | Low decks sit at the surrounding terrain level (`deck_level = level`, `resolved_terrain.rs:3571-3593`); there is no height event at either end. `tube_movement.rs` owns the whole object turn and writes `position.z = cell.level` with no deck term. One crossing exercises all three relations. |

---

## 3. THE MATRIX

Order sources: `move` = player Move click · `atk-mv` = attack-move · `AI` = AI team script ·
`bump` = scatter / repath-after-block · `retreat` = flee/retreat mission · `guard` = guard-area
pursuit.

Fixture shorthand — **LOOSE** = the map file sits in the retail root and
`headless_scenario::load` can name it today. **MIX** = it must be extracted first; record the
extraction as a slice prerequisite. **SYNTHETIC-NEEDED** = no retail fixture exists at all.

### Tier 1 — ordinary skirmish traffic

Drive, Walk and Hover, on and under intact high and low spans, under a player Move order.
Rows are ordered by expected player-visibility × frequency within the tier.

| ID | Loc | Kind | Rel | Src | Disp | Fixture | Open | The one check that settles it |
|---|---|---|---|---|---|---|---|---|
| T1-01 | Hover | high intact | onto | move | **VERIFIED-OK** | BayOPigs.mmx `(111,152)`→`(111,151)`; Hills.mmx `(98,74)`→`(97,74)` | OI-06 (=L1+L2), closed | **Fixed and settled 2026-08-27.** Cause: `supports_layered_bridge_pathing` excluded Hover, so the mover got a flat path with `Ground` stamped on every node including the deck, and the crossing loop's terrain test then asked whether it could enter the riverbed *under* the span. Seven refusals measured, all `layer_walkable`, zero `bridge_traversal` — the bridge check passed every time and was simply never asked about the right plane. The order was never dropped, so the unit drove into the abutment and was snapped back to cell centre ~13×/s indefinitely. Fix admits Hover to the whitelist. The one native gate on this path is VERIFIED (2026-08-27) by direct reads, on one stated premise: the admitted movers are all `[VehicleTypes]` hence `UnitClass`, which is what makes the fixed-vtable read the same slot their `CALL [EDX + 0x2CC]` dispatches through. Chain: `UnitClass` vtable `0x007F5C70` (installed by its constructor @ `0x0073543A`; identity from the existing Ghidra label, UNCHECKED against RTTI) → slot `+0x2CC` at `0x007F5F3C` holds `0x004D3810` → `Find_Path` calls that slot at `0x004D397F` and clears-and-returns-0 at `0x004D3989` → `CanReachDestination` reads `TechnoTypeClass+0x5B4` (MovementZone) and tail-calls `MapClass::Can_Reach_Zone`. It is a zone reachability abort with **no locomotor term**. Scoped: *at this gate* nothing picks a pathing plane by locomotor kind; a binary-wide negative is NOT claimed. The fix stands on gate-*deletion* logic (removing a VERA restriction needs the absence of a native gate demanding it, not affirmative native proof) plus the production crossings. Settled by `hover_tank_crosses_bay_of_pigs_high_bridge_at_deck_height` and `hover_tank_crosses_hills_high_bridge_at_deck_height`, ordinary undisabled orders, `test result: ok. 5 passed; 0 failed`. **Linked residual R-T101a:** `[LCRF]`, `[SAPC]` and `[YHVR]` were moved onto the layered builder by this fix and are exercised by nothing — the harness drives only `MTNK` and `ROBO`. Trigger: any amphibious transport path build. Player effect: unknown; a routing regression would be visible. Frequency: every water map, every faction's naval opening. Diagnosis: `docs/plans/bridge-hover-t1-01-diagnosis.md`. |
| T1-02 | Hover | high intact | along | move | **RESIDUAL** — L1 closed by T1-01, L5 open | BayOPigs.mmx, `(111,143)` mid-span; Hills.mmx row y=74 | OI-25 (=L1) **closed**, L5 open (R3) | **Rewritten 2026-08-27 — its stated mechanism no longer exists.** This row's check was "`build_flat_fallback_layers` returns all-`Ground` for a ROBO, so the deck is checked on the ground plane". That was the T1-01 defect and `53695936` removed it: Hover now takes the layered builder, so the flat all-`Ground` path is not produced for it at all. The L1 half is settled by T1-01's own crossings, whose mid-span assertion (`!mid_span.is_empty()` plus the height invariant on every deck frame) covers *along*, not just *onto*. **Still open — L5 only:** `hover_vertical_tick`'s `climbing` input (`movement_tick.rs`) reads raw `ground_level`, so the lift controller sees no 4-level rise. That is a visual/lift question, not a legality one, and no test looks at it. Do not re-investigate the layer half. **Residual R-T102.** *Trigger:* every hover deck entry and exit. *Player effect:* the lift controller sees no 4-level rise, so a Robot Tank or amphibious transport may mount the deck without its hover bob/tilt animating the climb — a cosmetic mismatch at the ramp, not a positional one; sim height is correct on every observed frame. *Frequency:* every hover bridge crossing. *Settling check:* a frame comparison against retail, not more code reading — and note the render-side height is itself unexamined (gap 7), so this cannot be settled from sim state alone. |
| T1-03 | Drive | high intact | onto | move | **VERIFIED-OK** | BayOPigs.mmx `(111,152)`→`(111,151)`; Hills.mmx `(98,74)`→`(97,74)`, both LOOSE | OI-04, OI-12 | **Settled 2026-08-27 at `7b66b073`.** The named check was run: hierarchy gate ON, no disabled helper (it no longer exists), ordinary `Command::Move` — which the harness now panics on if refused. `tank_crosses_bay_of_pigs_high_bridge_at_deck_height` and `tank_crosses_hills_high_bridge_at_deck_height`, literal `test result: ok. 4 passed; 0 failed`. Every structural deck frame asserts `on_bridge` set and `z == terrain + BRIDGE_DECK_LEVEL_DELTA`. |
| T1-04 | Drive | high intact | along | move | **VERIFIED-OK** | BayOPigs.mmx col x=111, 17 deck cells; Hills.mmx row y=74, 22 cells | OI-10, OI-11, OI-19 | **Settled 2026-08-27, same run as T1-03.** The harness asserts a non-empty mid-span frame set and the height invariant on *every* deck frame, over 17 and 22 deck cells. Caveat kept honest: "mid-span" is the harness's own definition, not this row's "≥3 from either end" wording; the two agree on these spans but a future narrow span could separate them. Stands for the corpus of spans with ≥3 anchors. |
| T1-05 | Drive | high intact | off | move | **RESIDUAL** — crossing verified, occupancy plane unasserted | BayOPigs.mmx `(111,135)`→`(111,134)` | OI-20, OI-21 | **Residual R-T105.** *Trigger:* every drive-track that terminates on a deck cell. *Player effect:* unknown and possibly none — the mover leaves the deck correctly in all seven observed crossings; the concern is that `movement_tick.rs` hardcodes `MovementLayer::Ground` for the terminal commit, which is right off-deck and wrong for a track *ending* on the deck, so an occupant could be listed on the wrong plane. *Frequency:* every crossing, but only observable if something queries the occupancy plane of a stopped-on-deck mover — nothing in ordinary play obviously does. *Settling check:* assert the occupancy plane after a track terminates mid-span; the harness records cell, z, on_bridge and BridgeOccupancy, never the plane, so this needs an instrument change, not another run. Original note follows. **Half observed, deliberately not upgraded.** The same run shows the exit frame clearing `on_bridge` with `bridge_occupancy` gone. The other half of the named check — that the occupancy entry lands on the Ground plane — is asserted nowhere: `movement_tick.rs:188-232` hardcodes `MovementLayer::Ground` for the terminal commit, which is right off-deck and wrong for a track that *ends* on the deck, and no test looks. A row is not VERIFIED-OK because most of it passed. |
| T1-06 | Walk | high intact | onto | move | **VERIFIED-OK** | Hills.mmx `(98,74)`→`(97,74)`; BayOPigs.mmx `(111,152)`→`(111,151)` | — | **Settled 2026-08-27, no code change needed.** Walk was UNCHECKED, not broken: `infantry_crosses_bay_of_pigs_high_bridge_at_deck_height` and `infantry_crosses_hills_high_bridge_at_deck_height` pass first time under ordinary undisabled orders, `test result: ok. 7 passed; 0 failed`. Worth recording that the row's premise was right — no infantry had ever been driven across a span here, and the native twin `WalkLocomotionClass::ProcessMovement` `0x0075C154`-`0x0075C199` was unexercised — the answer just turned out to be that it already worked. Original wording follows: Drive an `E1` across; no infantry has ever been driven across a span in this project. The harness drives only `MTNK` and `ROBO` (`movement_bridge_retail_tests.rs:1099,1107,1120`). The cited native twin `WalkLocomotionClass::ProcessMovement 0x0075C154-0x0075C199` has never been exercised. |
| T1-07 | Walk | high intact | along | move | **RESIDUAL** — crossing verified, sub-cell plane unasserted | Hills.mmx row y=74; BayOPigs.mmx col x=111 | OI-19 | **Half settled 2026-08-27.** The crossing itself is observed: `infantry_crosses_hills_high_bridge_at_deck_height` and its BayOPigs twin pass, so an `E1` holds deck height across the whole mid-span run. But this row's *named* check is that the **sub-cell reservation** arm picks the Bridge layer, and no test asserts that — the harness records cell, z, on_bridge and occupancy, never the sub-cell slot's plane. Not upgraded: a row is not VERIFIED-OK because the run it rides on passed. Same discipline as T1-05. **Residual R-T107.** *Trigger:* every infantry step onto or along a deck. *Player effect:* if the sub-cell slot is reserved on the ground plane while the soldier stands on the deck, two infantry could contend for one slot across the two planes, or a soldier under a span could block one on it. Not observed — four infantry crossings are clean end to end. *Frequency:* every infantry bridge crossing, so common on bridge maps, but the effect needs two movers on the same cell's two planes to surface. *Settling check:* same instrument change as R-T105 — record the sub-cell slot's plane. Shared with T1-08 and T1-14. Original wording follows: Same run; additionally assert the **sub-cell reservation** arm picks the Bridge layer — infantry are the only movers that take it, and it is the one Walk-specific bridge code path. |
| T1-08 | Walk | high intact | off | move | **VERIFIED-OK** for the exit; sub-cell release UNCHECKED | Hills.mmx `(76,74)`→`(75,74)`; BayOPigs.mmx | — | **Settled 2026-08-27, same runs as T1-06.** Both infantry crossings run approach-to-approach and `drive_across_high_bridge` asserts arrival at the far approach, so leaving the deck is observed rather than inferred, and the height invariant holds on the off-bridge frames too. The second half of the original check — that the sub-cell slot is released on the Bridge plane — is not asserted, same gap as T1-07. |
| T1-09 | Drive | low intact | along | move | **VERIFIED-OK** | Lostlake.mmx row y=117, x=39..51 (13 cells), LOOSE — already in the harness map list | OI-07 | **VERIFIED-OK 2026-08-28.** Named check run in full on the named fixture, which was re-derived by enumeration rather than trusted: ordinary undisabled `Command::Move` (38,117)→(52,117), 15-node path accepted, stood on 13/13 deck cells, arrived. Second geometry on `Killer.mmx` (22 cells, north–south). `tank_crosses_lostlake_low_bridge_at_ground_height` + Killer twin, `test result: ok. 14 passed; 0 failed`. **The invariant is not the high-span one** — a low deck has no `0x100` stamp, `bridge_deck_level == ground_level`, and both approaches sit at deck level, so there is no height event at all: `z == ground && !on_bridge && bridge_occupancy == None`. The `!on_bridge` half is load-bearing, not decoration — asserting only `z == ground` would pass an implementation that set `on_bridge` and then floated the mover 4 levels over flat ground. Arrival alone would prove nothing either, since a low deck reads as ordinary ground in every field the mover consults; the discriminator is that the middle of each span is pre-overlay **water**, and the test asserts the mover was over every axis position of that gap. **Collapse rationale corrected:** this stands for wood/concrete not because both take `TubeClass` (neither does — see gap 12) but because both are ordinary ground plane. |
| T1-10 | Walk | low intact | along | move | **VERIFIED-OK** for the crossing; sub-cell plane unasserted | Lostlake.mmx y=117 x=39..51 | OI-07 | **VERIFIED-OK for the crossing 2026-08-28; the row's own stated arm turned out unreachable.** `E1` crossed both maps, 13/13 and 22/22 deck cells, full water-gap coverage, invariant clean over 373 and 587 frames. But this row existed because `tube_movement.rs` gates on category `Unit \| Infantry` — and that file is not on the stock low-bridge path at all (gap 12), so the arm was neither exercised nor exercisable. Infantry is still separate from T1-09 for a different reason: the **sub-cell reservation plane**, which remains unasserted here exactly as at T1-07 and T1-08. |
| T1-11 | Hover | low intact | along | move | **VERIFIED-OK** — weaker than T1-09/10, see cell | Lostlake.mmx y=117 x=39..51 | — | **VERIFIED-OK 2026-08-28, and deliberately weaker than T1-09/T1-10.** `ROBO` is `AmphibiousDestroyer`, so the river is not an obstacle to it and the water gap cannot prove it *needed* the bridge. Measured: on Lostlake it stays on all 13 deck cells and covers all 7 gap positions; on **Killer it leaves the deck at (93,136), hovers open water to (93,143), and rejoins at (93,144)** — legitimate amphibious routing, with the invariant holding on the water frames too. So this row settles order-accepted, ground height with no `on_bridge`/`BridgeOccupancy` on every deck cell it used, and arrival — **not** that the span was required. The test asserts that weaker requirement explicitly for amphibious movers rather than pretending to the stronger one. Two earlier rationales died before this: the "fails on high spans" asymmetry (T1-01 fixed it) and the `tube_movement.rs` layer gate (gap 12 — that file is not on this path). |
| T1-12 | Hover | high intact | off | move | **VERIFIED-OK** | BayOPigs.mmx `(111,135)`→`(111,134)`; Hills.mmx | OI-06 closed | **Settled by T1-01's own crossings, 2026-08-27.** This row read "unreachable until T1-01 clears" — it has cleared. Both Hover crossings run approach-to-approach, so the mover leaves the deck as part of the assertion set, and `drive_across_high_bridge` now pins arrival at the far approach: a mover that entered the span and failed to leave it fails the test. Off-deck is therefore observed, not inferred. |
| T1-13 | Drive | high intact | under | move | **VERIFIED-OK 2026-09-15** (Hills); NOT-APPLICABLE (BayOPigs) | Hills.mmx, ground plane at `(76,74)…(97,74)`, terrain level 2, LOOSE | OI-14 (=R5) | **SETTLED 2026-09-15 by an ordinary retail crossing.** `tank_ordered_under_hills_high_bridge_crosses_all_four_ground_lanes`: `MTNK` `(87,71)→(87,78)`, path 8, 88 frames, 38 under-span frames on all four lanes, 0 deck frames, invariant held. Native contract: `UnitClass::Can_Enter_Cell 0x0073F0A0` (vtable `0x007F5C70+0x1AC`) never calls `CheckCellPassability 0x004834A0` — its xref list is CellRect, threat scan, placement, paradrop, overlay Mark, Jumpjet touchdown — and admits base-height ground beneath a span through `CheckBridgeTraversal 0x004D9C60`'s equal-level arm, selecting the ground list `+0xE4` at `0x0073F51A` and reading the terrain's own land row at `0x0073FAB5`. Rust: the `cell_entry.rs` class arm now covers every ground-layer mover, and `TerrainCostGrid::ground_cost_at` supplies the row beneath the deck so water under a span still refuses Track. The UNCHECKED caveat below is closed. Original diagnosis retained: Add `facts.ground_walkable` to `print_inventory` (`movement_bridge_retail_tests.rs:196-228`) and re-run. **SETTLED 2026-08-28: the order is REFUSED outright — not routed over the deck, not routed around.** `path=None`, no `MovementTarget`, for all three locomotors on both fixtures. Pinned by `tank_ordered_under_hills_high_bridge_is_currently_refused` (characterization — it asserts today's wrong behaviour and goes red when fixed). Nothing about the terrain stops it: under Hills' band at x=87 the ground is ordinary land, `zone_type=0`, all costs 100, in the same Normal zone (2) as the valley floor on both sides, and a mover placed there holds the correct state on 48 frames. Ablation isolates one input — `blocker_neighbor_counts`, i.e. the same hierarchy branch that caused the original bridge bug: flat A* returns `Some(8)`, layered A* returns 8 Ground steps straight under, `find_move_path[all]` → `None`, `[all − neighbors]` → `Some(8)`. Two mechanisms measured: `cell_rect.rs` `evaluate_is_clear_to_move` refuses the ground plane of any `0x100` cell (`has_bridge && !is_bridge → LevelMismatch`), which three of the band's four lanes escape only via the `bridge_transition` short-circuit — so VERA's answer differs *across lanes of one bridge*; and the hierarchy gate's deck exemption is keyed on the mover already carrying deck height, so an under-span step never qualifies. **Frequency:** any order whose shortest route crosses a span footprint on the ground plane; 61 of 184 stock maps carry a high span, and on Hills the whole north–south valley traffic through the bridge line is affected — the player gets a refused order, not a detour. **BayOPigs arm is NOT-APPLICABLE:** its riverbed is water with Track/Foot cost 0, so a tank could never be there regardless (`tank_cannot_reach_the_riverbed_beside_bay_of_pigs_high_bridge`). **Formerly UNCHECKED (closed 2026-09-15):** whether gamemd refuses ground-plane entry on a stamped cell at all — it does not; `0x0073F0A0` admits it, see the settlement at the head of this row. Consequence recorded for the T2 damaged-span rows: beneath a *collapsed* span the structural stamp is retained while the deck is no longer elevated, so `ground_cost_at == cost_at` there and Drive is admitted on the ground plane by the same rule; that matches the native reading (`0x004D9C60` and the land row decide, `0x100` only selects the plane). |
| T1-14 | Walk | high intact | under | move | **VERIFIED-OK 2026-09-08** (Hills); NOT-APPLICABLE (BayOPigs) | Hills.mmx ground under y=74 (land); BayOPigs.mmx x=111 (water) for the amphibious sub-case | OI-14 | **SETTLED 2026-09-08 by an ordinary retail crossing** (`infantry_ordered_under_hills_high_bridge_crosses_all_four_ground_lanes`, Infantry `+0x1AC 0x0051BF90` / `+0x1B0 0x004D9C60`, see `RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md`); this row was not updated at the time. Earlier record: **2026-08-28 — same cause as T1-13, re-run independently rather than assumed.** `infantry_ordered_under_hills_high_bridge_is_currently_refused` exercises `MovementZone::Infantry` with the sub-cell view on, and produces the identical single-input ablation: the same 8-node route exists, the same `blocker_neighbor_counts` flip refuses it. Read T1-13's cell for the mechanism, frequency and the parity caveat (closed 2026-09-15). Still riding along unasserted: the infantry **sub-cell reservation plane** on the under-span frames — the same instrument gap as T1-07 and T1-08. |
| T1-15 | Hover | high intact | under | move | **VERIFIED-OK 2026-09-15** | BayOPigs.mmx x=111 water riverbed under the span | OI-14, OI-06 closed | **SETTLED 2026-09-15 by an ordinary retail crossing.** `hover_tank_ordered_under_bay_of_pigs_high_bridge_crosses_all_four_ground_lanes`: `ROBO` `(108,143)→(115,143)`, path 8, 84 frames, 41 under-span frames on lanes `x=110..113`, 0 deck frames, invariant held. Same native contract and Rust change as T1-13. Original record: Order a `ROBO` (AmphibiousDestroyer) along the riverbed under the span. **Rationale rewritten 2026-08-27:** this row used to be framed as deciding "whether L1 is a nuisance or the reason ROBO never reached the deck, because the flat-branch planner routes it under the span". T1-01 settled that — the cause was the Ground-plane terrain test, the flat branch no longer sees Hover, and under-routing was never observed. What remains is the genuine question: can an amphibious mover legitimately travel the riverbed beneath an intact span, and does it hold ground height while doing so? **SETTLED 2026-08-28, and this is the cleanest of the three.** For `ROBO` the riverbed either side of BayOPigs' span genuinely *is* passable — Hover cost 100, AmphibiousDestroyer ground zone 2, ground entry admitted on all eight side cells — so unlike T1-13 there is no terrain excuse at all, and the order is **still refused**. Its production-config goal walk is stronger evidence than Hills': every band cell *and the far bank* return `None`, so there is no route of any kind, not even the over-the-deck detour. Pinned by `hover_tank_ordered_under_bay_of_pigs_high_bridge_is_currently_refused`. Mechanism, frequency and the UNCHECKED parity caveat are in T1-13's cell. |

### Tier 2 — ramps, damaged spans, and non-player order sources

| ID | Loc | Kind | Rel | Src | Disp | Fixture | Open | The one check that settles it |
|---|---|---|---|---|---|---|---|---|
| T2-01 | Drive | high intact | onto | atk-mv | **VERIFIED-OK** (2026-09-15) — `tank_attack_moved_across_{bay_of_pigs,hills}_high_bridge_crosses`, `infantry_attack_moved_across_hills_high_bridge_crosses`; the drop this row recorded is gone on `main` (probe: 24/19 cells occupied over the run, deck included; plain-Move control crosses too). The hut-pricing half of the settling check is still open. | BayOPigs.mmx x=111 span | OI-15 | Attack-move a group across the span and assert (a) it crosses, (b) it does **not** select a bridge repair hut as a path obstacle. VERA has four consumers of `bridge_repair_hut` and none is a passability or A* cost test; gamemd returns `MOVE_NO` unconditionally (`0x0051C62F` / `0x0073FC00`). If VERA prices the hut as ordinary destroyable, an attack-moving force cuts its own bridge. Stands for Walk. |
| T2-02 | Drive | high intact | along | AI | **RESIDUAL** — no longer blocked (T2-01 settled), unrun | Hills.mmx y=74 span | OI-08 | Scripted team move across the span. `ObjectClass::ShouldBeOnBridge 0x005F6A70` feeds zone reachability from the **destination** height; VERA passes the raw mover layer (`movement_bridge.rs:1105-1108`, `#[ignore]`d, panics "unimplemented"). A team goal on the far side is exactly the ≥4-level destination that triggers it. |
| T2-03 | Drive | high intact | along | bump | **VERIFIED-OK** | BayOPigs.mmx x=111, block the span mid-crossing with a second unit | — | Block a deck cell in front of a crossing tank and let `try_repath_after_block` (`movement_path.rs:561`) run. It is untested on a deck anywhere in the tree. Assert the repath stays on the Bridge layer instead of dropping to Ground. |
| T2-04 | Hover | high intact | onto | atk-mv | **VERIFIED-OK** (2026-09-15) — `hover_tank_attack_moved_across_bay_of_pigs_high_bridge_crosses` | BayOPigs.mmx x=111 | OI-06 closed | **The reason this row was separate is gone.** It read "Hover takes the flat branch, where `is_bridge_only_goal` can drop the order outright". `is_bridge_only_goal` is reachable only when `layered_pathing == false`, and since `53695936` Hover is layered, so that predicate no longer sees a Hover mover at all. Its other half — attack-move goal selection being a different code path from a plain move — is now measured: Drive, Walk and Hover attack-moves have all crossed (2026-09-15), so the row could fold into T2-01; it is kept only as the named Hover run. |
| T2-05 | Drive | high ramp/bridgehead | onto | move | **RESIDUAL** — ramp traversed in all 7 high crossings, `rejected_reason` unasserted | BayOPigs.mmx `(111,151)` and `(111,135)`; Hills.mmx `(97,74)`/`(76,74)` — the anchor / F1 / Opposite slots that carry `0x200` | OI-11, OI-12, OI-04 | Log `rejected_reason` for a step onto a bridgehead cell. Three separate ports converge here: `cell_entry.rs:442-452` short-circuits the object-list and speed-row half of the entry test on any transition cell; `core.rs:638-644` makes whole-deck `0x200` load-bearing for the diff-0 arm; `is_at_bridge_level` reads `bridge_walkable` where native reads `0x100`. |
| T2-06 | Walk | high ramp/bridgehead | onto | move | **RESIDUAL** — same as T2-05, infantry arm | Hills.mmx `(97,74)` | OI-12 | Same as T2-05 with an `E1`; infantry additionally carry the sub-cell reservation across the transition cell. |
| T2-07 | Drive | low intact | along | AI | **KNOWN-BROKEN** | Lostlake.mmx y=117; any map with an authored `[Tubes]` section | OI-07 | `tube_hierarchy_pairs_are_unregistered` (`zone_build.rs:3041-3045`) `panic!`s "unimplemented: tube branch of `RegisterBridgeOrTubeHierarchyPairs 0x00582D70`". The high-bridge half of the same function **is** ported. Settled by implementing the tube branch and asserting a long cross-map order routes through the tube instead of around it. |
| T2-08 | Drive | high damaged (body byte 6) | along | move | **RESIDUAL** — no loose damaged-high fixture | **MIX**: `all06umd.map` `(37,34)-(41,34)`, `c3y01md.map` `(59,99)-(65,99)`, `xmp04t8.map` `(163,192)-(167,192)`. No loose map carries byte 6. | — | Extract one MIX map to the retail root, then order a crossing over the damaged body cell. `Damaged(NS)` is 7 cells across the whole 184-map corpus — rare but author-placed and real. |
| T2-09 | Drive | high partially collapsed | onto | move | **VERIFIED-OK** | Deadman.mmx y=41: intact `(55,41)`, collapse stub `(56,41)` state 8, gap x=57..60, stub `(61,41)` state 7. LOOSE | OI-28 | Order across the gap. Correct behaviour is a refusal or a route around, never a drive into the gap. Deadman and YuriPlot are the only loose maps with author-placed collapse states. |
| T2-10 | Drive | low damaged/destroyed | along | move | **VERIFIED-OK** | Shrapnel.mmx `(106..108, 46..48)` and `(114..116, 58..60)` — ids 82/100/81 and 90/101/91, where `0x64/0x65` are the terminal sinks of the wood damage table. LOOSE | OI-29e | Order a crossing through the destroyed middle strip. This is the only loose map in the corpus with author-placed non-pristine low-bridge ids. |
| T2-11 | Drive | high repaired | along | move | **KNOWN-BROKEN** | Any high-wood map with a CABHUT; Hills.mmx or Carville.mmx | OI-29 | `bridge_repair_terrain_restoration_is_unported` (`bridge_state/tests.rs:2708-2712`) is `#[ignore]`d: `cell+0x11B += 4` is never re-applied and `ValidateBridgeZones 0x0056DB70` + `RebuildZoneConnectivity` never run. Repairs are uncommon per match but the effect **persists for the rest of the game** and both players then path over that span. Blocked: the branch selectors are runtime BSS theater constants that cannot be read statically — recorded rather than guessed. |
| T2-12 | Drive | high intact | off | retreat | **RESIDUAL** — order source unexercised | BayOPigs.mmx `(111,135)` under fire | — | Damage a unit mid-span so it takes the retreat mission and observe the exit. The concern is not the order source itself but that a mission-driven repath re-enters `movement_commands.rs:288-289`, which hardcodes `path_layers: vec![current_layer, current_layer]` for direct two-node targets (OI-24). |
| T2-13 | Drive | high intact | along | guard | **RESIDUAL** — order source unexercised | Hills.mmx y=74 | — | Put a unit on guard beside a span and give it a target across; assert the pursuit crosses rather than stalling at the bridgehead. No observation exists for any order source other than a single ordinary Move. |
| T2-14 | Drive+Walk | high intact | alongside | move | **RESIDUAL** | BayOPigs.mmx `x ∈ {109,110,112,113}` at `y=143` — the F2/Opposite structural slots that are stamped but off the drive line | OI-30c | **Trigger:** any ground unit standing on a structural bridge cell that is not the deck drive line. **Player effect:** the under-bridge SHP pass writes depth (`merge_passes.rs:215`, `LessEqual`, write ON) where the ordinary ground pass does not (`Always`, no write), so an infantryman one cell inside the structural band can clip later voxel units while the same man one cell outside does not. **Frequency:** uncommon, but gated on nothing rare — any bridged map, any unit walking past the abutment. Render-side; deliberately not fixed while sim rows are open. |

#### Tier 2 settlement, 2026-08-28

Every Tier 2 row is now dispositioned. Eight new retail tests; the whole retail module runs
`test result: ok. 29 passed; 0 failed`, full suite `7509 passed; 0 failed`.

**The headline of the first pass is resolved.** `T2-01`/`T2-04` originally recorded an
attack-move across a span being dropped on the tick after it was issued — for Drive, Walk and
Hover, on both geometries — while the route built fine, an order-source defect a bridge merely
revealed. On 2026-09-15 the four `..._is_currently_dropped` characterizations went red on
`main`: the probe reported the mover crossing the span under the attack-move (Hills/MTNK
24 cells occupied over 245 surviving ticks, Hills/E1 24, BayOPigs/MTNK and /ROBO 19), with the
plain-Move control crossing as well. They are rewritten as positive crossings through the shared
driver with the attack-move order source (`..._attack_moved_..._crosses`). Which change on
`main` lifted the drop was not bisected; the rows are held by the crossing tests from here on.

**`T2-02` (AI team) — RESIDUAL, no longer blocked.** An AI team's cross-bridge movement is
issued through the same non-player order machinery; with T2-01 settled it can be measured on
its own. *Trigger:* any AI team routed over a span. *Player effect:*
AI armies fail to use bridges. *Frequency:* every skirmish against AI on a bridge map — but the
AI opponent is deferred project-wide, so nobody sees it yet. *Settling check:* one scripted
team run across a span.

**`T2-03` (bump / repath) — VERIFIED-OK.** Two tanks, one span: the first is parked mid-deck,
the second ordered across into it. Asserted that the blocker genuinely reached the deck and
stopped at deck height, that every frame of the crosser obeys the native height model, and that
no node of the crosser's live `path_layers` landing on a structural cell is `Ground` — i.e. the
repath stays on the bridge plane instead of dropping to ground. This is also the project's only
observation of a drive track that *terminates* on a deck cell, which is residual R-T105's
trigger.

**`T2-09` (partially collapsed) — VERIFIED-OK.** Ordered onto the stamped stub on the far side
of a collapse gap, where every bridge-line route crosses cells whose deck is missing and whose
ground is four levels down. The mover never drives into the gap.

**`T2-10` (low destroyed) — VERIFIED-OK.** A destroyed low span is not crossable.

**`T2-05`/`T2-06` (ramp / bridgehead) — RESIDUAL.** Every one of the seven high crossings
traverses a ramp at both ends, so the geometry is exercised on every run — but the rows' named
check is the `rejected_reason` on a bridgehead step, and no test asserts a rejection reason.
*Trigger:* every bridge approach. *Player effect:* none observed; three separate ports converge
on these cells and a divergence would show as a refused or mis-planed approach step.
*Frequency:* constant on bridge maps. *Settling check:* assert `rejected_reason` on a
bridgehead step — an instrument change, not another run.

**`T2-08` (damaged high span) — RESIDUAL, fixture gap.** `retail_damaged_bridge_inventory`
found no loose retail map carrying a *damaged* high span; damage is a runtime state, not map
data. *Settling check:* damage a span in-scenario and re-run a crossing, or extract a fixture
from MIX. Named as a fixture gap rather than a silent skip.

**`T2-12`/`T2-13` (retreat, guard) — RESIDUAL.** Neither order source has ever been exercised
over a span. §2 records that order source is selected *above* the planner entry the locomotors
share, so a plain Move crossing is evidence about the planner but not about these. *Trigger:*
a retreating or guarding unit whose route crosses a span. *Player effect:* unknown; T2-01's first
pass showed an order source can be dropped entirely while the route builds fine, so this family
is not safely collapsible onto the Move rows. *Frequency:* retreat is common under fire; guard-area
pursuit is common on defensive lines. *Settling check:* one probe each, same shape as the
attack-move probe.

### Tier 3 — Ship, Teleport, and destroyed-span edges

Expected to end largely as residuals. Kept listed so they are not rediscovered.

| ID | Loc | Kind | Rel | Src | Disp | Fixture | Open | The one check that settles it |
|---|---|---|---|---|---|---|---|---|
| T3-01 | Teleport | high intact | onto | move | **KNOWN-BROKEN** | BayOPigs.mmx `(111,143)` as a warp destination | L3 (new) | `TeleportPhase::Relocate` (`teleport_movement.rs:348-390`) writes `rx/ry`, centres the sub-cell, clears `exact_z_leptons` — and **never writes `position.z`, never touches `on_bridge`**, and passes the *source* `loco.layer` into `occupancy.move_entity`. A `CLEG`/`CMIN` warping onto a deck keeps the riverbed height. Settled by adding the deck term and asserting `z == terrain+4, on_bridge=true` after a warp onto a structural cell. Stands for the Drive-piggyback arm, which converts to an ordinary Drive move. |
| T3-02 | Teleport | high intact | off | move | **KNOWN-BROKEN** | BayOPigs.mmx `(111,143)` → a ground cell | L3 | Same defect, opposite sign: a unit warping *off* a deck keeps `on_bridge=true` and deck height, so it floats 4 levels above the ground. Same fix, separate assertion. |
| T3-03 | Ship | high intact | under | move | UNCHECKED | BayOPigs.mmx x=111 column, water riverbed under the 17-cell span. LOOSE | — | Order a `DEST` under the span and assert it passes. Ship shares the drive track with Drive (`movement_step.rs:48-53`) and `BRIDGE_Z_OFFSET` (416 leptons, `movement_bridge.rs:161`) widens its braking window when its current cell carries a deck. A deck row for Ship is excluded (X-04), so this is Ship's whole column. |
| T3-04 | Drive | high intact→collapsing | along | move | UNCHECKED | BayOPigs.mmx x=111 with a column crossing while the span is cut | OI-29c, OI-29d | Cut the span with traffic on it. `drop_in_bridge_deck_entities` (`bridge_orchestrator.rs:1760`) snaps deck occupants to ground level with health untouched, and `kill_ground_occupants_at` (`:1312`) explicitly filters `!e.is_on_bridge_layer()`. **LEAD only — TS-sourced, no gamemd citation exists.** The check is a Ghidra pass on the native collapse occupant sweep before any Rust is written. |
| T3-05 | Drive | low intact→destroyed | along | move | UNCHECKED | Shrapnel.mmx low span, destroyed under a crossing unit | OI-29e | Same shape one tier down: `walker.rs:509` produces `StateOutcome::Absorbed` for intermediate stages with no occupant effect and no `Kill_Illegal_Occupiers` analogue. Also LEAD-only. Ranked below T3-04 because it needs the low-bridge damage path working first (T2-10). |
| T3-06 | Walk | high destroyed | onto | move | UNCHECKED | YuriPlot.mmx stub `(111,78)` state 16 / `(111,83)` state 17. LOOSE | OI-28 | Order an `E1` onto a surviving collapse stub. Correct behaviour is whatever gamemd's re-stamp leaves passable — and VERA never re-stamps (`bridge_edge_tile_rewrite_after_collapse_is_unported`, `bridge_state/tests.rs:2654-2658`), so its predicates keep reading a span the engine has already unstamped. Recorded correction: this is **not** a tile-art gap; the writers touch bits the zone build, both walkers and `CheckBridgeTraversal 0x004D9C60` all read. |
| T3-07 | Drive | high intact | under | move | UNCHECKED | **MIX**: `all02umd.map` NEWURBAN elevated freeway over city streets (18 ground-unit / 22 building / 21 ore shared under-deck template pairs) | OI-14 | The city-overpass variant of T1-13. Only worth extracting if T1-13's `ground_walkable` print comes back true and the Hills case turns out to be map-specific. `c1a02md`, `rushhr`, `manhatta`, `austintx`, `downtown`, `c4w01md` are the same family. |

**Row count: 36.** Tier 1: 15 · Tier 2: 14 · Tier 3: 7.

---

## 4. Non-order entries onto a deck — recorded, not matrix rows

These are real defects in the same family as T3-01, but none of the six order sources applies to
them, so they are recorded here rather than padding the matrix.

| ID | Item | Disposition | Evidence |
|---|---|---|---|
| N-01 | `spawn_object` has no bridge-deck term (`world_spawn.rs:354`, `height_map…unwrap_or(0)`) | **KNOWN-BROKEN** (OI-22) | MEASURED: spawning a Grizzly at BayOPigs `(111,143)` gives `z=1, on_bridge=false` on a cell whose deck is at 5 — literally under the bridge — and it then paths nowhere in either gate state. |
| N-02 | Map-placed units on a deck are placed at riverbed height | **KNOWN-BROKEN** (OI-22) | `austintx.map` (MIX) is the only map in 184 setting `[Units]` field 10 (`High`) to `1`; all **76** such units stand on stamped structural cells. `parse_common_fields` (`map/entities.rs:280-330`) reads fields 0-5 and 8-9 and **never reads field 10**. Field identity is inferred, not read from a VERA parser — but no unit with `High=0` lands on a structural cell anywhere in the corpus. |
| N-03 | No air-layer landing path writes bridge state | UNCHECKED (L4) | `air_movement.rs`, `jumpjet_movement.rs` and `parachute_descent.rs` contain **zero** occurrences of "bridge". A paradropped GI or a landing SHAD on a deck cell gets N-01's symptom. Deck landings are rare; paradrops are not. |

---

## 5. Fixtures — the frozen index

| Fixture | Map | Cells | Deck / approach level | State |
|---|---|---|---|---|
| High concrete, intact | **BayOPigs.mmx** LOOSE, URBAN | anchors `(112,135)-(112,151)`; engine drive line `(111,134)`→`(111,152)`, 17 deck cells. Second span anchors `(160,135)-(160,151)` | terrain 1, deck-Z 5, approach 5 | overlay 25, state byte 9 = `Healthy(EW)`. **Riverbed is water** (0 of 9 under-deck templates ore-capable). VERIFIED-BY-ENGINE. |
| High wood, intact, **land under** | **Hills.mmx** LOOSE, TEMPERATE | anchors `(76,75)-(97,75)`; drive line `(75,74)`→`(98,74)`, 22 deck cells | terrain 2, deck-Z 6, approach 6 | overlay 237, byte 0. 10 of 13 under-deck templates carry ore elsewhere on the map ⇒ ground is land at level 2. VERIFIED-BY-ENGINE for the span; **the land claim is map-byte-derived, engine-UNCHECKED** (§7 gap 1). |
| High, destroyed at load | **YuriPlot.mmx** LOOSE | stubs `(111,78)`/`(111,83)`, `(135,131)`/`(135,137)`, `(79,110)`/`(84,110)` | — | states 16/17 and 8/7 = `PartialCollapse`; no overlay in the gap. |
| High, partially collapsed | **Deadman.mmx** LOOSE | y=41 and y=90 rows; intact + stub + gap + stub | terrain 0, approach 4 | states 0 / 8 / 7. |
| High, `Damaged` body | **MIX only** | `all06umd.map`, `c3y01md.map`, `xmp04t8.map` | — | byte 6; 7 cells corpus-wide. Extraction is a slice prerequisite. |
| Low wood, intact | **Lostlake.mmx** LOOSE (already in the harness map list) | y=117, x=39..51 (13 cells) | level 1 throughout, no `+4` | end id 94, body 74-77. Alternatives: Killer.mmx x=93 (22), Shrapnel.mmx y=74 (13), EB3.mmx y=101 (13). |
| Low wood, damaged/destroyed | **Shrapnel.mmx** LOOSE, SNOW | `(106..108,46..48)` ids 82/100/81; `(114..116,58..60)` ids 90/101/91 | — | `0x64`/`0x65` are the terminal damage sinks. Only loose map with author-placed damaged low bridges. |
| Low concrete (`LOBRDB*`) | **MIX only** — `c2s02md.map` x=129 (25 cells), `nearoref.map` x=72 (20), `xmp13*.map` | — | Collapsed into the low-wood rows (§2); listed only if a family-specific defect appears. |
| Units pre-placed on a deck | **austintx.map** MIX, NEWURBAN | 76 civilian vehicles on four freeway spans | terrain 0, approach 4 | N-02's fixture. |
| Under-deck passable, city variant | **all02umd.map** MIX, NEWURBAN | elevated freeway over streets | — | T3-07. |

**Corpus context** (full sweep, 184 maps, 0 parse failures): 61 maps carry a high span, 59 carry a
low span, 15 carry both, 79 carry neither. 258 of 301 anchor runs are ≥3 anchors, i.e. long enough
to have a mid-span. Author-damaged high bridges: 58 cells across 15 maps. Bridge behaviour is
on-screen in ordinary skirmish, not an edge case.

---

## 6. Excluded — combinations that cannot occur in stock YR

One line each. These are **not** untested rows; do not add them.

| ID | Excluded combination | Why |
|---|---|---|
| X-01 | Fly (ORCA, HORNET, BEAG, BPLN, SPYP, PDPLANE, CARGOPLANE, ASW) × any bridge relation | `MovementZone=Fly` ⇒ `can_use_bridges() == false`; air-layer entities are filtered out of the ground movement loop at `movement_tick.rs:1657-1661` before it starts, and `air_movement.rs` contains zero "bridge". |
| X-02 | Jumpjet (Rocketeer, Kirov, DISK, SHAD, HIND, SCHP, SCHD, LUNR) × any bridge relation | Same `Air`-layer exclusion. The `[LocomotorBeam]` warhead (Magnetron lift) is the one way a ground unit could acquire a Jumpjet locomotor, and `combat_weapon.rs:161-182` records that branch as unimplemented — so it cannot occur today either. |
| X-03 | Rocket (V3ROCKET, DMISL, CMISL) × anything | A Rocket-locomotor object is a projectile and is never given a move order. |
| X-04 | Ship × any **on-deck** relation | `MovementZone::can_use_bridges()` (`locomotor_type.rs:377-386`) returns false for `Water`/`WaterBeach`. A deck row for Ship is impossible, not untested. |
| X-05 | Any locomotor × **under** a low span | Low decks sit at the surrounding terrain level (`resolved_terrain.rs:3571-3593`); `bridge_walkable` is never set for a low bridge. There is no space beneath — the crossing *is* the ground plane. |
| X-06 | Mech locomotor × anything | No CLSID resolves to Mech (`locomotor_type.rs:75-99`); the GUID appears only in commented `;origional -` lines. It is nonetheless in the `supports_layered_bridge_pathing` whitelist — a dead branch. Since `53695936` admitted Hover, that whitelist has four arms of which **three** are live (Drive, Walk, Hover). |
| X-07 | Tunnel / subterranean × anything | TS legacy, absent from RA2/YR, no CLSID, pinned absent from retail INIs by `dormant_clsids_absent_from_retail_inis`. **Not the same thing as low-bridge `TubeClass` movement**, which is live and has rows T1-09/10/11, T2-07, T2-10, T3-05. |
| X-08 | DropPod / Parachute locomotor × anything | Inert; no CLSID. Paradrop descent carries its own `parachute_descent` state instead of displacing the locomotor (that surface is N-03, not a locomotor row). |
| X-09 | Train / rail bridges | YR ships no rail bridges. The ancestor's `Can_Repair_Bridge` has a bridge-or-train iso-tile match; match the bridge half only. |
| X-10 | Concrete × wood as separate movement rows; `Nesw` × `Nwse` as a relation | Collapsed per §2 — the family field has no behavioural consumer. |
| X-11 | Low-bridge end pieces `LOBRDGE1-4` (ids 122-125) and `LOBRDGB1-4` (233-236) | **Zero cells in the entire 184-map corpus.** TS heritage. |
| X-12 | `IsAvoidBridges` bridge-cost multipliers; threat-avoidance HS scoring; `IsTrain`/`IsPassive` A* gates | Never set true anywhere in the ancestor and absent from stock `rulesmd.ini`. Do not create a bridge-avoidance column. |
| X-13 | Fog of war × bridges | `FogOfWar` defaults false; shroud only. No bridge interaction exists to test. |
| X-14 | High-bridge state `0x0F = Damaged(EW)` and `Healthy` variants 1/2/3 | Absent from the entire corpus; reachable only through the runtime state machine. Synthetic-only, and the renderer re-derives healthy jitter anyway — not a movement row. |

---

## 7. FROZEN SCOPE

The matrix above is the program's whole scope. **A later discovery is a residual unless
correctness requires it.** Concretely:

- A defect found while working a row is recorded against that row as a linked open item and
  deferred, unless the row cannot be settled without it.
- New locomotor/kind/relation/order-source combinations are **not** added. If one turns out to be
  load-bearing, it replaces a row rather than extending the matrix, and the replacement is stated
  in the slice report.
- Refactors, cleanups and style edits are out of scope inside a slice, per `AGENTS.md`.
- The excluded set (§6) is closed. Re-deriving an exclusion is a symptom that the ledger was not
  adopted.

---

## 8. EVIDENCE GAPS

What **no lane has established.** These are the program's blind spots, not its backlog.

1. **Whether any retail map has passable ground under a high span.** Hills.mmx is
   **PLAUSIBLE-STRONG, engine-UNCHECKED**: 10 of 13 distinct terrain templates directly under its
   22-cell span carry ore elsewhere on the same map, and the method's two controls (BayOPigs and
   Grinder, both known-water riverbeds) correctly returned zero. But this is terrain-template
   identity, not an engine land-type read. The settling action is one line: add
   `facts.ground_walkable` to `print_inventory` and re-run. **T1-13, T1-14, T1-15 and T3-07 all
   hang on this**, and demote to Tier 3 if it comes back false.

   **MEASURED 2026-08-28 — and it came back the opposite way from the prediction.** Every deck
   cell on both high-bridge fixtures reports `ground_walkable`: **17/17 on BayOPigs.mmx, 22/22 on
   Hills.mmx.** The prediction that Bay of Pigs' riverbed is impassable water — recorded as
   VERIFIED in the reachability diagnosis's residual R5, which said an under-span route is
   "impossible on Bay of Pigs, whose riverbed is zone 0" — is not what the path grid reports.

   **Do not over-read this.** VERA models a bridge as terrain on the *same cell* as the ground
   beneath it, so `ground_walkable` on a deck cell is ambiguous between "the riverbed under the
   span is passable" and "the deck itself is passable on the ground plane".

   **CLOSED 2026-08-28 — and the 17/17 / 22/22 number is an ARTIFACT. Do not cite it again.**
   The ambiguity above resolves to the *second* reading:
   `PathGrid::from_resolved_terrain_with_bridges` (`sim/pathfinding/core.rs`) hardcodes
   `ground_walkable = true` for any intact structural cell, commented "Intact bridge deck →
   walkable (overrides underlying terrain)"; `TerrainCostGrid::from_resolved_terrain` does the
   same, `COST_NORMAL` for every SpeedType. Both flags describe the **deck**. Reading them told
   us nothing about the riverbed, which is exactly the failure mode this gap was opened to
   avoid — a flag read produced the original wrong prediction and then produced a wrong
   correction to it.

   The real geometry, measured by perpendicular transect
   (`retail_under_high_span_geometry`): the structural band is **4 cells wide**, not the single
   drive line. Under BayOPigs' band at y=143 the terrain is water, `zone_type=4`, Foot/Track/Wheel
   cost **0** — impassable to `MTNK`/`E1`, passable to `ROBO`. Under Hills' band at x=87 it is
   ordinary land, `zone_type=0`, all costs 100, in the same Normal zone (2) as the valley floor
   either side.

   **The reachability diagnosis's residual R5 is also wrong** where it recorded as VERIFIED that
   BayOPigs' riverbed is "zone 0" and unenterable. Zone is a per-`MovementZone` answer: Normal
   ground zone 0, but AmphibiousDestroyer ground zone **2**.

   T1-13/14/15 were settled as `KNOWN-BROKEN` on 2026-08-28 and are now `VERIFIED-OK` by
   ordinary retail crossings (T1-14 on 2026-09-08, T1-13/15 on 2026-09-15) — see their rows.
   T3-07 (city overpass, MIX only) remains UNCHECKED.
2. **Whether the corridor-gate defect generalises beyond Bay of Pigs.** OI-30 is a **PREDICTION,
   never measured.** The repro established Bay of Pigs only, two bridges; the reachability
   diagnosis says so in its own §7/§9. The gate runs on every path build so the generalisation is
   expected — but "almost every map" is not a measurement and must not be cited as one. The
   instrument exists and has been run once for inventory (`retail_high_bridge_inventory`, output
   recorded), but no cross-bridge **order** has been issued on any map except Bay of Pigs.
3. **CLOSED for Drive and Hover on high spans; still open for Walk and for every low span.**
   This gap originally read "nobody has run a crossing". No longer true twice over: `7b66b073`
   removed the gate-disabling helper and the Drive crossings ran against the real install with
   nothing disabled, settling T1-03 and T1-04; `53695936` then fixed the Hover exclusion and
   added two Hover crossings on both map geometries, settling T1-01
   (`test result: ok. 5 passed; 0 failed`). Since `53695936` the shared harness also asserts
   arrival at the far approach, so all four crossings now prove completion rather than just
   deck entry — that retroactively strengthens T1-03 and T1-04 as well.
   What remains true and is the live half of this gap: **no crossing has been run for Walk on
   any span kind, and none for any low span by any locomotor.** Every "pinned by" for those rows
   is still a reading of an assertion body rather than an observation. The harness has still
   only ever driven two unit types, `MTNK` and `ROBO`.
4. **Walk on a high bridge has zero observations of any kind.** 60 stock types, structurally
   identical planner treatment to Drive, no test anywhere in the tree. The harness drives exactly
   two unit types ever: `MTNK` and `ROBO`.
5. **Order sources other than a single ordinary Move have zero observations.** Attack-move, AI
   team, scatter/bump, retreat and guard across a span have never been exercised;
   `try_repath_after_block` is untested on a deck.
6. **Four native questions that decide whether landed fixes are parity or coincidence** — all
   UNCHECKED, all need Ghidra: (a) is gamemd's zone comparison at `0x00429EA4` the same gate as
   `HierarchyGate`? (b) does `blocker_neighbor_counts` have a native counterpart at all — the
   module header (`zone_search.rs:26-28`) maps it to `CellClass+0x122` and the core comment says
   no counterpart is identified, **and the two in-repo texts contradict each other**; (c) does
   gamemd write `0x100` on low-bridge cells? — this decides whether "low bridges get no exemption"
   is drift or agreement; (d) does gamemd's deck-to-deck step consult `0x200` at all (OI-11)?
7. **Render-side deck height has never been examined by any lane.** Every `z` in every repro is
   the sim value. The matrix's acceptance condition is what the player sees, and
   `bridge_height_map` is read only by click-pick, target lines and the debug overlay — **no draw
   path uses it** (`render/.../instances/bridges.rs:162-165` anchors deck art to the ground height
   map plus fixed pixel offsets). One screenshot of a unit mid-span with the sim `z` logged
   alongside closes this.
8. **13 loose `.yro` maps were never swept** — `CrctBrd, DeepFrze, HighExpR, Ice_Age, IrvineCa,
   IsleLand, MojoSprt, MonsterM, MoonPatr, RiverRam, SinkSwim, Transylv, Unrepent`. These are the
   YR-exclusive skirmish maps; `.yro` is a MIX container the sweep script cannot read, but
   `map_file::load_from_path` handles them. `RiverRam.yro` ("River Rampage") is the obvious suspect
   for missed bridge content. The corpus counts in §5 are therefore lower bounds.
9. **UNMAPPED dimensions: none.** All three inventory lanes reported — locomotors, bridge kinds and
   fixtures, and known-open items. No matrix dimension was recorded as UNMAPPED.
10. **The diagnosis documents cite evidence lanes that are not in the repository.**
   `bridge-hover-t1-01-diagnosis.md` cites `gamemd.md`, `stall.md` and `repro.md` roughly 25
   times, including for whole tables it marks VERIFIED; none of the three is tracked or present.
   The same is true of the earlier bridge diagnoses. Effect: the measurements those documents
   rest on — "7 refusals", "2000 recorded frames", the native tables — cannot be audited by a
   later reader, only re-derived. Every status in them that was not independently re-read has
   since been demoted to UNCHECKED where it was load-bearing, but the pattern will recur unless
   investigation lanes write their evidence somewhere tracked. Not fixed here; recorded so the
   next program does not inherit the habit silently.
12. **The `TubeClass` premise this matrix was partly built on does not hold for the port.**
   Measured 2026-08-28 across 8 retail maps: `tube_index` is `None` and `low_bridge_tube_cell`
   is `false` on **all 689 low-bridge deck cells**. `build_auto_low_bridge_tubes`
   (`map/resolved_terrain.rs`) skips any cell whose `yr_cell_land_type` is not
   `YR_CELL_LAND_TUNNEL`, and a low-bridge *overlay* rewrites the cell's land to Road — so the
   gate never fires on stock data and `src/sim/movement/tube_movement.rs` is **not on the stock
   low-bridge path at all**. It is TS-shaped code reachable only from an explicit `[Tubes]`
   section. Three rows (T1-09/10/11) were justified by that premise and have been rewritten;
   `X-07`'s warning that TubeClass movement "is live" should be read as *the code exists*, not
   *stock maps use it*. **Open and needs Ghidra:** whether gamemd creates a tube record for
   these cells. If it does, the dead path is a DRIFT; if it does not, VERA agrees with the
   binary by accident. Until that is read, neither can be claimed.
13. **Line citations across these documents drift as the files they cite grow.** The T1-01 work
   added ~40 lines to one provenance block, so several `file.rs:NNN-NNN` spans in the matrix and
   the diagnoses now point a few dozen lines off, and at least one lands inside unrelated prose.
   Symbol names in the citations are still correct and are the reliable half. Prefer citing a
   function or item name with the line as a hint, not the reverse.
