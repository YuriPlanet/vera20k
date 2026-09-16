# NavCom / Head_To_Coord Drive Phase 1 Design

## Goal

Model normal ground Drive move-to-cell destination ownership like gamemd: owner `NavCom` and Drive destination/head-to state are separate from active path/track execution.

## Architecture Context

Current Rust collapses several native concepts into `MovementTarget`. `GameEntity.movement_target` stores the path, speed budget, retry timers, and `final_goal`; `DriveTrackState` stores curve stepping; selected target lines read `MovementTarget.final_goal` or the final path cell. Arrival currently clears `movement_target` and `drive_track` together in `movement_tick::finalize_finished_entities`.

The verified gamemd model is different:

- Foot owner state stores `NavCom_Aux` and `NavCom`.
- `FootClass::Set_Destination_Internal` owns the destination commit lifecycle.
- The active locomotor receives `Head_To_Coord` after `NavCom` is written.
- Drive stores destination, head-to/intermediate coordinate, speed fraction, residual, active track index, track point, and valid flag separately.
- `RadioClass::In_Radio_Contact` (formerly mislabeled PathType__Has_Valid_Steps) is separate from `NavCom`.
- Selected action lines read live target state: `ArchiveTarget`, else last `NavQueue` item, else `NavCom`.

Relevant Rust surfaces:

- `src/sim/game_entity.rs`: central deterministic entity state.
- `src/sim/components.rs`: existing `MovementTarget` and candidate home for owner/locomotor state structs.
- `src/sim/movement/movement_commands.rs`: current move command computes path and attaches `MovementTarget`.
- `src/sim/movement/movement_tick.rs`: current movement loop and finalization.
- `src/sim/movement/drive_track.rs`: existing Drive curve tables and runtime `DriveTrackState`.
- `src/app_target_lines.rs`: selected move line endpoint currently reads `MovementTarget`.
- `src/sim/world/world_commands.rs` and `src/sim/world/world_orders.rs`: command paths that issue or clear movement.

## Impact Analysis

This change touches common movement state, so the blast radius is moderate:

- Save/load and deterministic hashing change because `GameEntity` gains new serialized fields.
- Movement commands must distinguish "destination accepted" from "path execution attached."
- Arrival cannot blindly equate `movement_target == None` with destination cleared.
- Target-line visuals must switch to live navigation target state.
- Stop/cancel paths should clear owner nav through a helper, not only remove movement execution.

Risk areas:

- Tests and systems that use `movement_target.is_some()` as "moving or has destination" need review.
- Infantry animation still legitimately keys on `MovementTarget`; do not change it to `NavCom`.
- Air, Jumpjet, Teleport, refinery/dock, and chrono-miner cases must not be silently forced into this Phase 1 model unless their native path is verified.
- `NavQueue` producer semantics are not fully verified for all player commands; Phase 1 may add storage and consumer behavior without claiming full queue production parity.

## Chosen Approach

Use a gamemd-shaped owner state while keeping current path execution mostly intact.

Add owner navigation state on `GameEntity`, separate from `MovementTarget`:

- `nav_com_aux: Option<NavTargetRef>`
- `nav_com: Option<NavTargetRef>`
- `nav_queue: Vec<NavTargetRef>`

Add Drive locomotor state separate from `DriveTrackState`:

- destination coord triplet
- head-to coord triplet
- head-to/track valid flag
- current speed fraction placeholder, initially enough to preserve clear/clamp semantics

Then refactor normal ground Drive move-to-cell command flow:

1. Run existing preflight/goal/path checks where Rust currently must know the effective target.
2. Call a Rust equivalent of `Set_Destination_Internal(non_null, force)` for accepted normal cell targets.
3. That helper clears `nav_com_aux`, applies modeled non-null guards, writes `nav_com`, dispatches a Drive destination setter, and resets retry/block fields.
4. Attach/update `MovementTarget` as the execution path under that owner destination.
5. On empty-queue arrival, call the null destination helper instead of treating path exhaustion as owner destination deletion.
6. Keep `MovementTarget` as path/segment execution. It may be absent while `NavCom` is still visible.

This approach is preferred because every verified owner/locomotor/detail item has an explicit home without dragging refinery/dock/chrono/aircraft call chains into the first implementation.

## Tiny-Detail Ledger

- UnitClass normal Drive destination enters `0x00741970`, then falls through to `FootClass::Set_Destination_Internal @ 0x004D94B0` for normal empty-cell move commands. Source: [docs/research/UNITCLASS_SET_DESTINATION_NORMAL_DRIVE_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_SET_DESTINATION_NORMAL_DRIVE_CELL_GHIDRA_REPORT.md).
- Same-destination early return happens before reset/commit when target equals current `NavCom` and the relevant byte gate is clear. Source: [UNITCLASS_SET_DESTINATION_NORMAL_DRIVE_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_SET_DESTINATION_NORMAL_DRIVE_CELL_GHIDRA_REPORT.md).
- `Set_Destination_Internal` clears `Foot+0x5A0` (`NavCom_Aux`) before non-null guards. Source: [docs/research/FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md).
- Non-null target silently returns before `NavCom` write when `Foot+0x6AD`, `Foot+0x82`, or `Foot+0x2E4` is set. Source: [FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md).
- `NavCom` is `Foot+0x5A4` and is written only after those guards. Source: [FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md).
- Non-null `NavCom` dispatches endpoint coordinate through target vtable coordinate getter, then active locomotor vtable `+0x44`, unless the one-shot skip-head-to byte is set. Source: [FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_SET_DESTINATION_INTERNAL_NAVCOM_HEADTO_HANDOFF_GHIDRA_REPORT.md).
- Drive vtable `+0x44` resolves to `0x004AFD40`; `0x004AFCC0` is a head-to getter, not the setter. Source: [docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md).
- Drive destination absolute fields are `+0x34/+0x38/+0x3C`; head-to absolute fields are `+0x40/+0x44/+0x48`. Source: [DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md).
- Drive destination setter applies bridge Z adjustment `ftol(g_DriveHeightStep * 4)` when destination cell has `CellClass+0x140 & 0x100`. Source: [DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md).
- Drive `Stop_Moving @ 0x004AFE00` clamps current speed to max `0.3` and clears destination only; it does not clear head-to, active track, point index, or owner `NavCom`. Source: [DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md).
- Drive moving predicate can still report moving when destination is null but head-to XY differs from owner XY; Z is ignored for that equality. Source: [DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md).
- Drive track completion clears Drive head-to/track state before owner `NavCom` is necessarily cleared. Source: [docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md).
- Empty-queue normal arrival calls owner `Set_Destination(NULL,1)` and returns; it does not call `OnArrival` in that branch. Source: [DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md).
- Non-empty queue arrival calls `FootClass::Stop_Moving`, then owner `OnArrival(0,1)`, which pops the first queued target and calls `Set_Destination(next,0)`. Source: [DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md).
- `RadioClass::In_Radio_Contact @ 0x0065AE30` scans path array entries and is not equivalent to `NavCom`, `NavQueue`, or Drive destination state. Source: [UNITCLASS_SET_DESTINATION_NORMAL_DRIVE_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_SET_DESTINATION_NORMAL_DRIVE_CELL_GHIDRA_REPORT.md).
- Selected action-line draw requires `ArchiveTarget || NavCom`; `NavQueue` alone does not draw. Source: [docs/research/NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md).
- Selected action-line endpoint priority is `ArchiveTarget`, else `NavQueue.Items[Count - 1]`, else `NavCom`; movement bridge-Z adjustment applies after endpoint coordinate resolution. Source: [NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_NAVQUEUE_ACTION_LINE_ENDPOINT_VISIBILITY_GHIDRA_REPORT.md).

## Design

### Components

`NavTargetRef`

Represents the Rust equivalent of a native `AbstractClass*` target reference for the Phase 1 subset.

Initial variants:

- `Cell { rx: u16, ry: u16 }`
- `Entity { id: u64 }`

Phase 1 uses `Cell` for normal move-to-cell. `Entity` exists because selected action lines and later attack/archive/building destinations need the same endpoint shape; it should not imply full building/dock destination parity is complete.

`NavigationState`

Owner-level Foot navigation fields:

- `nav_com_aux: Option<NavTargetRef>`
- `nav_com: Option<NavTargetRef>`
- `suspended_nav_com: Option<NavTargetRef>`
- `nav_queue: Vec<NavTargetRef>`
- retry/block timer fields that currently live in `MovementTarget`, or bridge fields that reset both owner-level and execution-level timers during transition.

Implementation can either store these as separate `GameEntity` fields or one `navigation: NavigationState` field. Prefer one struct to keep owner navigation state auditable.

`DriveLocomotionRuntime`

Drive-specific state added near locomotor code:

- `destination: Option<Coord3D>`
- `head_to: Option<Coord3D>`
- `track_valid: bool`
- `current_speed_fraction: SimFixed`

`DriveTrackState` remains the curve adapter and keeps raw track index, point index, residual, transforms, and target facing. Do not fold `DriveTrackState` into `DriveLocomotionRuntime` during Phase 1.

### Interfaces / Contracts

Add helper functions under `sim/movement` or a new `sim/movement/navcom.rs`:

- `set_destination_internal_cell(entity, target, force, context) -> DestinationResult`
- `set_destination_internal_null(entity, force) -> DestinationResult`
- `foot_stop_moving(entity)`
- `drive_set_destination(entity, coord, resolved_terrain)`
- `drive_stop_moving(entity)`
- `selected_navigation_endpoint(entity) -> Option<NavTargetRef>`

Contracts:

- `set_destination_internal_cell` writes owner `NavCom` before Drive destination.
- `set_destination_internal_null` clears owner `NavCom` before Drive clear-navigation.
- Pathfinding failure and silent-dropped destination are different outcomes.
- `MovementTarget` is not the owner destination.
- `DriveTrackState == None` is not proof that destination or head-to is null.

### Data Flow

Normal move-to-cell:

1. `world_commands` receives move command.
2. `issue_move_command_with_layered` resolves effective target and path.
3. For normal ground Drive, it calls `set_destination_internal_cell`.
4. The helper clears `nav_com_aux`, writes `nav_com`, computes target coord, writes Drive destination, resets retry/block state.
5. `issue_move_command_with_layered` attaches `MovementTarget` for current execution path and starts DriveTrack when appropriate.
6. `app_target_lines` reads `attack_target` first, then `nav_queue.last()`, then `nav_com`.

Path/track finish:

1. Track completion may clear Drive head-to/track execution state.
2. Owner `nav_com` remains until arrival lifecycle explicitly clears or replaces it.
3. Empty-queue arrival calls `set_destination_internal_null(entity, true)`.
4. Non-empty queue arrival calls `foot_stop_moving`, then queue pop/reissue through `set_destination_internal_cell(next, false)`.

Stop command:

1. Cancel command-owned active path and combat/order state as today.
2. Route owner navigation clear through the null destination helper.
3. Preserve special locomotor cancellation behavior already handled by stop paths.

### Error Handling

No panics for missing entities or invalid targets. Existing command functions continue returning `bool`.

Represent destination command results explicitly:

- `Accepted`
- `NoEntity`
- `RejectedByGuard`
- `NoPath`
- `NoWalkableGoal`

Only accepted normal non-null destinations should write `NavCom`.

### Testing Strategy

Focused unit tests:

- `test_normal_drive_move_sets_navcom_before_path_execution`
- `test_normal_drive_move_writes_drive_destination_not_head_to`
- `test_path_finish_does_not_clear_navcom_until_null_destination`
- `test_empty_queue_arrival_clears_navcom_through_null_destination`
- `test_stop_command_clears_navcom_and_drive_destination`
- `test_selected_action_line_uses_navqueue_last_else_navcom`
- `test_selected_action_line_navqueue_without_navcom_does_not_draw`
- `test_selected_action_line_archive_target_wins_over_navcom`
- `test_drive_stop_moving_clears_destination_not_head_to`

Regression tests should also keep the earlier DriveTrack start tests so the first path leg still uses Drive tracks after this ownership split.

### Determinism

All new sim state must live under `GameEntity` and serialize deterministically. `NavQueue` order is gameplay-visible and must preserve insertion order. `EntityStore` stays `BTreeMap<u64, GameEntity>`.

Use fixed-point or integer coordinate types only. No floating point in sim-side Drive/NavCom state.

## Architectural Decisions

- Follow existing plain-struct `GameEntity` pattern instead of adding ECS or external state.
- Keep `MovementTarget` for path execution to avoid rewriting all pathing at once.
- Add owner navigation state separately because gamemd owner fields are separate from path arrays and Drive state.
- Add Drive destination/head-to state separately because gamemd Drive `Stop_Moving`, `Is_Moving`, and track completion depend on those distinctions.
- Do not implement refinery/dock/chrono/aircraft destination paths in Phase 1. They are deferred because they have verified different call chains and prerequisite radio/mission/piggyback systems, not because they are unimportant.

Tech debt:

- During transition, retry/block timer ownership may temporarily be mirrored between `NavigationState` and `MovementTarget`. The intended final state is owner-level timers with execution path reading/writing through helper methods.
- `NavTargetRef::Entity` may exist before every native target type is fully supported. Tests must avoid claiming unsupported target classes.

## Alternatives Considered

### A. Owner NavCom + Drive locomotor state

Chosen. Best parity fit. Every ledger item has a state owner and lifecycle hook.

### B. Store `nav_goal` inside `MovementTarget`

Rejected. It keeps the wrong ownership model: destination would still vanish when path execution is removed. This is confirmed drift against `NavCom` visibility and action-line behavior.

### C. Full FootClass / DriveLocomotion object model now

Deferred. It is likely the long-term direction, but first implementation would pull in docks, radio, chrono piggyback, aircraft, and mission arrival dispatch. Phase 1 should install the correct ownership boundary without pretending those special systems are done.

## Implementation Handoff

Recommended implementation order:

1. Add `NavTargetRef`, owner navigation state, and Drive destination/head-to state.
2. Add helper functions for non-null cell destination, null destination, foot stop, and Drive destination/stop.
3. Route normal ground Drive move-to-cell through the non-null helper.
4. Update selected action lines to read `ArchiveTarget -> NavQueue last -> NavCom`.
5. Change path-finish/arrival finalization so `MovementTarget` removal does not automatically clear owner destination.
6. Add focused tests for the ledger items above.

Do not start refinery/dock/chrono in this task unless a separate implementation contract is written for that scope.
