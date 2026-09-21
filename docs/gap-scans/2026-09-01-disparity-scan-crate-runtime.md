---
title: Disparity Scan - Remaining Phase 14 Crate Runtime
date: 2026-09-01
scope: Pickup admission and continuation, selection, removal and regeneration, active ordinary-retail effects and modifiers, and specific-cell producers after scenario-start placement
methodology: docs-first discovery, direct Rust verification, selective active-YR verification, and installed-retail INI/map census
---

# Disparity Scan - Remaining Phase 14 Crate Runtime

## Scope and evidence basis

This scan covers the remaining ownership hypotheses in **GSI-04.23 Crates and
powerups** and **GSI-08.34 Crate combat modifiers** after scenario-start crate
creation/delivery merged in PR #209. It includes:

- the complete active pickup admission, selection, remove/replace, effect, and
  return transaction;
- all thirteen active `CrateClass__PickupDispatch` calls in eleven native
  movement bodies and their caller-specific continuations;
- `[Powerups]`, the remaining `[CrateRules]` fields, and independent crate type
  metadata required by those paths;
- slot clear/removal and the per-tick regeneration scan;
- the eight positive-weight ordinary-retail outcomes: Money, Unit, HealBase,
  Reveal, Armor, Speed, Firepower, and Veteran;
- common animation/sound/EVA consequences of those outcomes; and
- specific-cell placement through active ordinary-retail trigger action 108 and
  destruction-only `CrateBeneath` producers.

It deliberately excludes the already-reviewed scenario-start random-placement
mechanism, every non-crate Phase 14 row, TS gameplay handlers, and network-only
WOL bookkeeping. Evidence-backed exclusions are enumerated below rather than
silently removed from the model.

The research-index preflight selected these sources, all read directly:

- `docs/research/PHASE3_ACTIVE_RETAIL_CRATE_RUNTIME_GHIDRA_REPORT.md`;
- `docs/research/CRATE_SYSTEM_GHIDRA_REPORT.md`;
- `docs/research/FUN_00481A00_CRATE_PICKUP_WARP_ARRIVAL_GHIDRA_REPORT.md`
  (stale/superseded pickup ABI, caller closure, and effect-index assignments;
  navigation only);
- `docs/research/RULESCLASS_POWERUPS_TABLE.md`;
- `docs/research/PERTICKUPDATE_NON_OBJECT_GLOBAL_LOOPS_GHIDRA_REPORT.md`;
- `docs/research/MAPCLASS_GHIDRA_REPORT.md`; and
- `docs/research/traces/SKIRMISH_CRATES_OFF_INITIAL_CRATES_TRACE.md`
  (native Crates-off trace retained, but its pre-PR-#209 Rust-state claims are
  stale).

The 2026-07-23 crate design and evidence-foundation plan were also read as
hypotheses. They are not specifications and several of their scope/semantic
claims have been superseded by the active-retail report. The decisive gamemd
evidence is the active-binary address/caller evidence in the 2026-08-31 report,
plus the later live `CellClass__CheckCellPassability @ 0x004834A0` occupation
recheck used for PR #209. Installed retail data was checked directly at
`ini/rules.ini:642`, `ini/rules.ini:22496`, `ini/rulesmd.ini:782`, and
`ini/rulesmd.ini:30345`; the installed-map census in the active-retail report
binds action 108 and `CrateBeneath` to ordinary maps.

Current Rust was inspected from `origin/main` merge commit
`15a48e55325ea0902c71accede1397395ea2a2c3`. Every Rust-state claim below comes
from that tree rather than from a research document's older Rust-divergence
section.

## Summary

- 25 documented candidate behavior groups inventoried
- 25 active-YR claims verified or evidence-excluded at the admitted retail boundary
- 7 verified gaps (2 prerequisite-blocked)
- 0 doc-derived candidates awaiting verification
- 7 verified matches / corrected false positives
- 6 evidence-backed inactive, invalid-domain, or out-of-boundary groups

The already-merged startup mechanism is a verified match at its audited
boundary, not proof that pickup or regeneration exists. Current
`src/sim/crates.rs:16-18` explicitly leaves contents, pickup, removal, and
regeneration to later mechanisms.

This report is a dated disparity snapshot, not a parity percentage or a
completion certificate.

## Verified gaps

### HIGH priority

**G1. Runtime crate rules and the fixed nineteen-entry Powerups authority are missing**

- **Active-YR evidence:** `RulesClass__ReadPowerups @ 0x00673E80` owns the fixed
  canonical nineteen-entry table. The third token is over-water eligibility;
  the fourth is the native binary64 magnitude. Installed weights in canonical
  order are `[20,20,10,0,0,0,0,0,10,10,10,10,0,0,20,0,0,0,0]`, total 110.
  `RulesClass__ReadCrateRules @ 0x0066B900` also owns radius, fixed mappings,
  solo money, Unit type, sound, and FreeMCV fields beyond the startup subset.
  RulesClass separately owns the seven `[AudioVisual]` effect-sound identities
  `CrateMoneySound`, `CrateRevealSound`, `CrateFireSound`, `CrateArmourSound`,
  `CrateSpeedSound`, `CrateUnitSound`, and `CratePromoteSound`. Scenario data
  owns the inactive-by-default `TruckCrate` and `TrainCrate` producer gates.
- **Retail evidence:** the full stock sections are present at
  `ini/rulesmd.ini:782-796` and `ini/rulesmd.ini:30345-30365`; the seven stock
  effect-sound mappings are at `ini/rulesmd.ini:635-641`.
- **Research pointer:** active-retail report, **Rules authority** and
  **Selection and guard order**; `RULESCLASS_POWERUPS_TABLE.md` is a navigation
  aid only because its token-three interpretation is stale.
- **Rust state:** `src/rules/crate_rules.rs:10-18` owns only minimum, maximum,
  regen, and three image names. There is no `PowerupTable`, canonical
  `CrateType`, remaining CrateRules fields, `CrateGoodie`, `CarriesCrate`,
  `CrateBeneath`, `CrateBeneathIsMoney`, or `CrateTrigger` in current rules.
  `src/rules/overlay_types.rs:123,361` parses only `Crate=`. Current rules own
  none of the seven `[AudioVisual]` crate sound identities, and current
  scenario data owns neither `TruckCrate` nor `TrainCrate`. In contrast,
  `[CombatDamage] C4Warhead` is already parsed by
  `src/rules/bridge_warheads.rs:29,53` and retained in RuleSet; it is an
  existing HealBase prerequisite, not part of this missing-authority claim.
- **Exact verdict:** DRIFT.
- **Priority rationale:** every pickup selection/effect depends on this fixed
  authority; Unit and specific-cell producers cannot be implemented exactly
  without it.

**G2. Synchronous pickup dispatch and all caller-specific movement continuations are absent**

- **Active-YR evidence:** `CrateClass__PickupDispatch @ 0x00481A00` has exactly
  thirteen calls in eleven bodies at `0x5153E9`, `0x5B1894`, `0x4B405D`,
  `0x4B46E6`, `0x4B0D1B`, `0x6A3689`, `0x6A3D15`, `0x6A03EB`, `0x71972E`,
  `0x75C56C`, `0x6A1401`, `0x4B1DBE`, and `0x54C9F6`. Drive/Ship ForceTrack,
  ProcessDriveTrack, both ProcessMovement calls, Hover, both Jumpjet calls,
  Walk, and Teleport do not share one generic continuation. The return value
  controls the native locomotor continuation; Unit success and event-49 death
  return zero, while most consumed outcomes return one.
- **Research pointer:** active-retail report, **Complete pickup caller closure**
  and **Return-value matrix**.
- **Rust state:** exact search finds no `pickup_crate`, `crate_pickup`,
  `process_committed_cell_entry`, or `committed_cell_entry` in `src/sim/`.
  Current movement remains driven through
  `src/sim/movement/movement_tick.rs:1380,1436,1494`, with separate Drive track
  (`src/sim/movement/drive_track.rs:3995-4031`), air
  (`src/sim/movement/air_movement.rs:189`), and Teleport
  (`src/sim/movement/teleport_movement.rs:311`) paths. None calls the crate
  authority after a committed arrival.
- **Exact verdict:** DRIFT.
- **Priority rationale:** every ordinary crate pickup is missing; deferring the
  transaction to a phase tail would also perturb same-call-stack state and RNG.

**G3. Slot clear/removal, immediate replacement, and per-tick regeneration are missing**

- **Active-YR evidence:** `CrateSlot__ClearAndPreserveTimer @ 0x004A1750`
  attempts identity-specific overlay removal, clears the coordinate regardless,
  preserves remaining signed/wrapping duration, and sets start to `-1`.
  `MapClass__RemoveCrateAtCell @ 0x0056C020` uses mode-zero overlay identity or
  the first ascending occupied slot in nonzero mode. Pickup ignores remove
  failure and, when nonzero mode plus Crates is enabled, calls one immediate
  random replacement before the effect. `MapClass__UpdateCrateRegenTimers @
  0x0056BBE0` scans all 256 slots ascending and performs live first-free
  replacement, including possible later-index same-scan cascades.
- **Scheduler evidence:** sole caller `LogicClass__PerTickUpdate @ 0x0055AFB0`,
  direct call `0x0055B655`, after AlphaShape purge and before Tactical, Factory,
  and House. At the current Rust partition the exact insertion point is before
  the first factory sweep at `src/sim/world/mod.rs:7352`, using the current
  pre-increment `binary_frame` (which commits at `src/sim/world/mod.rs:6287`).
- **Research pointer:** active-retail report, **Exact timer and regeneration**
  and **Clear and removal**; per-tick report lines 44-47.
- **Rust state:** `src/sim/crates/state.rs:52-77` exposes only first-empty,
  slots, mutation by index, and occupied-cell iteration. Exact search finds no
  runtime clear/remove/regen function. `src/sim/world/mod.rs:7352-7385` enters
  production directly with no crate regeneration rung.
- **Exact verdict:** DRIFT.
- **Priority rationale:** every startup crate currently remains forever unless
  another future mechanism mutates it; ordinary three-minute regeneration and
  every pickup replacement are absent and RNG-ordering-critical.

**G4. Money, Unit, HealBase, Reveal, and their presentation tails are absent**

- **Active-YR evidence:** the handlers at `0x00482463`, `0x00482041`,
  `0x00482B8F`, and `0x00481F9D` respectively perform the verified credit RNG,
  unbounded CrateGoodie selection/spawn retry, live Logic owner-heal through
  `C4Warhead`, and ordered shroud/radar writes. Mutation precedes
  sound/animation; Unit placement success alone returns zero and skips the
  common animation tail at `0x004832F5`.
- **Research pointer:** active-retail report, **Eight active-retail effects**,
  Money through Reveal.
- **Rust state:** there is no effect dispatch in `src/sim/crates.rs`; its only
  production functions end with startup placement helpers before the test
  module. House credits and object-spawn/damage primitives exist, but no crate
  path calls them. The exact `C4Warhead` identity is already parsed at
  `src/rules/bridge_warheads.rs:29,53` and retained at
  `src/rules/ruleset.rs:2328,2925-2928`; the missing HealBase part is the crate
  caller and live Logic traversal, not this rules prerequisite.
  `src/sim/house_state.rs:294` has `map_is_clear`, but no `Visionary` latch;
  current fog reveal does not implement the crate handler's four ordered
  per-cell writes. No crate sound/EVA/common-animation producer exists.
- **Exact verdict:** DRIFT.
- **Priority rationale:** these are four of the eight stock weighted outcomes,
  including economy, unit creation, base recovery, and full-map information;
  all are immediately player-visible and outcome-changing.

**G5. GSI-08.34 Armor, Speed, Firepower, and Veteran crate modifiers are not delivered**

- **Active-YR evidence:** the handlers at `0x00482D56`, `0x00482F36`,
  `0x00483125`, and `0x00482972` scan the live Ground display buffer without an
  owner filter, use strict 3-D `ftol(sqrt(...)) < CrateRadius`, and apply the
  parsed magnitudes in live order. Armor, Speed, and Firepower own independent
  persistent binary64 instance multipliers; Veteran runs the verified tier
  helper loop. Speed excludes Aircraft, and the exact Foot speed consumer uses
  `Foot+0x580` between native truncation stages.
- **Research pointer:** active-retail report, **Shared radius contract** and
  Armor/Speed/Firepower/Veteran sections.
- **Rust state:** `src/sim/game_entity.rs:426-429` has a persistent native-bit
  armor multiplier, folded into current damage at `src/sim/combat/mod.rs:2806`
  and hashed at `src/sim/world/world_hash.rs:1381`. There is no persistent
  crate Speed or Firepower field on `GameEntity`, and both effects also lack an
  exact production consumer. Production movement reaches the reduced
  `owner_current_speed_from_fraction` helper through
  `src/sim/movement/movement_tick.rs:2027-2032`; the helper at
  `src/sim/movement/drive_locomotion.rs:296-322` has no persistent crate
  multiplier stage and explicitly records other unresolved native terms. The
  locomotor `SimFixed` fraction is a different owner.
  `CombatMods::attacker_unit_firepower` exists only as a transient/default
  input (`src/sim/combat/damage/mod.rs:47-53`) with no GameEntity writer, while
  `src/sim/combat/damage/attacker.rs:18-28` labels `fire_damage` staged and
  non-authoritative and no production caller reaches it; production currently
  reads defender fields only (`src/sim/combat/damage/mod.rs:45-46`). Veterancy
  state exists, but no crate radius/tier transaction invokes it. No current
  code performs the native live Ground scan or the effect presentation order.
- **Exact verdict:** DRIFT (Armor substrate is a verified prerequisite match,
  not a delivered effect).
- **Priority rationale:** four of eight stock random outcomes are missing;
  three permanently change combat/movement results and all can affect enemies
  inside the radius.

### MEDIUM priority

**G6. Specific-cell placement and ordinary-retail action 108 are missing and trigger-blocked**

- **Active-YR evidence:** helper `0x0056BEC0` performs one FNPC snap, one
  ascending free-slot scan, no duplicate suppression, exact full-dword data
  handling, and at most one placement. `TriggerAction__Execute @ 0x006DD8B0`
  case `0x6C` (`0x006DF69B..0x006DF6CD`) resolves `TAction+0x44`, passes signed
  `+0x90`, has no mode/Crates gate, and returns the helper boolean. Installed
  ordinary retail contains thirteen calls: eleven in `xxmas.map` and two in
  `xarena.map`, all selecting positive-weight outcomes.
- **Retail evidence:** active-retail report lines 1034-1041 records exact cells
  and data values, including full-dword sentinel behavior.
- **Research pointer:** active-retail report, **Specific-cell placement** and
  **Trigger action 108**.
- **Rust state:** `src/sim/crates.rs` exposes no specific-cell placement entry.
  `src/sim/trigger_runtime.rs:28-42,300-431` has no action 108 case and drops
  unknown actions. `src/map/actions.rs:11-21` does not retain the exact signed
  `TAction+0x90` operand. More fundamentally, current trigger state is aggregate
  by Trigger ID and lacks per-Tag/TriggerInstance ownership, Events 1/8, and
  native Spring/action ordering.
- **Exact verdict:** DRIFT - BLOCKED by the verified trigger-ownership
  prerequisite for production action delivery. The specific-placement primitive
  itself is not blocked.
- **Priority rationale:** active in ordinary retail but limited to two stock
  skirmish maps; the prerequisite also unblocks broader Phase 14 campaign
  scripting.

**G7. Destruction-only CrateBeneath ingress is missing**

- **Active-YR evidence:** `BuildingClass__Place_OccupyMap @ 0x00441F60` is
  reached only from the two fatal destruction continuations, after UnInit while
  the Building remains allocated. It reads `BuildingType+0x1767`, derives the
  northwest render-coordinate cell, and invokes specific placement with data
  zero for `CrateBeneathIsMoney` or exact `0x14` otherwise. It has no
  Crates/mode/owner gate. Voluntary sale, construction, capture, direct despawn,
  and ordinary placement are verified negative paths.
- **Retail evidence:** fourteen types set the flag; 58 placed instances across
  sixteen ordinary maps are active, 55 Money and three random.
- **Research pointer:** active-retail report, **Building CrateBeneath**.
- **Rust state:** exact search finds no `CrateBeneath` field in rules or sim and
  no building-fatal crate adapter. Current generic trigger/lifecycle deletion
  is not an equivalent owner. This path also needs G6's specific-placement
  primitive, but not the action-108 trigger rewrite.
- **Exact verdict:** DRIFT - BLOCKED only by the shared specific-placement
  prerequisite.
- **Priority rationale:** narrower than random pickup but materially visible on
  sixteen ordinary maps and deterministic once one of those props is destroyed.

### LOW priority / exactification residuals

No active ordinary-retail crate difference was demoted to LOW. The remaining
bounded differences below are exclusions or prerequisite-blocked verified gaps,
not low-severity parity claims.

## Doc-derived candidates needing verification

None in this bounded scan. The fresh active-retail report closes the active
ordinary behaviors needed to choose the next implementation mechanism. Older
unverified or contradicted claims were rejected rather than promoted.

## Deferred / blocked by prerequisites

- **Action 108 production delivery (G6)** - ACTIVE-YR VERIFIED GAP; blocked by
  replacement of aggregate Trigger-ID state with per-Tag/TriggerInstance
  ownership and correct Event 1/8/action order. Adding an isolated `108` match
  to the current dispatcher would be known architectural drift.
- **CrateBeneath production delivery (G7)** - ACTIVE-YR VERIFIED GAP; blocked by
  the exact shared specific-cell placement primitive, then attachable to the
  already-identified fatal Building destruction transaction.
- **CrateTrigger events 49/50** - ACTIVE-YR VERIFIED but no ordinary stock map
  defines those conditions. Parse/index/latch shape remains a regression
  surface; campaign/custom action consequences wait for the same trigger-owner
  prerequisite and are not required for ordinary-stock crate activation.

## Evidence-backed exclusions

- **Scenario-start random placement:** closed by reviewed PR #209. Current
  production reaches it at `src/sim/scenario_post_map.rs:105-120`; the fixed
  256-slot state is serialized/hash-authoritative. This scan does not reopen it.
- **Authored CRATE/WCRATE OverlayPack bootstrap:** all 184 named installed maps
  contain zero such identities. Native deliberately filters crate identities in
  nonzero mode. Current `src/sim/overlay_grid.rs:207-242` preserves that gate.
- **Weight-zero gameplay handlers:** Cloak, Explosion, Napalm, Darkness, ICBM,
  Gas, Tiberium, and TS carryover slots are never selected by stock weights and
  no ordinary action 108 requests them. Keep all nineteen indices parsed and
  testable; do not implement TS Squad/Invulnerability/IonStorm/Pod gameplay.
- **CarriesCrate runtime:** only TRUCKB sets the type flag, but both native
  scenario gates default false and every installed `TruckCrate`/`TrainCrate`
  value is `no`. Current Rust lacks all three inputs: `CarriesCrate`,
  `TruckCrate`, and `TrainCrate`. Parse them and retain synthetic gate
  regression; do not add an ordinary-retail death producer.
- **LAN/WOL modes 3/4:** Phase 14's admitted offline boundary is raw mode 0/5.
  The WOL pickup counter and network-specific Reveal cells are verified but
  belong to the later online-service rows. Current `game_mode_nonzero` is enough
  for the admitted crate branch; do not synthesize a partial WOL counter.
- **Allocator/orphan/invalid-domain state:** native Overlay allocator OOM orphan
  identity, zero-total malformed Powerups, invalid pointer graphs, and memory
  corruption are outside valid retail gameplay. Persistent ghost slot/timer/RNG
  behavior remains required and is already represented.

## Doc errors discovered

- **`PHASE3_ACTIVE_RETAIL_CRATE_RUNTIME_GHIDRA_REPORT.md`, Rust divergence
  lines 1526-1535** - stale after PR #209. Current Rust now owns the exact fixed
  slots, signed rule subset, Map rectangle, Mark/ghost path, timer, save/hash,
  and production startup delivery. Only the later runtime items remain absent.
- **Same report, Mark occupation lines 369-375** - the statement that any
  nonzero selected occupation byte rejects is wrong. The later live
  `CellClass__CheckCellPassability @ 0x004834A0` recheck proves the intersecting
  filters reduce to exact bit `0x40`; current Rust implements this at
  `src/sim/crates.rs:619-634`.
- **`CRATE_SYSTEM_GHIDRA_REPORT.md` older sections** - several claims are
  superseded: third Powerups token is over-water eligibility, radius effects do
  not owner-filter, `CrateBeneath` has no global Crates/mode gate, stock
  CarriesCrate producer gates are false, and initial count uses human seats.
- **`RULESCLASS_POWERUPS_TABLE.md`** - token three is mislabeled as a generic
  enabled flag and its stock-active summary is stale.
- **`MAPCLASS_GHIDRA_REPORT.md` current-Rust table** - `Crate system — Not
  implemented` is stale after PR #209; its own header already warns that the
  report is superseded. The raw slot layout remains a useful pointer.
- **`FUN_00481A00_CRATE_PICKUP_WARP_ARRIVAL_GHIDRA_REPORT.md` lines 14-49 and
  82-115** - the pickup ABI, caller closure, and effect-index assignments are
  stale/superseded. The later active-retail recheck proves
  `CrateClass__PickupDispatch @ 0x00481A00` receives `CellClass*` in `ECX` plus
  the collector argument and has thirteen calls in eleven movement bodies,
  whose continuations are not interchangeable. The canonical active indices
  are Money 0, Unit 1, HealBase 2, Reveal 8, Armor 9, Speed 10, Firepower 11,
  and Veteran 14; in particular, HealBase is not index 18.
- **`traces/SKIRMISH_CRATES_OFF_INITIAL_CRATES_TRACE.md` lines 12, 62, 71, and
  86-87** - its statements that Rust has no crate slots or scenario-start
  placement are stale after PR #209. Current `src/sim/crates/state.rs:11-77`
  owns the fixed slots and `src/sim/scenario_post_map.rs:105-120` reaches
  scenario-start placement in production. The trace's native option-off
  observation remains useful; its older Rust divergence does not.
- **2026-07-23 crate design/plan hypotheses** - over-scope weight-zero gameplay
  handlers and CarriesCrate as ordinary-runtime obligations, and older FreeMCV
  wording conflates parsed `[CrateRules] FreeMCV` with the actually-read session
  `Bases` option. They must not drive implementation without the newer report.

## Appendix - verified matches and false positives

| Preliminary claim | Evidence state | Actual Rust state |
|---|---|---|
| Crate state is wholly absent | ACTIVE-YR VERIFIED false positive after PR #209 | `src/sim/crates/state.rs:11-77`, `src/sim/world/mod.rs:819-824`, snapshot v114, and `src/sim/world/world_hash.rs:1152-1162` own the 256 raw slots. |
| Scenario-start crates are disconnected/test-only | ACTIVE-YR and production-path verified | `src/sim/scenario_post_map.rs:105-120` calls the production placer; accepted random-map startup reaches the same path. |
| Crates-off still spends startup RNG | ACTIVE-YR VERIFIED false positive | `src/sim/crates.rs:190-204` returns before placement/RNG; the reviewed option-off trace remains matched. |
| Map-authored crates need slot bootstrap | ACTIVE-YR VERIFIED exclusion | Installed census is zero and native nonzero-mode load filters them; `src/sim/overlay_grid.rs:228-231` matches. |
| Session Bases authority is absent | ACTIVE-YR/Rust VERIFIED false positive | `src/sim/game_options.rs:29-38,71-81` persists stock-default `bases` and `crates`; the missing item is the pickup guard consumer. |
| Armor instance state must be invented in crates | ACTIVE-YR/Rust VERIFIED false positive | Persistent `NativeF64Bits` armor authority already exists at `src/sim/game_entity.rs:426-429`, is consumed and hashed; only the crate writer/radius transaction is absent. |
| Every nonzero occupation bit blocks startup Mark | ACTIVE-YR VERIFIED false positive | Later live recheck proves exact `0x40`; current `src/sim/crates.rs:619-634` matches. |

## Ghidra annotation candidates

None. The scan discovered documentation corrections, not a new metadata label or
signature that passes AGENTS.md's synchronization gate.

## Recommendations

The next complete, production-reachable mechanism should be **slot clear,
identity-specific removal, and the live per-tick regeneration rung**. It builds
directly on PR #209's persisted slots and verified random placer, reaches
ordinary production without waiting for movement/effect work, and supplies the
same removal primitive later pickup needs. Its design must preserve ascending
live scan/reinsertion semantics and run immediately before the first factory
sweep on the pre-increment frame.

After that, close the rules/Powerups/metadata authority, then the synchronous
pickup/caller barrier plus the eight active effects and presentation. Treat
the missing persistent Speed/Firepower state and their exact production
consumer integrations as prerequisites within the corresponding effect
mechanism, not as already-delivered combat or movement behavior. Treat
specific-cell placement as a shared primitive: attach `CrateBeneath` to the
fatal Building transaction independently, while leaving action 108 unpublished
until the per-Tag trigger ownership prerequisite is green and reviewed.

Do not broaden any of these mechanisms into weight-zero TS handlers, inactive
CarriesCrate production, authored-overlay bootstrap, or WOL bookkeeping.
