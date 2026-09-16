# Phase 8 brief — Production, construction, power and radar

Rows 166–184 of
[`2026-07-30-clean-slate-system-implementation-order.md`](../2026-07-30-clean-slate-system-implementation-order.md).
State: **OPEN** (never targeted as a phase; several rows carry earlier slice
work).

## Rows

The System Map was retired on 2026-09-10. Its citations and Registry column
below are historical; revalidate claims rather than maintaining map status.

Registry column is `native_evidence / rust_implementation / parity` from
`docs/system-map/registry.v2.json` at `cca05d50`. Owners were checked to exist
at that commit. `topology.v2.json` still lists ten `src/app_*.rs` paths that
no longer exist (GSI-13.24, GSI-09.11, LOOP-005, LOOP-012); the files moved
under `src/app/` (`presentation/`, `loading/`, `input/commands.rs`,
`match_runtime/sim_tick.rs`).

| Row | GSI | Rust owner(s) | Native anchor(s) | Registry | Notes |
|---|---|---|---|---|---|
| 166 | 04.08 | `src/sim/production/wall_placement.rs`, `src/sim/gate_runtime.rs`, `src/sim/overlay_grid.rs`, `src/map/overlay.rs` | none recorded | N/A / N/A / N/A (MIXED-SCOPE) | Wall autofill, destruction transactions and spend semantics landed (3b32fe1e, 95f77159, da38da27, PR #194). Gates and LaserFence unscanned. |
| 167 | 05.17 | `src/sim/production/factory.rs` | none recorded | ANCHORED / PARTIAL / UNCHECKED | Factory lifecycle; save-order research exists (`FACTORYCLASS_*_RESWARM_20260528.md`). |
| 168 | 09.07 | `src/sim/power_system.rs`, `src/sim/production/production_placement.rs` (place_ready_building), `src/sim/radar.rs` | `HouseClass::AI_AssessPower` 0x00508C30, `UpdateTacticalRadarAvailability` 0x00508DF0 (`POWER_SYSTEM_GHIDRA_REPORT.md`) | ANCHORED / PRESENT / DRIFT | Mechanism blocks MBLK-003/005/006 in `mechanisms.v1.json`. Slice 84991f14 closed live replacement-plant output and Radar=yes recovery. |
| 169 | 09.08 | `src/sim/production/production_tech.rs` | [BUILDINGCLASS_PREREQUISITES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_PREREQUISITES_GHIDRA_REPORT.md) (no anchor in topology) | UNCHECKED / PARTIAL / UNCHECKED | Prerequisite checks; the file also hosts the build-time formula that row 171 owns (see Corrections). |
| 170 | 09.09 | `src/sim/production/factory.rs`, `production_queue.rs` | [BUILD_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILD_QUEUE_GHIDRA_REPORT.md), `FACTORYCLASS_PRODUCTION_DEEP_DIVE.md` | ANCHORED / PRESENT / DRIFT | Abandon/cancel semantics partly fixed (front-most removal); `AbandonProduction` 0x004C9FF0 residual labelled at `factory.rs:283`. |
| 171 | 09.10 | `src/sim/production/factory.rs` (per-step charge), `production_queue.rs` (enqueue, `total_base_frames`), `production_tech.rs` (`build_time_base_frames`) | [FACTORY_CLASS_BUILD_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACTORY_CLASS_BUILD_SPEED_GHIDRA_REPORT.md), `FACTORY_CREDIT_SYSTEM_GHIDRA_REPORT.md`, `GRIZZLY_FACTORY_STEP_CADENCE_GHIDRA_REPORT.md` | ANCHORED / PRESENT / DRIFT | Per-step charge present; same-tick money order and cancel-refund cost timing inherited from Phase 7 (below). |
| 172 | 09.11 | `src/sim/production/production_placement.rs`, `src/sim/world/world_commands.rs`, `src/sim/naval_base_placement.rs` | `HouseClass::Place_Production` 0x004FB0E0, `BuildingClass::Can_Enter_Cell` 0x00449440, cell placement passability 0x0047C620 | UNCHECKED / PARTIAL / UNCHECKED | Feature 12948d89 closed marked-ground and overlay rejection with a sealed Dustbowl GAPOWR oracle; damaged-wall/ToTile/LaserFence/gate exceptions and rejection UI/EVA are explicit residuals. Naval placement landed in PR #180. |
| 173 | 07.23 | `src/sim/world/mod.rs`, `src/sim/world/techno_ai.rs` (`PASSIVE_TARGET_CLEAR_MISSIONS`), `src/sim/credit_income.rs:220`, `src/sim/production/production_sell.rs:861` | [BUILDINGCLASS_MISSION_GUARD_AND_CONSTRUCTION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSION_GUARD_AND_CONSTRUCTION.md); `OnConstructionComplete` 0x00445F80 cited in `src/sim/world/mod.rs` | UNCHECKED / PARTIAL / UNCHECKED | Handler delay uses the shared base-stub frames (`src/sim/world/techno_ai/mission_handlers.rs:1626`); construction completion lives in `world/mod.rs` and `building_anim.rs`. Verify the split against `BuildingClass::Mission_Construction` before treating it as parity. |
| 174 | 09.12 | `src/app/presentation/instances/shp.rs` (`BuildupOrSpecial`, presentation only); `src/sim/world/mod.rs` (`OnConstructionComplete` 0x00445F80) | `OnConstructionComplete` 0x00445F80 | UNCHECKED / PARTIAL / UNCHECKED | No sim-side buildup state exists: grep for buildup/GrandOpening in `src/sim` finds only power-system tests and comments. Buildup as sim authority is likely MISSING, not PARTIAL; scan first. |
| 175 | 07.41 | `src/sim/production/production_spawn.rs`, `war_factory_exit.rs` | `BuildingClass::ExitObject_Main` 0x00443C60 (`GRIZZLY_PRODUCTION_COMPLETE_TO_WAR_FACTORY_EXIT_GHIDRA_REPORT.md`) | UNCHECKED / PARTIAL / UNCHECKED | Two deferred DRIFT residuals labelled at `production_spawn.rs:814` and `:928` (enemy-gate arms). Naval delivery closed 5f100c76. |
| 176 | 09.16 | `src/sim/world/world_spawn.rs` (`deploy_mcv`, `construction_yard_type_for_mcv`), `src/sim/world/world_commands.rs` (`Command::DeployMcv`), `src/sim/ai.rs` (`try_deploy_mcv`), `src/sim/deploy_tests.rs` | [AMCV_CANDEPLOY_PREDICATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AMCV_CANDEPLOY_PREDICATE_GHIDRA_REPORT.md), [AMCV_DEPLOY_FACING_RULE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AMCV_DEPLOY_FACING_RULE_GHIDRA_REPORT.md), `GACNST_*_GHIDRA_REPORT.md` (five docs) | ANCHORED / PRESENT / DRIFT | `src/sim/deploy.rs` is the infantry deploy-fire machine, not MCV. Redeploy transfer, undeploy refund and free-unit docs exist but are unreconciled with code. |
| 177 | 09.14 | `src/sim/production/production_sell.rs` | [BUILDINGCLASS_SELL_AND_REPAIR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_SELL_AND_REPAIR_GHIDRA_REPORT.md), `GARRISON_SELL*` docs | UNCHECKED / PARTIAL / UNCHECKED | Refund formula is `SELL_REFUND_PERCENT=50 × health%` in code; `MBLK-002` was downgraded to UNCHECKED during Phase 7 (see Corrections). |
| 178 | 07.24 | `production_sell.rs` (sell state machine), `src/sim/world/lifecycle.rs`, `world/mod.rs` | `MBLK-001-SELL-MISSION-AUTHORITY` steps | UNCHECKED / PARTIAL / UNCHECKED | Handler delay uses the shared base-stub frames as row 173; the state machine is a separate owner. |
| 179 | 12.10 | `src/sim/radar.rs`, `src/render/radar_events.rs`, `src/rules/radar_event_config.rs` | `MBLK-004-POWERED-RADAR-GATE`, [GAP_RADAR_SHROUD_MINIMAP_INTERACTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GAP_RADAR_SHROUD_MINIMAP_INTERACTION_GHIDRA_REPORT.md) | ANCHORED / PARTIAL / DRIFT | Radar event surfaces composed from gamemd 0x656EC0 / 0x65FA70 (PRs of 2026-08-21). Phase 7 established: event type 10 has no dedupe; sidebar OnHold/Canceled use type −1. |
| 180 | 14.10 | `src/app/sidebar_projection.rs`, `src/app/input/gadget_input.rs`, `src/app/input/sidebar_eva.rs`, `src/app/presentation/sidebar_gadgets.rs` (StripClass::AI poll, Flash_AI), `src/sidebar/sidebar_view.rs` | none recorded | ANCHORED / PARTIAL / DRIFT | Sidebar EVA lines landed in Phase 7 (#254). Credit-counter cadence residual R8. |
| 181 | 14.08 | `src/app/input/commands.rs`, `src/app/input/context_order.rs`, `src/app/input/dispatch.rs` | none recorded | UNCHECKED / PARTIAL / UNCHECKED | Placement/repair/sell/deploy/rally input; rally-point line research exists ([FACTORY_RALLY_POINT_LINE_CALLER_COLOR_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACTORY_RALLY_POINT_LINE_CALLER_COLOR_GATE_GHIDRA_REPORT.md)). |
| 182 | 13.24 | `src/render/sidebar_chrome.rs`, `src/render/sidebar_cameo_atlas.rs`, `src/app/presentation/sidebar_build.rs`, `sidebar_render.rs`, `src/sidebar/power_bar_anim.rs`, `gadget_flash.rs` | `SidebarClass::LoadSHPs` 0x006A5840, `StripClass::Draw` 0x006A9540 | ANCHORED / PRESENT / DRIFT | GPU pixels, blending and progress cadence unverified per topology note. `MBLK-006` power-bar handoff. |
| 183 | 13.23 | `src/render/minimap.rs`, `native_radar_surface.rs`, `native_radar_terrain.rs`, `native_radar_viewport.rs`, `radar_anim.rs`, `src/app/presentation/render/minimap_transaction.rs` | [CELLCLASS_GETRADARCOLOR_FULL_BRANCH_INVENTORY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_GETRADARCOLOR_FULL_BRANCH_INVENTORY_GHIDRA_REPORT.md), [DRAWRADARACTIONLINES_004DC340_ENEMY_LINES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRAWRADARACTIONLINES_004DC340_ENEMY_LINES_GHIDRA_REPORT.md) | ANCHORED / PRESENT / DRIFT | Map-load radar animation is hardwired to Allied (topology note on 13.24). |
| 184 | 14.11 | `src/render/minimap_interaction.rs`, callers `src/app/input/cursor.rs:1113` and `src/app/presentation/sidebar_render.rs:320` | none recorded | ANCHORED / PARTIAL / DRIFT | Unscanned. |

## Loops

Historical references from the archived `topology.v2.json`:

- **LOOP-005-BUILD-PLACE**: 14.10 (1) → 09.08 (2) → 09.09 (3) → 09.10 (4) → 13.24 (6) → 14.08 (7) → 09.11 (9, 12) → 09.12 (14) → 09.07 (15).
- **LOOP-006-FACTORY-EXIT**: 14.10 → 09.08 → 09.09 → 09.10 → 07.41 (6).
- **LOOP-012-POWER-OUTAGE-RECOVERY**: 14.08 (1) → 09.14 (2) → 09.07 (5) → 12.10 (6) → 13.24 (7) → 13.23 (8) → 14.10 (10) → 09.08 → 09.09 → 09.10 → 14.08 (15) → 09.11 (16) → 09.12 (18) → 09.07 (19) → 12.10 (20) → 13.24 (21) → 13.23 (22).
- **LOOP-007-ENGINEER-CAPTURE** stages 14–17 (09.07, 09.08, 09.09, 12.10) and **LOOP-008-REVEAL-RADAR** stages 7–8 (12.10, 13.23) touch this phase from Phase 9/5 loops.

Rows 04.08, 05.17, 07.23, 07.24, 09.16 and 14.11 sit in no recorded loop;
trace them from their callers (wall placement command, factory
register/unregister, MCV deploy command, minimap click).

## Prior work

Newest first. None of these ran as a Phase 8 pass; each was a slice or a
side effect of another phase.

- c23d0d8a (2026-09-06) centralised indexed entity identity; touched
  `production/`, `power_system.rs`, `building_anim.rs` mechanically.
- Phase 7 PRs #254 (sidebar/placement EVA lines), #256 (credit tick in
  `sidebar_projection.rs`), #246 (`building_anim.rs` SpecialAnim gates).
- da38da27 / 95f77159 (2026-09-01) wall mutation effects and destruction
  transactions; PR #194 froze wall-spend semantics.
- 5f100c76 (2026-08-24) naval delivery transaction; b703f20d (2026-08-28)
  construct factory products at production start.
- 2026-08-21 radar PRs: 0x656EC0 primary surface, 0x65FA70 sensed events,
  generated surface for type-5 events.
- 12948d89 placement legality with sealed Dustbowl GAPOWR oracle;
  84991f14 replacement-plant power and Radar=yes recovery; PR #180 naval
  base placement; PR #186 AI deploy latches.
- 2026-07-30 phase0 authority spine rewrote `power_system.rs` and
  `radar.rs`.

Nothing in this phase is closed by evidence yet.

## Corrections

Do not trust these until re-read against the binary:

- `production_tech.rs:422 build_time_base_frames` bakes a refuted ×0.9
  factor (`cost × speed × 9 / 10000`). It is recorded as DRIFT at
  `factory.rs:397` but still live: `production_queue.rs:191` calls it at
  enqueue and stores `total_base_frames`. Re-derive from `FactoryClass` build
  speed before touching row 171.
- `mechanisms.v1.json` `MBLK-002-SELL-REFUND-COMMIT` once claimed a
  RefundPercent-driven VERIFIED refund; Phase 7 (audit R7) relabelled it
  "Rust DRIFT" (line 249: native uses type cost, owner modifiers and
  RefundPercent at 0x00711F60, no health term for a human house). Code uses
  `SELL_REFUND_PERCENT=50 × health%`.
- Repair-depot occupant eviction (0x22 → 0x17 loop) belongs to the
  Hospital/Armory branch at 0x0043CB0C; `Rules+0x16F8` is a constant 1.0, not
  an INI key (Phase 7 closure record).
- Radar event type 10 (capture) has no dedupe; sidebar OnHold/Canceled events
  use type −1 (Phase 7 scan-15-06 correction).
- Topology lists ten `src/app_*.rs` paths that no longer exist (GSI-13.24,
  GSI-09.11, LOOP-005, LOOP-012); moved in the F12 batch, 805f2da8.
- Legacy queue-cancel semantics (`.rev()` last-match, full-cost refund) were
  a DRIFT; the fix is `Factory::cancel_one` (`factory.rs:1130`) and
  `cancel_by_type_for_owner` (`production_queue.rs:1055`).
  Older research describing the legacy behaviour is describing Rust, not
  gamemd.

## Inherited residuals

From the Phase 7 final audit Part C
(`../../gap-scans/2026-09-06-phase7-closure/reverse-audit-phase7-final.md`):

- **R4** same-tick money ordering: repair/depot spend runs after the factory
  step; native runs it before. Visible only near zero balance; every tick both
  are active. Labelled at `src/sim/world/mod.rs:7758-7810` (the audit's
  "unrecorded" is stale). Lands on row 171.
- **R6** cancel refund uses the paid amount; native uses `GetCost(house)` at
  cancel time. Only when a FactoryPlant is built or lost mid-build.
  `factory.rs:264-283`. Lands on rows 170/171.
- **R7** sell-refund provenance (above). Lands on row 177.
- **R8** credit-counter dispatch cadence vs committed frames; visible when
  render rate ≠ logic rate. `sidebar_projection.rs:120`. Lands on row 180.
- **R11** drained-building power cutoff `+0x5778` and DrainAnim are listed
  as recorded minor residuals; no code label carries `0x5778` (the nearby
  labels are `+0x577B`/`+0x67C` in `credit_income.rs:183-200`). Whether a
  drained plant still supplies power in VERA is unverified. Lands on row 168.
- `production_spawn.rs:814` and `:928` enemy-gate arms (deferred DRIFT).
  Lands on row 175.
- Placement residuals from 12948d89: damaged-wall/ToTile/LaserFence/gate
  exceptions, delayed-command rejection UI/EVA. Lands on rows 172/181.

## Coverage

The global parity harness fixture (`src/sim/world/global_parity_harness_tests.rs`)
spawns a war factory, a refinery and a harvester but issues no build, place,
sell or deploy command; it is sim-only, so it exercises 09.07 only as
per-tick power assessment over pre-placed buildings and touches no
presentation row (13.23/13.24) at all. The
sealed Dustbowl GAPOWR oracle
(`src/app/loading/init_helpers_retail_placement_oracle_tests.rs`) covers
placement legality, four-cell occupancy, buildup and power for one building. A moved hash pin from this
phase therefore means "composition changed", not "mechanism covered"; parity
for these rows needs a fixture that builds, places and sells.

## Open queue

Empty. No scan has run against this phase.

## Start here

Scan rows in loop order, not row order: LOOP-005 first (14.10 → 09.08 →
09.09 → 09.10 → 13.24 → 14.08 → 09.11 → 09.12 → 09.07), then LOOP-012 for the
sell and power branches, then LOOP-006 for factory exit, then the six
loop-less rows. Before the first scan, re-read the ×0.9 build-time claim and
the sell-refund formula against the binary; both feed several rows and both
are known-wrong somewhere.
