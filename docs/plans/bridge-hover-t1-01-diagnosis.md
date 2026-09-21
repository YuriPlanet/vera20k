# T1-01 — Hover cannot enter an intact high bridge deck

Reconciles three lanes: `gamemd.md` (Ghidra, read-only, 2026-08-27 — outranks on native
behavior), `stall.md` (read-only Rust trace), `repro.md` (instrumented live measurement —
its observations outrank reasoning, including this document's).

VERA citations are `file:line` at HEAD `9b97b7c9`, each re-read while writing this and found
at the cited line. gamemd citations are the addresses the Ghidra lane decompiled this
session; they are marked VERIFIED on that lane's read, not re-derived here.

---

## 1. VERDICT

A Hover mover is stopped by VERA's own terrain-walkability test at the cell boundary, and it
is stopped there because the route it was given is labelled wrong. The path planner has a
whitelist of locomotors allowed to plan on the bridge plane; Hover is not on it, so the
Robot Tank gets the flat planner, whose fallback stamps every node of the route — deck cells
included — as "ground". The route itself is fine: A* found the same nineteen cells the tank
uses. But each tick, when the unit reaches the boundary of the first deck cell, the crossing
code takes that "ground" label at face value and asks whether the *ground under the bridge*
is walkable. Under this span it is a river, so the answer is no, the unit is snapped back to
the middle of its cell, and the same route is replanned. Nothing gives up, so it repeats
forever. The bridge legality check itself passes every time. VERIFIED — measured
(`repro.md` §0, §3, §5: 7 refusals, all `DENY=layer_walkable`, `grid_ok=Some(false)`, zero
`DENY=bridge_traversal`), and confirmed in code at `movement_step.rs:1986` reached through
the Ground arm at `movement_step.rs:1934-1968`.

---

## 2. WHAT THE PLAYER SEES

A Robot Tank ordered across an intact high bridge accepts the order — no error cursor, no
refusal — pivots toward the span, drives up to the abutment, and stops dead at the last cell
before the deck. It does not sit still: it accelerates into the edge and is reset to its
cell centre about thirteen times a second, so on screen it jitters at the bridgehead and
never advances a cell. The order is never dropped, so the unit is stuck until the player
clicks somewhere else; a large group will pile up behind it. VERIFIED — measured: 2000
recorded frames, every one at `(111,134)`, order still held at the end (`repro.md` §2, §5).
Frequency: **every attempt**, by any of the four stock Hover units, on any intact high
bridge. The Robot Tank is an ordinary Allied TechLevel-2 buildable (`Prerequisite=GAWEAP,GAROBO`,
`Cost=600`, `ini/rulesmd.ini` `[ROBO]`), and high spans exist on common maps — a player who
builds one and fights across a bridge map hits this in the first engagement. Low bridges are
unaffected: the ROBO crosses the Lostlake low span cleanly (goal reached, tick 157 —
`repro.md` §7), because a low deck genuinely lives on the ground plane, so the all-Ground
label happens to be correct there.

---

## 3. THE MECHANISM GAP

**No hover-specific bridge gate was found on the stages examined below.**

**Status corrected 2026-08-27 (critic round 3).** This section previously read "gamemd has no
hover-specific bridge gate anywhere on the path — VERIFIED (gamemd lane)". Two problems: the
claim is a whole-path negative drawn from five sampled stages, and its evidence is a lane report
that is not in the repository, so no later reader can audit it. Every address in the table below
is therefore **UNCHECKED here** — reported by the investigation lane, not re-derived at this
document's own hand, and explicitly *not* to be cited as resolving `Find_Path`'s `vtable+0x2CC`
gate (see the provenance block in `movement_path.rs`, which demotes exactly these three).

What *is* verified end to end, by direct reads recorded in `movement_path.rs`: the `+0x2CC` slot
binding, and that the one native gate on this path is a MovementZone reachability abort with no
locomotor term. The fix rests on that plus gate-deletion logic plus the production crossings —
not on the table below.

| stage | gamemd | Hover vs Drive |
|---|---|---|
| cell legality | `UnitClass::Can_Enter_Cell` @ `0x0073F0A0`, reached through mover vtable `+0x1AC` (`0x007F5C70+0x1AC`) | **same function**; Hover callsite `0x00515570`, Drive `0x004B34C0`, same `(cell, facing, height, 0, 1)` argument shape |
| bridge legality | `CheckBridgeTraversal` @ `0x004D9C60` (slot `+0x1B0`) | **locomotor-agnostic**: body reads only `Cell+0x11B` level, `+0x11C` ramp, `+0x140` bits `0x100`/`0x200`. No locomotor pointer, no `SpeedType`, no `MovementZone` term |
| the one locomotor-dispatched hook | `FootClass` passability check @ `0x004D9C10` → `ILocomotion+0x1C` | Drive vtable `0x007E7EB0+0x1C` and Hover vtable `0x007EACFC+0x1C` **both** resolve to `0x0055ABF0`, whose body is `return 0;` |
| destination Z onto the deck | `HoverLocomotionClass::Set_Destination` @ `0x00514D90` raises `dest.Z += 4 levels` when the destination cell has flag `0x100` | **unconditional**, no `OnBridge` precondition, no height precondition — same shape as Drive's @ `0x004AFD40` |
| terrain row | `LandType × SpeedType` speed table inside `Can_Enter_Cell` | Hover is nonzero on every row a deck reports; only `Rock` and `Wall` are 0% (`ini/rulesmd.ini` `[GroundTypes]`) |

Across the stages sampled above, the only Hover/Drive asymmetry the lane reported is *when* the
`OnBridge` byte flips (Hover on altitude reaching deck height, `0x00514944`; Drive on a
cell-level delta, `0x004B1830`) — a lift-timing difference, not an admission gate.
**UNCHECKED** — lane-reported, not re-derived here, and scoped to those stages: this is not a
claim that no other asymmetry exists in the binary.

**VERA's model, same path:**

| stage | VERA | effect on Hover |
|---|---|---|
| planner branch selection | `supports_layered_bridge_pathing` — **as it was before the fix**, `matches!(kind, Drive \| Walk \| Mech) \|\| on_bridge` | Hover excluded → flat branch. **Historical: `53695936` admits Hover, so this row describes the defect, not current behaviour.** |
| layer array | `build_flat_fallback_layers`, `movement_path.rs:539-558` — `vec![Ground; path.len()]` when `start_layer != Bridge` | all 19 nodes `Ground`, deck included (measured, `repro.md` §3) |
| runtime layer | `can_enter_layer_context`, `src/sim/pathfinding/core.rs:683-702` — copies `terrain_layer` **verbatim** from the planned layer, re-deriving only `occupancy_bits_layer` | `terrain_layer: Ground` while `object_list_layer`/`occupancy_bits_layer` are correctly `Bridge` (measured) |
| the refusal | `movement_step.rs:1986` on the Ground arm at `:1934-1968` → `cell_entry.rs:397-419` → `evaluate_shared_cell_leaf` bridgehead early return, `cell_entry.rs:442-453` → `HardBlocked` | `grid_ok = PathGrid::is_walkable` = raw `ground_walkable` = the riverbed = **false** |

**Did VERA invent a gate with no native counterpart? Yes — one, and its own provenance comment
already said so before this investigation started.** `supports_layered_bridge_pathing` selects a
pathing *plane* by locomotor kind.

What is **VERIFIED** about that, by direct reads recorded in the `movement_path.rs` provenance
block (not by this document, and not by any lane): the one native gate `Find_Path` consults is
`vtable+0x2CC` → `FootClass::CanReachDestination` @ `0x004D3810`, a MovementZone reachability
abort carrying no locomotor term. Scoped exactly that far — **at that gate** nothing selects a
plane by locomotor kind.

What is **NOT** claimed, and was wrongly claimed here before 2026-08-27: that there is no
locomotor-keyed branch anywhere on the native path. That was a whole-path negative resting on a
lane report which is not in the repository (matrix evidence gap 10), so it is unauditable as
well as unscoped. It has been withdrawn, not merely restated.

The fix does not need the stronger claim. It **deletes** a VERA-only restriction, and a deletion
needs the absence of a verified native gate demanding it — not affirmative proof that no such
gate exists anywhere. Its positive evidence is the two production crossings.

A second, milder invention rides along: `is_bridge_only_goal` (`movement_path.rs:101-121`),
whose comment states outright *"**VERA-internal, gamemd has no equivalent**"*. It is
consulted only on the flat branch, so **before `53695936`** it applied to Hover and not to
Drive; since the fix it sees neither. It is **not** what fires here — the goal `(111,152)` is an ordinary approach
cell, and the order was accepted (VERIFIED, measured) — but it is the same class of defect
on the same line of code.

---

## 4. RANKED CAUSES

### C1 — Locomotor whitelist excludes Hover, so the deck route is planned flat and labelled `Ground`. **CONFIRMED.**
- **For:** measured layer array is `Ground` on all 19 nodes for ROBO and `Bridge` on all 17
  deck nodes for MTNK over the identical cells (`repro.md` §3). Measured probe: at the same
  cell, with the same span, ROBO and MTNK return **byte-identical** verdicts —
  `layer=Ground: passable=false`, `layer=Bridge: passable=true` — so the planned layer is the
  only variable (`repro.md` §4). Code path re-verified: `movement_path.rs:91-94` →
  `:468` → `:521` → `:544-546`.
- **Against:** nothing.
- **Cheapest check:** already done. If more is wanted, one `log::warn!` of `target.layer_at`
  at `movement_step.rs:1874`.

### C2 — `check_bridge_traversal` refuses the hover mover. **REFUTED.**
- Refuted by measurement: all 7 refusals logged `traversal_allowed=true`, and the tally was
  `0 DENY=bridge_traversal` (`repro.md` §0, §5). The probe allows the step on *either*
  planned layer (`repro.md` §4). `stall.md` §2 also ruled it out analytically; the
  measurement is what settles it.

### C3 — `LevelMismatch` on a non-bridgehead deck cell (`cell_rect.rs` `has_bridge && !is_bridge`). **REFUTED for T1-01.**
- This was `stall.md`'s top-ranked sub-mechanism (its G2a). It cannot be reached here: the
  refusal fires at the **first** deck cell `(111,135)`, and every one of the 17 deck cells
  measures `transition=true` (`repro.md` §2), so `evaluate_shared_cell_leaf` takes the
  `bridge_transition` early return at `cell_entry.rs:442-453` and returns before any
  `evaluate_live_cell_passability` call. Refuted by measurement. It stays a live hypothesis
  for a map whose deck cells are not all transition cells — **UNCHECKED elsewhere**.

### C4 — the Hover `SpeedType`/`MovementZone` row refuses the deck. **REFUTED.**
- Refuted by the probe (`repro.md` §4): `Hover`/`AmphibiousDestroyer` and `Track`/`Normal`
  give the same answer on both layers. Refuted again by the DENY line itself:
  `grid_ok=Some(false) terrain_ok=Some(true)` — the raw grid bit is what fails, not the
  speed row. This also refutes `stall.md`'s G2b sub-mechanism as stated. Independently,
  gamemd's Hover row is nonzero on every LandType a deck reports (`gamemd.md` §4).

### C5 — hover turn-stall, throttle, or frame budget. **REFUTED.**
- Measured: throttle ramps 0 → 0.75, `hover_stall` clears by tick 27, and `sub_y` climbs
  128 → 252 → crosses 256, i.e. the unit physically reaches the boundary (`repro.md` §5).
  The ~25-frame spawn turn is real but converges.

### C6 — the order is dropped (`is_bridge_only_goal`, or the Hover `OccupiedEnemy` arm). **REFUTED.**
- Measured: `ACCEPTED PATH: 19 node(s)`, and `target=true` at the end of every run
  (`repro.md` §3, §5). Both lanes agree.

### C7 — the hover vertical controller / `position.z` / deck-height model. **REFUTED as the cause.**
- Measured: `z`, `on_bridge` and `bridge_occupancy` never change; the mover never reaches
  `resolve_cell_transition_bridge_state` (`movement_step.rs:2162`), which sits *after* the
  refusal's `break` at `:2043` (`repro.md` §5, `stall.md` §3). Real residuals live here
  (§7 R2, R3) but they are downstream.

### C8 — `TooBigToFitUnderBridge`, or the tube lane. **REFUTED.**
- `merge_path_blocks` drops the flag (`movement_path.rs:45-56`, with its gamemd citation).
  The tube lane needs a non-adjacent path node and never engages on a high span
  (`repro.md` §7 side note, `stall.md` §5).

---

## 5. FIX PLAN

**Smallest change: delete the invented gate, do not add a hover case.**

**F1 (the fix).** `src/sim/movement/movement_path.rs:91-94` — remove `Hover`'s exclusion
from `supports_layered_bridge_pathing`.

**gamemd source, as landed (status corrected 2026-08-27):** the gate `Find_Path` actually
consults is `vtable+0x2CC` → `FootClass::CanReachDestination` @ `0x004D3810`, a MovementZone
reachability abort with no locomotor term. That binding is **VERIFIED** by direct reads —
`UnitClass` vtable `0x007F5C70` (installed at `0x0073543A`), slot `0x007F5F3C` holding
`0x004D3810`, the callsite at `0x004D397F`, the clear-and-return-0 at `0x004D3989` — recorded in
the `movement_path.rs` provenance block. `CheckBridgeTraversal` @ `0x004D9C60`,
`UnitClass::Can_Enter_Cell` @ `0x0073F0A0` and `ILocomotion+0x1C` → `0x0055ABF0` are
**UNCHECKED here**: lane-reported corroboration that the downstream legality path is
locomotor-agnostic, never re-derived, and not the gate. The fix is justified by *deleting* a
VERA-only restriction — which needs the absence of a native gate demanding it, not affirmative
native proof — plus the production crossings.
Rewrite the provenance block at `:64-82` to record what remains VERA-internal after the
change. All four consumers pick this up with no further edit:
`movement_tick.rs:434`, `movement_tick.rs:694`, `movement_commands.rs:394`,
`movement_blocked.rs:160` (the last is what makes the repath stop reinstalling the bad
route). Recommended scope: **Hover only** in this slice. Removing the whitelist outright is
the more faithful reading of the binary, but it also moves Ship/Teleport/Jumpjet/Fly and
should be its own reviewed slice.

**F2 (not recommended for this slice).** `stall.md` §8 option 1 — "stop discarding the
layers on the flat branch" — is more expensive than that lane states: the flat search's own
wrapper already throws the layers away at `src/sim/pathfinding/core.rs:2735`
(`find_path_with_costs_marker` returns `Option<Vec<(u16,u16)>>`), so this needs a new return
type threaded through `find_path_with_costs_marker` → `find_path_zoned_marker`
(`zone_search.rs:303`) → `movement_path.rs:472`. It also does not remove the invented gate.
Correction recorded so the next session does not cost it wrongly. **VERIFIED** — signature
read at `core.rs:2703-2736`.

**F3 (defer, name it).** Leave `is_bridge_only_goal` (`movement_path.rs:119-121`) in place
this slice. F1 takes Hover off the flat branch, so the Hover half of that defect closes as a
side effect; the Ship/Teleport/Jumpjet half stays open and is recorded as R5.

**The characterization test.** `src/sim/movement/movement_bridge_retail_tests.rs:1077-1081`,
`hover_tank_is_currently_blocked_from_entering_a_high_bridge`, asserts the broken behavior —
its `expect_crossing=false` arm at `:933-954` asserts the ROBO *never reaches a deck cell*
and only proves the mover is not broken in general. **Convert it, do not delete it.** The
harness already contains the full positive crossing assertion under `expect_crossing=true`:
deck frames non-empty (`:957-962`), `on_bridge` on every deck frame (`:986-990`),
`z == terrain_level + BRIDGE_DECK_LEVEL_DELTA` (`:991-996`), never at the riverbed level
(`:997-1001`), `BridgeOccupancy.deck_level == position.z` (`:1002-1007`), and mid-span
coverage (`:1010-1020`). So the conversion is:

1. `:1080` — `drive_across_high_bridge("BayOPigs.mmx", "ROBO", false)` → `..., true)`.
2. `:1079` — rename to `hover_tank_crosses_bay_of_pigs_high_bridge_at_deck_height`.
3. `:1070-1076` — replace the doc comment: it currently explains the whitelist exclusion as
   present-tense fact and cites `movement_path.rs:83-95`; after F1 that text is false.
4. Consider adding the `Hills.mmx` ROBO twin alongside `:1064-1068`, so the green is not one
   map's geometry.

Once green, the `expect_crossing=false` arm at `:933-954` has no caller left. Leave it —
it is the mechanism for characterizing the next mover that cannot cross — or delete it in a
separate cleanup, not in this slice.

---

## 6. BLAST RADIUS

**Stock units on the changed branch.** One CLSID maps to `LocomotorKind::Hover`
(`src/rules/locomotor_type.rs:81-83`), and `ini/rulesmd.ini` gives it to exactly four stock
types (VERIFIED, grepped this session): `[ROBO]` Robot Tank (7453), `[LCRF]` Allied
Hovercraft (7056), `[SAPC]` Soviet Amphibious Transport (7933), `[YHVR]` Yuri Amphibious
Transport (8918). `[YURIPR]` has the line commented out (5283). So the fix moves the Robot
Tank **and all three faction amphibious transports** onto the layered path builder.

**That transport move is the real risk, not the bridge.** The layered branch
(`movement_path.rs:386-465`) differs from the flat one in more than layers: it consults
`ground_blocks`/`bridge_blocks` instead of the merged `entity_blocks`, smooths per-layer
(`:429-459`) instead of on the ground plane (`:498-518`), and runs the zoned search with
`zone_mz`. The three transports carry `MovementZone=Amphibious` (VERIFIED, `ini/rulesmd.ini`),
which is not a water-mover zone in VERA (`src/rules/locomotor_type.rs:332-333` — only
`Water | WaterBeach`), so they stay on the land branches; but their zone grid spans water,
so the corridor the zoned layered search picks can differ from today's. An amphibious
water-crossing test is in scope for this change and was **not** run — no test in the tree drives
`LCRF`, `SAPC` or `YHVR`. **Disposition (2026-08-27): deliberately deferred**, recorded as
residual `R-T101a` against T1-01 in `bridge-movement-matrix.md` with its trigger, effect and
frequency. This paragraph originally said it "must be run"; it was not, and calling that a
deferral in the ledger while leaving "must be run" here would let the row read as complete from
one document and blocked from the other. UNCHECKED, and owned by R-T101a.

**Committed goldens — predicted not to move, must still be run.**
`src/sim/world/bridge_parity_harness_tests.rs` pins `BRIDGE_HARNESS_FINAL_HASH`
(`:98`) and spawns only `MTNK` (Drive) and `E1` (Walk) (`:104-116`).
`src/sim/world/global_parity_harness_tests.rs` pins `GLOBAL_HARNESS_FINAL_HASH` (`:499`),
`GLOBAL_HARNESS_PRE_LIFECYCLE_V28_HASH` (`:341`) and `GLOBAL_HARNESS_PRE_MISSION_V29_HASH`
(`:342`), and spawns `E1`, `MTNK`, `HARV` (`:507-517`) — Walk and Drive only. Both harnesses
therefore contain **zero Hover movers**, and Drive/Walk are already inside the whitelist, so
neither the whitelist arm nor `build_flat_fallback_layers` changes behavior for them. Both
hashes cover `position.z` and `bridge_occupancy.deck_level`
(`bridge_parity_harness_tests.rs:340-342`), so a Hover unit *would* move them if one were
present. **Prediction, UNCHECKED until the suite runs.** If either moves, the change reached
further than this analysis says — investigate rather than re-baseline, and follow the
pending-re-baseline rule in AGENTS.md if the tree is not clean.

**Unchanged and still broken:** Drive movers cross cell boundaries through the
`DriveTrackCellJump` arm with no terrain, bridge-traversal or cliff check at all
(`movement_tick.rs` ~`:2216-2382`; measured — MTNK produced **zero** `REACHED_BOUNDARY`
lines, `repro.md` §6). F1 does not touch that lane, so the Drive crossing goldens cannot move
through it. It is a separate recorded DRIFT (R4).

---

## 7. RESIDUALS

**R1 — Hover `OnBridge` flips on the wrong predicate. DRIFT, deferred.**
gamemd flips Hover's `OnBridge` on *altitude reaching deck height*
(`0x00514944`: `!OnBridge && (cell->Flags & 0x100) && GetHeight() >= 4 levels`), where Drive
and Walk flip on a cell-level delta (`0x004B1830`, `0x0075C179`). VERA uses the
cell-level-delta rule for every mover (`movement_bridge.rs:208`:
`dst_h == src_h.wrapping_sub(4) && dst.has_structural_bridge()`). Status split, corrected
2026-08-27: the VERA half is **VERIFIED** (the code is in the tree and readable); the gamemd
half — the two native predicates and their addresses — is **UNCHECKED**, reported by an
investigation lane whose report is not in the repository (matrix evidence gap 10) and never
re-derived here. Settling this residual means reading `0x00514944` and `0x004B1830` directly,
not re-reading this line. Trigger: every hover deck entry. Player effect: the flip can land a
frame or two off retail relative to the lift ramp — the sprite mounts the deck at a slightly
different moment. Frequency: every hover bridge crossing, so a handful of times a match on a
bridge map. Whether it is visible at all is **UNCHECKED** and needs a frame comparison, not
more reading.

**R2 — hover lift is not composed into `position.z`. DRIFT, deferred.**
gamemd: `Z = groundZ(coord) + (OnBridge ? deckOffset : 0) + hoverAltitude`, additively, via
`FootClass::Set_Height_On_Bridge` @ `0x005F5FA0` and its inverse `ObjectClass::GetHeight`
@ `0x005F5F40` (VERIFIED, gamemd lane). VERA stores `position.z = signed level +
(on_bridge ? 4 : 0)` and keeps hover altitude in a separate field
(`movement_tick.rs:2881`, `loco.altitude`), never summed into `position.z`. Trigger: every
frame a hover unit exists. Player effect: a Robot Tank rides flush with the deck instead of
floating `HoverHeight` above it. Frequency: continuous, but sub-cell — a render-side offset,
not a sim outcome.

**R3 — the hover `climbing` input has no deck term.**
`movement_tick.rs:2851-2863` compares raw `next.ground_level > cur.ground_level`. Entering a
deck reads the riverbed as the next cell's ground, so it computes "descending" exactly where
retail is climbing onto a deck. Trigger: hover deck entry and exit. Player effect: a momentary
lift dip at the bridgehead. Frequency: twice per hover bridge crossing. VERIFIED (VERA code);
gamemd equivalent UNCHECKED.

**R4 — Drive skips all three boundary gates.** `movement_tick.rs` `DriveTrackCellJump` arm.
Trigger: every Drive cell transition. Player effect: a Drive mover can enter a cell a stale
path made illegal. Frequency: continuous, but usually harmless because the planner agrees.
Separate DRIFT, recorded, not part of T1-01.

**R5 — `is_bridge_only_goal` still drops orders for Ship/Teleport/Jumpjet/Fly.**
`movement_path.rs:119-121`, whose own comment says gamemd has no equivalent
(`FootClass::Find_Path` @ `0x004D3920` has one clear-and-return exit, at `0x004D3989`, and it
is not bridge-keyed). Trigger: a click on a deck cell whose ground plane is impassable, by one
of those movers. Player effect: the order is refused where retail runs the search. Frequency:
rare — those locomotors are uncommon and the click has to land on a deck.

**R6 — deck-offset numeric value UNCHECKED.** `g_nFootOnBridgeDeckOffsetLeptons`
(`0x00AC13BC`) and the Hover copy (`0x00A8F1B4`) are runtime-initialised and zero in the file
image; only the formula `ftol(4 × level-height + K)` was verified. VERA's
`BRIDGE_DECK_LEVEL_DELTA = 4` matches the *level count*, not a re-derived lepton value.
No player effect while everything downstream uses levels.

**R7 — one map, one span.** All measurements are BayOPigs `(111,134)→(111,152)`. Whether
every high span's riverbed is `ground_walkable=false`, and whether a Hover mover on a
land-under-span map (Hills) routes *under* an intact deck instead of stalling, is
**UNCHECKED** — T1-13/T1-15's question. Adding the `Hills.mmx` ROBO test in §5 step 4 closes
part of it.
