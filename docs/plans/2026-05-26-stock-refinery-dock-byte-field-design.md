# Stock Refinery Dock Byte-Field Design

## Goal

Replace the current miner-local dock pivot/unload approximation with a gamemd-style byte-field model for stock `CMIN/HARV -> GAREFN/NAREFN` dock `0x16`, mission `0x10`, and unload-start state.

Post-swarm status: this design is implementation-ready for the stock refinery dock timing/behavior patch after the 2026-05-26 Plan C swarm closed the RateTimer, mission-delay, `+0x110`, accumulator-ordering, and frame-counter proof gates.

One byte-level caveat remains: `Unit+0x104` is verified as non-cadence opaque scratch in the `+0xF8..+0x110` cluster, but the exact runtime value copied from stack scratch at unload-start was not sampled. That means Plan C can implement the verified gameplay/timing mechanism now, but it must not claim full byte-perfect `+0x104` state parity until a runtime/value trace closes that last field-content detail.

## Post-Swarm Patch Update

Swarm: `2026-05-26T21:48+02:00 - plan-c-refinery-dock-unload-proof-gates`

Reports:

- [docs/research/miner/UNIT_0X110_UNLOAD_ACCUMULATOR_STEP_WRITERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/UNIT_0X110_UNLOAD_ACCUMULATOR_STEP_WRITERS_GHIDRA_REPORT.md)
- [docs/research/miner/MISSION_DEPLOY_UNLOAD_TIMER_CLUSTER_0X104_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_DEPLOY_UNLOAD_TIMER_CLUSTER_0X104_SOURCE_GHIDRA_REPORT.md)
- [docs/research/miner/MISSION_0X10_RETURN_DELAY_STORAGE_AND_RESCHEDULE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_0X10_RETURN_DELAY_STORAGE_AND_RESCHEDULE_GHIDRA_REPORT.md)
- [docs/research/miner/TECHNOCLASS_AI_UPDATE_UNLOAD_ACCUMULATOR_ORDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/TECHNOCLASS_AI_UPDATE_UNLOAD_ACCUMULATOR_ORDERING_GHIDRA_REPORT.md)
- [docs/research/miner/DOCK_RATE_TIMER_FRAME_COUNTER_ORDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCK_RATE_TIMER_FRAME_COUNTER_ORDERING_GHIDRA_REPORT.md)

Resolved proof gates:

- `+0x110` is constructor-set to `1` by `TechnoClass::Constructor @ 0x006F2B81`.
- `Mission_Deploy_Building` unload-start does not rewrite `+0x110`; it preserves the constructor step.
- `TechnoClass::AI_Update` increments `+0xF8` by `+0x110` when the cluster expires.
- `+0x104` is not cadence input; unload-start copies stack scratch and `AI_Update` later overwrites it. Do not model it as facing, pad cell, refinery id, or Z.
- Mission `0x10` return delay is stored passively: `+0xC8 = g_CurrentFrameCounter`, `+0xD0 = return value`; next dispatch waits for `elapsed >= duration`.
- Facing-not-ready direct return is literal `5` and consumes no RNG.
- Accepted unload-start returns through `[Unload] Rate=.016`: `ftol(.016 * 900) + RandomRanged(0,2)`, stock `14..16`, consuming exactly one RNG draw.
- `TechnoClass::AI_Update` calls `Mission_Dispatch` before the accumulator block, so state 3 reads `+0xF8` before the current tick increment.
- Unload-start initializes the cluster with elapsed `0`; it does not increment `+0xF8` in the same AI pass.
- `g_CurrentFrameCounter` increments after logic/object work; same-tick `RateTimer::Set` / `Current` and unload-cluster reads see elapsed `0`.

Implementation consequence:

- Replace the current Rust `unload_timer` countdown and `(interval - 10)` preload with a frame-stamped periodic accumulator.
- Add mission-`0x10` passive delay fields instead of polling `Pivoting` every Rust tick.
- Preserve the current `FacingClass` same-tick set/read behavior; do not add a dock-local `binary_frame - 1` hack.
- Treat `+0x104` as opaque scratch for now; do not use it for cadence or visible behavior.

Patch order:

1. Add serialized miner fields for mission `0x10` delay and unload accumulator state.
2. Introduce helper methods for passive mission delay due checks and scheduling.
3. Introduce helper methods for the `+0xF8..+0x110` periodic accumulator.
4. Change `Pivoting` / mission `0x10` handling so facing-not-ready schedules delay `5` and consumes no RNG.
5. Change accepted unload-start to initialize latch/substate/accumulator and schedule stock `14..16` with one RNG draw.
6. Change unload state 3 to check `+0xF8 >= HarvesterDumpRate * 900.0` before applying the current tick accumulator increment.
7. Remove Plan C drift from this path: forced East snap, `DockDeploy`, physical stock `on_pad` meaning, and `interval - 10` preload.
8. Update tests and stale assertions.

Patch must not claim:

- exact `+0x104` byte value at unload-start;
- save/load parity for `+0x104` or `+0x110`;
- exact observed first-deposit replay frame without runtime trace.

## Architecture Context

The current Rust flow already splits refinery docking into phases under `MinerState::Dock`, but some phases still compress gamemd side effects:

- `src/sim/miner/mod.rs` owns `Miner`, `RefineryDockPhase`, cargo state, dock timers, and serialized miner state.
- `src/sim/miner/miner_dock_sequence.rs` owns `Approach`, `MissionEnter`, `AwaitingAcceptedCell`, `FaceSync`, `MissionQueued`, `Pivoting`, `Unloading`, and `Departing`.
- `src/sim/miner/miner_dock.rs` owns refinery contact bookkeeping: contact list, waiting retry queue, contact-entered, and `on_pad`.
- `src/sim/movement/locomotor.rs` owns active locomotor kind, CMIN primary/piggyback ownership, and generic movement state.
- Render reads simulation state such as `display_type_override`, entity facing, and unit type fields, but `sim/` must not depend on render/audio/ui/net.

The existing `sync_dock_facing` function models dock `0x16` as a miner-owned smooth East pivot. `start_unload_deploy` then snaps `entity.facing = 0x40`, marks `on_pad`, emits `DockDeploy`, seeds a local unload timer, and switches to `Unloading`.

The verified model says those side effects are split differently: `0x16` is active-locomotor RateTimer sync, `0x15` queues mission `0x10`, and mission `0x10` owns the path/RateTimer gate and first unload-active byte/timer writes.

## Impact Analysis

Touched modules:

- `src/sim/miner/mod.rs`: add explicit gamemd-like dock/unload fields and migrate old serialized `dock_pivot_facing`.
- `src/sim/miner/miner_dock_sequence.rs`: replace `sync_dock_facing`, `Pivoting`, and `start_unload_deploy` with mission `0x10` dispatch/gate helpers.
- `src/sim/miner/miner_dock.rs`: stop treating `on_pad` as stock physical pad occupancy for zero-link DockUnload; keep or rename as Rust-internal queue bookkeeping only if needed.
- `src/sim/movement/locomotor.rs`: add active Drive-owned RateTimer/Do_Turn state.
- `src/sim/miner/miner_tests.rs`: rewrite stale tests that assert East snap, per-tick pivot polling, and stock `DockDeploy`.

Blast radius:

- Save compatibility: old saves may contain `dock_pivot_facing`, `on_pad`, and phases with meanings that change.
- Two-miner contention: removing physical `on_pad` meaning can change admission blocking if tests currently depend on it.
- CMIN piggyback: dock RateTimer must live on active Drive while primary Teleport remains stored.
- Visual display: removing the final East snap may expose current render-facing values that were previously hidden.
- Audio events: removing stock `DockDeploy` affects tests or UI hooks expecting it.
- Mission timing and RNG: full byte-field parity needs the accepted-path mission timer epilogue and any RNG consumption, not just the not-ready return delay `5`.

Determinism:

- New byte-like fields must be deterministic, serialized, and updated in the same tick order as existing miner state.
- No floating point in sim logic. Any RateTimer dynamics must use integer/fixed math.
- Live entity iteration order remains existing `EntityStore` / miner snapshot order.

## Chosen Approach

Use approach C: a gamemd-style byte-field model, but implement it as a staged Rust migration inside the existing `sim/miner` and `sim/movement` boundaries.

This design intentionally does not jump straight to a generic whole-engine `MissionClass`. It introduces the exact fields needed for the stock harvester dock/unload slice first, with names that map to the verified gamemd offsets. A future broader mission model can absorb these fields after more systems need them.

The chosen approach is full for this dock/unload slice:

- active Drive-owned RateTimer state for dock `Do_Turn(0x4000)`;
- explicit mission `0x10` delay and substate modeling;
- explicit unload-active latch equivalent to `+0x6D1`;
- explicit dump accumulator equivalent to `+0xF8`;
- explicit timer cluster equivalent to `+0x100..+0x10C`;
- no direct body-facing snap at unload start;
- no stock `DockDeploy` sound from the verified unload-start block;
- no stock reciprocal `+0x2E4`/physical pad-link meaning for normal zero-link unload.

## Tiny-Detail Ledger

- Stock activation: `CMIN/HARV` have `Harvester=yes` and dock to `NAREFN,GAREFN`; `GAREFN/NAREFN` have `DockUnload=yes`, `Refinery=yes`. Source: `rulesmd.ini`, [UNIT_MISSION_DEPLOY_BUILDING_UNLOAD_START_IMPLEMENTATION_VERIFICATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNIT_MISSION_DEPLOY_BUILDING_UNLOAD_START_IMPLEMENTATION_VERIFICATION_GHIDRA_REPORT.md).
- Accepted movement target is `NW+(3,1)`, not `GetDockCoord` and not `QueueingCell`. Source: [STOCK_REFINERY_DOCK_UNLOAD_STATE_MACHINE_CURRENT_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/miner/STOCK_REFINERY_DOCK_UNLOAD_STATE_MACHINE_CURRENT_SYSTEM_MODEL_SYNTHESIS.md).
- `QueueingCell=4,1` remains staging/fallback only. Source: `artmd.ini`, lifecycle doc map.
- `0x18` sets contact-entered `+0x418`; it is not unload-active state and not reciprocal pad occupancy. Source: `STOCK_REFINERY_DOCK_UNLOAD_LIFECYCLE_DOC_MAP.md`.
- First ordinary `0x16` can call active locomotor `+0x4C(0x4000)` and return `1` without sending `0x15`. Source: [DOCK_ARRIVAL_PIVOT_SEQUENCE_DOC_CONFLICT_AUDIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCK_ARRIVAL_PIVOT_SEQUENCE_DOC_CONFLICT_AUDIT_GHIDRA_REPORT.md).
- `0x16` has no `GetDockCoord`, no `Set_Destination`, no location write, and no unload start. Source: same audit.
- Drive locomotor `+0x4C` is `DriveLocomotionClass::Do_Turn`; decompile is `RateTimer__Set(&param_2)`. Source: [FACING_BYTE_VS_DIRECTION_INDEX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACING_BYTE_VS_DIRECTION_INDEX_GHIDRA_REPORT.md).
- East compass convention remains valid: 8-bit `0x40`, direction index `2`, delta `(1,0)`. Source: same facing report.
- `0x15` queues mission `0x10`; it does not start unload, snap, drain cargo, emit sound, or set pad occupancy. Source: [RADIO_0X15_START_UNLOAD_SIDE_EFFECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_0X15_START_UNLOAD_SIDE_EFFECTS_GHIDRA_REPORT.md), mission deploy verification.
- Mission `0x10` dispatches to `UnitClass::Mission_Deploy_Building`. Source: [UNIT_MISSION_DEPLOY_BUILDING_UNLOAD_START_IMPLEMENTATION_VERIFICATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNIT_MISSION_DEPLOY_BUILDING_UNLOAD_START_IMPLEMENTATION_VERIFICATION_GHIDRA_REPORT.md).
- Mission `0x10` checks `RadioClass::In_Radio_Contact` (formerly mislabeled PathType__Has_Valid_Steps) before facing/RateTimer gate and before unload-start writes. Source: same verification.
- Facing/RateTimer accept condition is `((RateTimerCurrent >> 7) + 1) & 0x1FE == 0x80`. Source: same verification.
- If not accepted and `+0x6AF` is clear, mission `0x10` calls locomotor `+0x4C(0x4000)` and returns delay `5`. Source: same verification.
- Accepted unload start writes `+0xF8=0`, `+0x6D1=1`, `+0x10C=1`, `+0x100=current frame`, `+0x104=stack value`, `+0x108=1`, optional slot 7, then `+0xBC=3`. Source: same verification.
- Accepted unload start does not explicitly force body facing to East. Source: same verification OQ-07.
- No direct sound/Voc call appears in the verified unload-start init range. Source: same verification OQ-09.
- First cargo drain is later state 3, gated by `+0xF8 >= HarvesterDumpRate * 900.0`, not unload-start frame. Source: same verification and dump-rate reports.
- Normal stock zero-link unload has no stock reciprocal `unit/building +0x2E4` write at unload start. Source: same verification OQ-08.
- Exact visible body-facing byte for every approach remains runtime-sensitive. Source: [FACING_BYTE_VS_DIRECTION_INDEX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACING_BYTE_VS_DIRECTION_INDEX_GHIDRA_REPORT.md) OQ-10.
- `+0x104` is written from stack scratch during unload-start timer-cluster initialization; it is not a cadence input and should be modeled as opaque scratch if carried. Source: [MISSION_DEPLOY_UNLOAD_TIMER_CLUSTER_0X104_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_DEPLOY_UNLOAD_TIMER_CLUSTER_0X104_SOURCE_GHIDRA_REPORT.md). Status: `RESOLVED for timing/behavior; exact runtime byte value remains unchecked`.
- Accepted unload-start returns through mission timer epilogue after latch/timer/substate writes: stock `[Unload] Rate=.016` gives `14..16` frames and consumes exactly one `RandomRanged(0,2)`. Source: [MISSION_0X10_RETURN_DELAY_STORAGE_AND_RESCHEDULE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_0X10_RETURN_DELAY_STORAGE_AND_RESCHEDULE_GHIDRA_REPORT.md). Status: `RESOLVED`.
- Facing-not-ready mission `0x10` direct-returns `5`, stores that return as passive mission duration, and consumes no RNG. Source: [MISSION_0X10_RETURN_DELAY_STORAGE_AND_RESCHEDULE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_0X10_RETURN_DELAY_STORAGE_AND_RESCHEDULE_GHIDRA_REPORT.md). Status: `RESOLVED`.
- `+0x110` is constructor-set to `1`; Mission_Deploy unload-start preserves it; AI_Update adds it to `+0xF8`. Source: [UNIT_0X110_UNLOAD_ACCUMULATOR_STEP_WRITERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/UNIT_0X110_UNLOAD_ACCUMULATOR_STEP_WRITERS_GHIDRA_REPORT.md). Status: `RESOLVED`.
- Mission dispatch runs before the generic Techno accumulator block, so Mission_Deploy state 3 reads the previous `+0xF8` value before the current tick increment. Source: [TECHNOCLASS_AI_UPDATE_UNLOAD_ACCUMULATOR_ORDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/TECHNOCLASS_AI_UPDATE_UNLOAD_ACCUMULATOR_ORDERING_GHIDRA_REPORT.md). Status: `RESOLVED`.
- Same-tick timer start/read uses elapsed `0` because gamemd increments `g_CurrentFrameCounter` after logic/object work. Source: [DOCK_RATE_TIMER_FRAME_COUNTER_ORDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCK_RATE_TIMER_FRAME_COUNTER_ORDERING_GHIDRA_REPORT.md). Status: `RESOLVED`.

## Design

### Components

1. `DriveRateTimer` on `LocomotorState`

   Add an integer/fixed deterministic RateTimer-like substate owned by the active Drive locomotor. It is the home for dock `Do_Turn(0x4000)` and for the value sampled by mission `0x10`.

   Required responsibilities:

   - store current/target timer value in the same logical domain as the verified `0x4000` argument;
   - expose a `do_turn_rate_timer(target)` helper for active Drive;
   - expose a `current_rate_timer()` helper for mission `0x10`;
   - expose an accept-window helper implementing `((current >> 7) + 1) & 0x1FE == 0x80`;
   - never directly write `entity.facing` as a side effect of dock `0x16`.

   Post-swarm status: exact RateTimer progression and same-tick frame-counter behavior are verified enough for this dock-facing slice. Preserve same-tick elapsed `0`; do not compensate locally with `binary_frame - 1`.

2. `MissionDeployBuildingState` under `Miner`

   Add a dock/unload mission state bundle that mirrors the verified mission `0x10` slice:

   - mission id/substate equivalent for `+0xBC`;
   - mission retry delay for the not-ready return `5`;
   - unload-active latch equivalent to `+0x6D1`;
   - dump accumulator equivalent to `+0xF8`;
   - timer cluster fields equivalent to `+0x100`, `+0x104`, `+0x108`, `+0x10C`.

   These fields should live in `Miner` while only harvester refinery unload uses them. They can later move to a generic mission component if multiple systems require exact MissionClass layout.

   Post-swarm status: accepted-path mission timer epilogue, return-delay storage, `+0x110`, and accumulator ordering are resolved. `+0x104` is resolved as non-cadence opaque scratch for this slice; exact runtime byte content remains unchecked and must not be used for gameplay/timing decisions.

3. Dock contact state cleanup

   Keep `contacts`, `waiting_retry_queue`, and `contact_entered`.

   Reclassify `on_pad`:

   - remove it from normal stock zero-link unload gating if possible;
   - if retained for Rust-internal safety, rename or document it as internal unload-active bookkeeping, not a stock `+0x2E4` physical pad link;
   - tests must not claim `on_pad` is gamemd reciprocal pad occupancy.

4. Unload visual state

   Replace direct `display_type_override` as the simulation source of truth with the unload-active latch. Rendering can still consume `display_type_override` during the bridge, but it must be derived from the latch, not independently started by radio/phase side effects.

### Interfaces / Contracts

- `drive_do_turn_rate_timer(entity, target)`:
  - valid only when active locomotor is Drive;
  - sets the Drive-owned RateTimer target/current according to verified or later-verified RateTimer semantics;
  - no body-facing snap.

- `drive_rate_timer_accepts_deploy(entity)`:
  - reads active Drive RateTimer current;
  - applies `((current >> 7) + 1) & 0x1FE == 0x80`;
  - returns boolean only.

- `run_mission_deploy_building_gate(...)`:
  - runs only after `0x15` queues mission `0x10`;
  - path gate first, with the exact verified `RadioClass::In_Radio_Contact` branch polarity and cleanup return behavior documented in the implementation plan before code is written;
  - RateTimer accept gate second;
  - not-ready path calls `drive_do_turn_rate_timer(0x4000)` and stores/returns delay `5`;
  - accepted path calls unload-start initializer and then follows the verified mission timer epilogue/return cadence.

- `start_unload_latch(...)`:
  - writes the verified mission/unload fields in order;
  - sets derived visual override from `UnloadingClass`;
  - does not write body facing;
  - does not emit stock `DockDeploy`.

### Data Flow

Normal successful stock flow:

1. `MissionEnter` sends/receives accepted `0x12`.
2. Already-there reply marks `contact_entered` through `0x18`.
3. First `0x16` calls active Drive `do_turn_rate_timer(0x4000)` and returns.
4. Later synced `0x16` sends `0x15` under stopped/destination/contact/mission gates.
5. `MissionQueued` records mission `0x10` without unload side effects.
6. `MissionDeploy` gate checks path validity.
7. `MissionDeploy` gate checks Drive RateTimer window.
8. Not ready: call `do_turn_rate_timer(0x4000)`, set mission delay `5`, stay not unloading.
9. Ready: initialize unload latch/timer/substate, derive UnloadingClass display, then follow the accepted-path mission timer epilogue/return cadence before subsequent state-3 drain processing.
10. State 3 drains resource slots using the dump accumulator threshold.
11. Empty-slot gate writes state 4.
12. State 4 clears unload latch/display and returns to harvest scheduling.

### Error Handling

- Missing active Drive locomotor during stock dock `0x16` or mission `0x10` is a Rust invariant failure for HARV/CMIN, not a designed gameplay branch. The implementation should assert/test this invariant and recover only through an explicitly researched path; it must not silently snap facing or invent an abort behavior.
- Missing refinery during state 3 follows existing missing-refinery abort rules from the research docs; do not introduce credit drain on missing building.
- Old saves with `dock_pivot_facing` should deserialize and either drop that state or migrate to a conservative "not ready, call Do_Turn on next mission dispatch" state.

### Testing Strategy

Focused tests to add or rewrite:

- `dock_radio_0x16_sets_drive_rate_timer_without_body_facing_write`
- `dock_radio_0x16_first_sync_does_not_queue_unload`
- `mission_deploy_not_ready_calls_drive_doturn_and_delays_five`
- `mission_deploy_accepts_rate_timer_window_not_exact_facing_byte`
- `mission_deploy_unload_start_does_not_snap_body_facing`
- `mission_deploy_path_gate_blocks_unload_init`
- `mission_deploy_path_gate_uses_verified_has_valid_steps_polarity`
- `mission_deploy_accepted_path_preserves_return_delay_and_rng_contract`
- `mission_0x10_return_delay_blocks_reentry_until_elapsed_gte_duration`
- `mission_deploy_facing_not_ready_reschedules_five_without_rng`
- `mission_deploy_unload_start_reschedules_14_to_16_and_consumes_one_rng`
- `mission_deploy_unload_start_writes_latch_before_first_drain`
- `mission_deploy_unload_start_initializes_timer_cluster_exactly`
- `mission_deploy_unload_start_preserves_accumulator_step_one`
- `unload_accumulator_step_defaults_to_one_for_harv_and_cmin`
- `techno_ai_update_unload_cluster_increments_f8_by_step_one`
- `unload_start_same_ai_pass_does_not_increment_dump_accumulator`
- `mission_deploy_state3_reads_accumulator_before_current_ai_increment`
- `unload_cluster_increments_once_on_next_binary_frame`
- `dock_unload_cadence_ignores_timer_cluster_0x104`
- `mission_deploy_unload_start_does_not_emit_dockdeploy`
- `stock_zero_link_unload_does_not_set_physical_on_pad_link`
- `cmin_and_harv_share_drive_owned_dock_sync`
- update existing East-snap tests to assert no direct facing snap.

Regression tests to keep:

- accepted `NW+(3,1)` and `QueueingCell=4,1` remain separate;
- CMIN far return still stages at `QueueingCell`;
- CMIN close return still enters radio dock path;
- War Miner still never teleports;
- two-miner waiter is not refinery-promoted and retries on its own due mission timer;
- healthy stock exit still does not use `Force_Track(0x47)`.

## Architectural Decisions

- Keep the first byte-field model inside `Miner` and `LocomotorState`, not a global ECS MissionClass refactor. This follows the existing sim ownership boundaries and avoids a broad rewrite while still mapping the verified gamemd fields.
- Put RateTimer ownership under active Drive locomotion because gamemd calls active locomotor `+0x4C`, and CMIN now has the correct active Drive piggyback bridge.
- Treat `display_type_override` as derived presentation state during the bridge. The simulation source of truth becomes the unload-active latch.
- Do not implement exact `+0x104` byte content from intuition. The post-swarm reports prove it is non-cadence opaque scratch for this slice, but they do not prove the exact runtime stack value. If a patch serializes or hashes `+0x104`, mark that byte as unchecked until a runtime/value trace closes it.

## Alternatives Considered

### A. Miner-local timer bridge

Rejected. It would be smaller, but it keeps dock `0x16` owned by the miner FSM instead of active Drive locomotion. That is the same architectural drift that caused the current East-pivot mismatch.

### B. Locomotor-owned RateTimer bridge only

Rejected for the user-selected scope. It is a good incremental patch, but it would leave mission `0x10` byte fields, unload latch, `DockDeploy`, and physical `on_pad` drift for a later patch.

### C. Full byte-field dock/unload slice

Chosen. It gives every verified dock/unload detail a home and prevents another round of "correct high-level outcome, wrong state owner" drift.

## Follow-Up Gate

The original proof-gate follow-up was completed by the 2026-05-26 Plan C swarm. Implementation may proceed for the verified stock dock timing/behavior slice.

Remaining gates before claiming wider byte-perfect parity:

1. Runtime/value trace for exact `+0x104` stack-scratch contents if this field is serialized, hashed, exposed, or compared as byte state.
2. Save/load serializer audit for `+0x104` / `+0x110` if old/new save compatibility is part of the patch.
3. Runtime first-deposit frame trace if the implementation needs a reference replay frame, not just the verified static mechanism.

Implementation checklist:

1. Add serialized fields and compatibility defaults for mission `0x10` delay and the unload accumulator cluster.
2. Add or reuse RateTimer API while preserving same-tick elapsed `0`.
3. Replace dock `0x16` sync with active Drive RateTimer semantics and no body-facing snap.
4. Replace mission `0x10` gate and passive delay storage.
5. Replace unload-start latch/timer initialization and remove snap/sound/on-pad drift.
6. Replace unload countdown with accumulator update/check ordering.
7. Update tests listed above and remove stale assertions that encode the old approximation.
