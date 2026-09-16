# Per-Object Ground Movement and Drive Process Design

**Status:** APPROVED architecture; Checkpoint A CLOSED for the inert/test-only host harness; production activation BLOCKED on the remaining research gates in this document.

**Date:** 2026-07-20

**Primary implementation contract:** docs/contracts/2026-07-20-ground-drive-process-track-stepping-implementation-contract.md

## Goal

Make the existing live per-object AI stage own the complete active standard-YR ground-locomotor pass, then run normal Drive movement through a Rust-native owner that preserves the verified gamemd.exe mission-to-locomotor order, Drive Process control flow, track-point arithmetic, immediate same-pass effects, and arrival timing.

This is not a claim that all movement and pathfinding become parity-complete. Static object-pass/Drive scheduling and the ordinary Unit Techno/Mission/Foot host contract are now resolved at Checkpoint-A contract granularity, and an inert cloned-fixture harness is authorized. The full speed integer, accepted-chain metadata, Walk, A*, wall classification, complete Phase-1 population readiness, lifecycle/effect ownership, and executable native oracle remain explicitly named below and still block production activation.

## Architecture Context

### Native authority and order

The active standard-YR object scheduler is a forward walk of the LogicClass live vector. It loads the current pointer, calls vtable +0x5C, advances the index, and reloads the vector count. It does not take a pass-entry snapshot and does not repair the index after an order-preserving compacting removal. Same-pass tail appends can therefore run later in the same pass, while a removal at or before the current index can cause the shifted successor to be skipped.

For a normal eligible Unit/Foot object, the verified active path is:

1. Leaf pre-Foot work.
2. FootClass::AI at 0x004DA530.
3. TechnoClass::AI_Update at 0x006F9E50.
4. The first Techno pre-mission segment through RockingUpdate.
5. IsAlive guard B immediately after RockingUpdate.
6. The remaining Techno pre-mission work.
7. The +0xC4 per-object counter increment.
8. MissionClass::Mission_Dispatch at 0x005B3060, including the selected real mission handler such as FootClass::Mission_Move at 0x004D4200 when its timer permits.
9. The first Techno post-mission segment: passive acquire, bomb detonation, SlaveManager, then CaptureManager.
10. IsAlive guard E after CaptureManager.
11. The remaining Techno post-mission work.
12. Return to Foot and check alive before the locomotor region.
13. Apply Foot's concrete pre-Process gates and call the active locomotor through ILocomotion vtable +0x40.
14. Check owner alive immediately after locomotor Process, then continue eligible Foot/leaf post-work.

The mission-dispatch-before-locomotor boundary is byte-proven by the 0x006FA64F counter store, the 0x006FA655 mission-dispatch call, and the later locomotor call near 0x004DA877. These are active-YR mechanisms, not a TS-legacy path.

### Current Rust split

Current Rust applies commands, then calls Simulation::object_ai_stage. That stage walks the live order and authoritatively commits the mission projection for live non-miner Unit entities. It does not own their locomotor work.

Simulation::advance_tick then takes a live-order snapshot and calls movement::tick_movement_with_grids. That global function:

- refreshes Drive NavCom targets;
- processes active low-bridge tubes;
- processes forced Drive tracks;
- collects movers;
- builds and refreshes blocker caches;
- handles pending Drive arrivals;
- moves all ordinary ground/bridge movers;
- defers some occupancy, chain, crush, arrival, and debug effects;
- performs formation-speed synchronization;
- finalizes finished movement;
- updates generic locomotor phases;
- ticks Hover vertical state.

The result is structurally different from gamemd: missions for multiple objects can run before the locomotor of the first object, and later-object AI cannot see all effects at the native per-object point.

The current hosted Unit bracket is also only a projection of this native spine. `techno_common_pre` is empty. `unit_techno_bracket` checks alive immediately after that empty helper, writes `mission.tick_counter` plus `derived_mission`, then calls `techno_common_post`; it does not execute native Mission_Dispatch/Mission_Move timer semantics, does not place guard B after RockingUpdate, and does not place guard E after passive acquire/bomb/SlaveManager/CaptureManager. `techno_common_post` currently implements only the damage-Spark slice and has no second alive guard. No authoritative Foot pre-Process gate/locomotor/post-Process-alive bracket exists in this host yet.

An older S2b plan treated movement absorption as hash-neutral because mission dispatch was then a shadow marker. That assumption is stale. Mission commitment is now authoritative and hashed for part of the population, so this authority flip can legitimately alter state-hash timelines. Equality to pre-flip Rust is not a parity oracle.

### Current state owners

- EntityStore owns GameEntity storage.
- LogicVector owns active-object iteration order.
- MissionCom owns the committed mission projection and per-object tick counter.
- MovementTarget owns current Rust path and generic movement intent.
- NavigationState owns NavCom-like destination state.
- DriveLocomotionRuntime owns normal Drive destination, path queue, point cursor mirror, speed fractions, residual budget, delay, and tube payload.
- DriveTrackState owns the currently installed raw track transform/state.
- OccupancyGrid owns current cell-list occupancy and a mutation generation.
- movement_commands.rs currently crosses the owner boundary by installing the first normal Drive track during command dispatch.
- The renderer directly consumes entity position and facing, so simulation drift is presented without a compensating render layer.

## Impact Analysis

| Surface | Design responsibility | Primary risk |
|---|---|---|
| src/sim/world/techno_ai.rs | Retain the live scheduler and place the ground locomotor after the exact segmented Techno/mission/Foot bracket | Flattening guard B/E into one pre/post helper pair; treating `derived_mission` as Mission_Dispatch; double mission commit |
| src/sim/world/mod.rs | Supply object-pass inputs and retire the standalone production ground-movement call | Phase reordering; double processing; adjacent service placement |
| src/sim/movement/ground_pass.rs (new) | Own transient pass scratch state and one-object ground locomotor orchestration | Hidden authoritative state in a cache or deferred list |
| src/sim/movement/movement_tick.rs | Be mechanically decomposed; production all-mover authority is retired at the flip | Losing prelude state, post-effect ordering, or current test access |
| src/sim/movement/drive_locomotion.rs | Own the verified normal Drive Process branch order | Scattered cadence, arrival, retry, or speed authority |
| src/sim/movement/drive_track.rs | Remain the raw-table/point arithmetic kernel | Cursor order, residual boundaries, metadata overreach |
| src/sim/movement/movement_step.rs | Retain reusable non-owner stepping/crossing helpers | Keeping Drive owner decisions in a generic helper |
| src/sim/movement/movement_commands.rs | Stop at destination/NavCom/path intent for normal Drive | Recreating the first track too early |
| src/sim/components.rs | Keep DriveLocomotionRuntime canonical; avoid duplicate persisted authority | Save/hash divergence between mirrors |
| src/sim/movement/navcom.rs | Centralize null destination, stop, arrival, and queue splits | Generic finalization that erases native branch identity |
| src/sim/movement/tube_movement.rs | Provide the active low-bridge per-object branch | Conflating active YR tubes with dormant TS subterranean movement |
| src/sim/movement/bump_crush.rs and occupancy helpers | Commit later-object-visible effects at the owning object point | Wrong victim lifecycle, sound coordinate/order, RNG consumption |
| src/sim/miner | Make the existing Harvest mission seam callable for one live miner before its locomotor | Retaining the later global miner snapshot as mission authority |
| src/sim/world/world_hash.rs and src/sim/snapshot.rs | Preserve full authoritative movement coverage; version the authority flip | Treating a Rust regression hash as gamemd parity evidence |
| Renderer handoff | Remain unchanged | Hiding sim drift with render offsets or smoothing |

The blast radius includes occupancy arbitration, pathfinding cache visibility, Scenario RNG order, crush/scatter, wall and bridge state, factory/gate contacts, sounds, deferred deletion, mission arrival, save/load, replay hashes, and direct rendering.

The design does not require a new persistent movement component. The new pass context is transient. The atomic production flip must increment the current SNAPSHOT_VERSION once under the project authority-flip convention because the meaning and timing of persisted/hashed movement state changes, even if the serialized struct layout does not.

## Chosen Approach

Extend the existing object-AI pass with an owned, short-lived GroundMovePassState and an immediate process_for_object call after each Foot object's completed Techno/mission bracket.

This is preferred because:

- LogicVector and for_each_live_object already implement the native live-order contract.
- The current Techno-AI stage already owns the authoritative non-miner Unit mission commit.
- It creates one scheduler owner rather than a second movement scheduler.
- It supports Rust-native cache ownership and borrow boundaries without copying the C++ inheritance/COM architecture.
- It lets occupancy, lifecycle, RNG, and arrival effects commit before the next object.
- It provides a stable owner in which the verified Drive Process sequence can live.

The final production flip is atomic across the complete current Phase-1 ground population. Mechanical extraction and readiness seams can land without changing production behavior, but no production state may process some ground mover categories per object while leaving the rest in the global pass.

This explicitly reconciles the scheduling report's ordinary-Drive-first handoff with the approved architecture. Ordinary Drive may be the first code extracted, unit-tested, or exercised on cloned/shadow state because its native call order is now proven. It may not become a live production authority first. A vehicle-only active flip is known DRIFT unless this approved design decision is explicitly changed.

## Tiny-Detail Ledger

Every approach and implementation task must preserve or explicitly block on the following details.

- **Active target:** stock Yuri's Revenge standard-skirmish Foot objects using active Drive, Walk, Hover, and Ship locomotors. Drive activation is proven by the stock AMCV Drive CLSID reaching 0x004B0500. [GHIDRA 0x004DA530, 0x004B0500; doc: DRIVE_PROCESS_MOVEMENT_TICK_ORDER_GHIDRA_REPORT.md]
- **TS/YR split:** low-bridge TubeClass traversal is active YR map behavior; subterranean Tunnel, Mech, and DropPod locomotor paths are dormant TS legacy in stock YR and are not substituted for low-bridge tubes. [doc: [TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_FOOTCLASS_SUBSTRATE_SERVICE_DESIGN.md) invariants N3 and ledger row 2; docs/research/bridges/04-locomotion-height-tubes]
- **Live iteration:** use the live LogicVector and reload its length after every object. Do not snapshot, sort, repair the index, or use EntityStore order. [GHIDRA 0x0055B5FB..0x0055B619; doc: [LOGICCLASS_PERTICKUPDATE_SCHEDULER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_PERTICKUPDATE_SCHEDULER_GHIDRA_REPORT.md)]
- **Compaction result:** an order-preserving removal at or before the current index can skip the shifted successor; a tail append can run in the same pass. [GHIDRA 0x0055B5FB..0x0055B619 and 0x0055BAE0; doc: [ACTIVE_VECTOR_REMOVE_HELPER_FUN_0055BAE0_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_VECTOR_REMOVE_HELPER_FUN_0055BAE0_RESWARM_20260528.md)]
- **Segmented Techno sequence:** Techno pre-work is not one helper followed by one guard. Guard B is mid-pre immediately after RockingUpdate; more pre-mission work follows before +0xC4 and Mission_Dispatch. Techno post-work is not one helper followed by one guard. Passive acquire, bomb detonation, SlaveManager, and CaptureManager precede guard E; more post-work follows it. [GHIDRA 0x006F9E50; doc: [TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md)]
- **Real mission dispatch:** `derived_mission` is a Rust projection, not native Mission_Dispatch. The verified Checkpoint-A host contract requires Mission_Dispatch timer gates, virtual mission-handler selection, return-delay storage, and Scenario RNG ownership for Foot Mission_Move. The inert harness may trace those facts on clones; production still does not execute them. [GHIDRA 0x005B3060, 0x004D4200, 0x005B3A00]
- **Foot sequence:** after Techno returns, Foot checks alive, applies the concrete locomotor pointer/state gates, calls the current locomotor once, and checks alive immediately afterward before later Foot/leaf work. [GHIDRA 0x004DA530, call 0x004DA877]
- **Whole ground population:** ordinary Unit, Infantry, miner, Ship, Hover, forced-track, and active low-bridge-tube work must not be split into separate production passes when the authority flips. A split changes occupancy contention and later-object visibility.
- **Locomotor invocation population:** the active ground locomotor owner runs for every eligible live Foot object, including Drive idle/no-MovementTarget cases. It is not selected solely by a pre-collected MovementTarget mover list. [GHIDRA 0x004DA530 and Drive Process 0x004B0500]
- **Command boundary:** a normal move command establishes destination/NavCom/path intent. With no active track, Drive Process calls Process_Movement, which selects the first track and can call Process_Drive_Track in the same invocation. [GHIDRA 0x004B0500, 0x004B2630; doc: DRIVE_PROCESS_MOVEMENT_TICK_ORDER_GHIDRA_REPORT.md]
- **Slope order:** Drive Process samples slope before track/path work. [GHIDRA 0x004B0500]
- **Fresh normal cursor:** Process_Movement writes cursor zero. RawTrack 4 metadata value 11 is not the fresh normal cursor. Accepted-chain and forced-track starts are distinct. [GHIDRA 0x004B2630; live 2026-07-20 decompile]
- **Zero budget:** installing a fresh track with zero budget consumes no point and leaves owner coordinates and facing unchanged. [GHIDRA 0x004B0F20; doc: [AMCV_OPEN_GROUND_DRIVE_RETRACE_20260720.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/AMCV_OPEN_GROUND_DRIVE_RETRACE_20260720.md)]
- **Speed-before-budget:** Accelerates false directly applies the target fraction; Accelerates true runs the ramp/brake branch; SetSpeedFraction clamps to [0,1]. This happens once per Process_Drive_Track invocation before GetCurrentSpeed. [GHIDRA 0x004B0F69..0x004B1274; doc: [DRIVE_PROCESS_DRIVE_TRACK_SPEED_BUDGET_RESIDUAL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_DRIVE_TRACK_SPEED_BUDGET_RESIDUAL_GHIDRA_REPORT.md)]
- **One invocation opportunity per eligible object turn:** speed mutation and point budget share every reached Drive Process/DriveTrack invocation. There is no independent 15 Hz Drive admission gate between the live-object turn and Drive movement. A whole object turn that is not reached or fails Foot's gates performs no Drive mutation. [doc: [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md)]
- **Budget formula:** normal call uses fresh GetCurrentSpeed plus stored residual. A same-process retry still repeats DriveTrack's pre-budget speed-state work and calls GetCurrentSpeed, then masks that fresh integer contribution and uses residual only. [GHIDRA 0x004B0F20; doc: [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md)]
- **Point cost and bound:** each point costs exactly 7, and the loop condition is strict budget greater than 7, not greater-than-or-equal. [GHIDRA 0x004B0F20]
- **Point/cursor order:** subtract 7, read and apply the current point, perform its state effects, then increment the cursor. [GHIDRA 0x004B0F20; doc: [DRIVE_APPLY_TRACK_DELTA_POINT_RESIDUAL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_APPLY_TRACK_DELTA_POINT_RESIDUAL_GHIDRA_REPORT.md)]
- **Residual interpolation:** when a track remains active, residual interpolation uses residual times 1/7 under the strict residual greater than 3 trust gate and does not update facing. [GHIDRA 0x004B0F20 and facing call 0x004B1AC1]
- **Cell continuation:** ordinary cell/occupancy effects occur inside the point body. After committing them and incrementing the cursor, the loop continues while budget remains greater than 7 unless a verified native branch exits. [GHIDRA 0x004B0F20; doc: [MTNK_STATIC_WALL_DETOUR_RETRACE_20260720.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/MTNK_STATIC_WALL_DETOUR_RETRACE_20260720.md)]
- **Same-process chain:** active-track completion can run Process_Movement and Process_Drive_Track(retry) in the same Drive Process. Retry repeats pre-budget speed-state work, but its freshly computed integer speed is masked before residual-only consumption. [GHIDRA calls 0x004B0576, 0x004B0A79, 0x004B0AAA; doc: [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md)]
- **Arrival owner:** empty-queue arrival follows Set_Destination(NULL,1) through stop/clear on the next eligible no-active-track Drive Process. The non-empty queue path is distinct; a separate global arrival sweep is not the native owner. [GHIDRA 0x004B0500, 0x004B0F20; doc: [NAVCOM_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVCOM_LIFECYCLE_GHIDRA_REPORT.md) section 5]
- **Immediate visibility:** occupancy, path-cache invalidation, scatter RNG, crush/uninit, wall, gate/contact, and arrival state that a later live object can read must commit at the verified per-object point. A borrow-conflict queue is not a semantic reason to delay an effect.
- **Crush lifecycle:** active Unit PerCellProcess can kill/uninit a crushable victim during the mover's cell process; native UnInit conceals/unregisters before pending-delete storage is freed at the tail. [GHIDRA UnitClass::PerCellProcess 0x00741700, ObjectClass::UnInit 0x005F65F0; doc: [CRUSH_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CRUSH_SYSTEM_GHIDRA_REPORT.md)]
- **RNG:** Scenario RNG is consumed at the same per-object branch and in live-object order. Pure cache preparation consumes no RNG.
- **Render handoff:** renderer consumes the corrected native coordinate/facing state without a compensating render hack. [doc: [AMCV_OPEN_GROUND_DRIVE_RETRACE_20260720.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/AMCV_OPEN_GROUND_DRIVE_RETRACE_20260720.md)]
- **Unknown full speed integer:** the complete FootClass::GetCurrentSpeed input set, widths, rounding, house/veterancy/ability modifiers, and all stock/mod combinations remain UNKNOWN pending focused RE. [GHIDRA helper 0x004DB1A0; BLOCKED contract row]
- **Resolved static schedule, bounded runtime uncertainty:** one reached Main_Tick has one live-object pass; each eligible Foot turn offers one locomotor Process call; normal Drive has no separate 15 Hz movement gate; `g_CurrentFrameCounter` increments later once per normal reached tick. Stock local `GameSpeed=1` supplies a one-bucket 16 ms timer-domain budget, but realized retail jitter/throughput remains runtime-unmeasured and is not a license to retain or invent a Drive divisor. [doc: [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md)]
- **Unknown RawTrack roles:** raw bytes are agreed, but accepted-chain metadata names and start values conflict across older reports. Do not globally reinterpret or zero every initializer. [BLOCKED contract row; [DRIVE_TRACK_TABLES_DEEP_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_TRACK_TABLES_DEEP_DECODE.md) versus newer apply-track evidence]
- **Known Walk drift:** active Walk uses pre-move sub-cell selection/marking, continuous heading toward the chosen sub-cell, Set_Speed(1.0), action-timer animation, and a less-than-17-lepton completion sequence. Current Rust differs. Relocation does not certify Walk parity. [GHIDRA 0x0075AC80, 0x0075AEC0; doc: [E2_STATIC_WALL_WALK_RETRACE_20260720.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/E2_STATIC_WALL_WALK_RETRACE_20260720.md)]
- **Known pathfinding drift:** dynamic wall Can_Enter_Cell codes, A* cost/tie mechanism, and straight-segment optimization differ in current Rust; the literal one-wall native route is unverified when wall ownership/state is unspecified. Relocation does not certify A* parity. [GHIDRA 0x0042C900, 0x0042B7F0, Unit Can_Enter_Cell; MTNK/E2 retraces]

## Design

### Components

#### Existing object-AI scheduler

Simulation::object_ai_stage remains the sole production owner of the per-object live walk. It must continue through Simulation::for_each_live_object so membership changes retain native same-pass effects.

techno_ai.rs owns ordering only:

- category/leaf dispatch;
- the segmented Techno/mission bracket, including the exact positions of guard B and guard E;
- real Mission_Dispatch/mission-handler invocation rather than only a derived mission projection;
- Foot's post-Techno alive check, concrete pre-Process gates, and immediate post-Process alive check;
- the call to the active Foot locomotor owner at the verified slot;
- leaf post-Foot work.

It must not absorb pathfinding, Drive table arithmetic, occupancy algorithms, or locomotor-specific state machines.

#### Object pass inputs

advance_tick passes a compact, read-only object-pass input containing:

- rules;
- path grid;
- overlay registry where needed;
- host tick duration;
- simulation tick;
- committed binary frame;
- path and blockage delay configuration.

This input does not own authoritative simulation state. Mutable state stays on Simulation and its substrate so per-object effects use existing lifecycle, occupancy, RNG, sound, and world APIs.

#### GroundMovePassState

A new src/sim/movement/ground_pass.rs owns short-lived scratch state for one object pass. It may contain:

- lazily built per-owner blocker maps;
- blocker-neighbor counts;
- occupancy/passability generation stamps;
- already-scattered bookkeeping scoped to the native branch;
- movement statistics and non-authoritative diagnostics;
- other scratch values positively proven not to be gameplay authority.

It must not contain:

- a mover snapshot;
- a finished-entity list used for later gameplay cleanup;
- a deferred Drive-arrival list;
- a deferred crush-kill list;
- an ordered vector of cell effects;
- a second persistent cursor, residual, speed fraction, mission, destination, or occupancy authority.

Caches are lazy because a mission can establish movement just before locomotor Process, and same-pass membership/occupancy changes can create or invalidate facts after pass preparation. Every cache records the mutation generation it reflects. Occupancy, lifecycle, wall, bridge, or other passability changes invalidate it before another object consumes it.

#### Per-object ground dispatcher

ground_pass::process_for_object receives Simulation, the current stable ID, pass inputs, and GroundMovePassState. It resolves the current active locomotor rather than selecting solely by MovementTarget.

Ownership classification:

- Drive, Walk, Hover, Ship: owned by the ground pass.
- Forced Drive track: distinct Drive branch.
- Active low-bridge tube: distinct Drive/tube branch.
- Fly, Jumpjet, Teleport, Rocket, Parachute and other active special locomotors: not processed here; they remain a named later migration and must never be double-processed.
- Tunnel, Mech, DropPod TS-legacy stock-dormant paths: excluded from the active-YR ground contract.

The production authority flip is blocked until all currently Phase-1-owned active ground branches have per-object entry seams.

There is no interim production skip-by-ID protocol. During mechanical extraction, the existing global wrapper remains the sole live authority and may call newly extracted one-object helpers in its existing order. Per-object-host exercises are read-only shadows or operate on cloned fixtures. At the atomic flip, the live object host activates every prepared Phase-1 ground handler and the entire standalone `tick_movement_with_grids` production call is removed in the same behavior-bearing change. A `BTreeSet` of "already handled vehicles" that leaves Infantry, miner, Hover, Ship, tube, forced-track, or postlude effects in the bulk path is forbidden known DRIFT.

#### Drive process owner

drive_locomotion.rs owns the normal Drive state machine. It is responsible for:

- one invocation per eligible reached Foot object turn, behind Foot's verified gates;
- slope-before-movement ordering;
- active-track versus no-active-track branch selection;
- Process_Movement ownership;
- Process_Drive_Track normal/retry calls;
- speed fraction update;
- fresh integer budget request;
- residual ownership;
- same-process continuation;
- NavCom/head-to transitions;
- arrival branch identity;
- owner-alive rechecks after effects.

The function must model the verified control-flow contract without recreating COM, raw pointers, vtables, or C++ inheritance.

#### Drive track kernel

drive_track.rs remains the immutable raw table and fixed/integer point-transform kernel. It:

- selects verified track table rows;
- installs a caller-specified initializer kind;
- consumes the current point;
- transforms sub-cell coordinates/facing;
- computes strict budget/residual/interpolation results;
- reports one borrow-boundary control event when world access is required.

It does not own commands, NavCom, pathfinding, locomotor scheduling, occupancy, lifecycle, sounds, missions, or generic end-of-move cleanup.

Fresh normal, accepted-chain, forced, and other special initializers are separate entry points or an explicit sourced initializer discriminator. Fresh cursor zero must not leak into blocked accepted-chain semantics.

#### Immediate DriveStepEffect

When a point-body action needs whole-world access, the kernel returns one DriveStepEffect rather than a vector. The Drive owner:

1. releases the entity borrow;
2. commits the effect through Simulation/ground-pass helpers;
3. re-resolves the owner and relevant state;
4. invalidates scratch caches affected by the commit;
5. resumes the same Drive Process when the verified control path and residual permit.

This effect exists to cross a Rust borrowing boundary. It is not a semantic deferral queue.

#### Command and navigation boundary

movement_commands.rs owns order acceptance, NavCom/destination installation, and path intent. It does not select/install the first normal Drive track.

navcom.rs owns shared destination-null, stop, clear-navigation, and queue transitions. Drive invokes those APIs at its verified arrival point. Walk/Hover/Ship must have their own owner-specific completion paths and may not route through a generic Drive finalizer.

#### Miner and Infantry readiness

The mission layer must become callable for one live object before its locomotor:

- The existing Harvest mission seam must process one miner in its live-object slot and retire the corresponding later global miner mission authority for that behavior.
- Infantry must authoritatively commit/dispatch its mission before Walk Process.

ground_pass does not absorb miner or infantry mission state machines. It consumes the post-dispatch state produced by their mission owner.

### Interfaces / Contracts

#### Object-pass invocation contract

Static native ownership is resolved. One reached Main_Tick contains one live-object pass, and every eligible Foot object turn calls its current locomotor Process once; Drive has no separate 15 Hz movement gate. All Drive speed-state and point-budget work belongs to that Process/DriveTrack opportunity, including the pre-budget speed-state work of a same-invocation retry. `g_CurrentFrameCounter` is the pre-increment value during the pass and advances once in the later normal tail.

Rust may schedule native-equivalent passes from its host loop, but it must not create a second Drive cadence authority or multiply per-call content by `GameSpeed`. Realized retail wall cadence and jitter remain an executable-oracle measurement item, not an open static scheduling mechanism.

#### Per-object ordering contract

For each live entry:

1. Resolve the current ID directly from LogicVector.
2. Tolerate an absent ID.
3. Skip an inactive/dying object.
4. Run leaf pre-Foot work and enter Foot/Techno.
5. Run the first Techno pre-mission segment through RockingUpdate.
6. Apply IsAlive guard B; return if dead.
7. Run the remaining Techno pre-mission segment.
8. Increment the +0xC4 mission tick counter once.
9. Execute Mission_Dispatch and its selected real mission handler when its timer permits.
10. Run passive acquire, bomb detonation, SlaveManager, then CaptureManager.
11. Apply IsAlive guard E; return if dead.
12. Run the remaining Techno post-mission segment and return to Foot.
13. Apply Foot's post-Techno alive check.
14. Apply Foot's concrete pre-Process gates and resolve the current locomotor.
15. Run active ground locomotor Process when this design owns the active kind.
16. Apply Foot's immediate post-Process alive check.
17. Run eligible remaining Foot/leaf post-work.
18. Return to for_each_live_object, which advances the index without repair and reloads length.

If mission or locomotor work conceals the current object, the scheduler retains the native compaction consequence. If it appends an object, the later live length can include it.

#### Cache contract

Pass preparation may perform pure, read-only work but may not pre-collect the production mover population. A lazy cache must be rebuilt when its occupancy/passability generation is stale. A cache optimization is acceptable only if it yields exactly the same cell-entry/path decision as a fresh query over current authoritative state.

Formation-speed synchronization is gameplay mutation, not cache preparation. It must be researched and assigned a verified owner before the flip.

#### Side-effect contract

Anything a later object can observe commits before that later object:

- entity position/facing;
- occupancy removal/addition;
- bridge and cell-list state;
- path-index/head-to changes;
- PerCellProcess results;
- crush/scatter and Scenario RNG;
- lifecycle conceal/uninit;
- wall/passability changes;
- gate/factory-exit contacts;
- mission/NavCom arrival state;
- sound/event order when order is observable downstream.

Only diagnostics or positively proven global services may remain in a pass postlude.

#### State/hash contract

DriveLocomotionRuntime remains the canonical persisted normal-Drive owner. Duplicate persisted authorities are forbidden unless full-input byte equivalence and synchronization are proven.

All authoritative movement and mission fields continue to participate in state_hash. A new transient GroundMovePassState is neither serialized nor hashed. The authority flip increments the current snapshot version once and rebaselines only named, evidence-backed fixtures.

### Data Flow

#### Command to first normal Drive point

1. Command validation accepts the destination.
2. Navigation/path intent is written; no normal track exists yet.
3. The live scheduler reaches that Foot object.
4. Its real Mission_Dispatch observes the native mission/timer state and invokes the selected mission handler when due.
5. Foot locomotor Process reaches Drive.
6. Drive samples slope.
7. The no-active-track branch runs arrival/delay/NavCom gates.
8. Process_Movement obtains/validates path state, computes the target fraction, selects the first normal track, writes cursor zero, and installs head-to state.
9. When native control flow permits, Process_Drive_Track(normal) runs in the same Drive invocation.
10. If budget is not greater than 7, no point is consumed and owner coordinate/facing remain unchanged.

#### Active-track Drive invocation

1. Sample slope.
2. Run Process_Drive_Track(normal).
3. Update the speed fraction once.
4. Request fresh speed integer.
5. Add residual.
6. Consume current points while budget is strictly greater than 7.
7. Commit every point-body world effect immediately.
8. If a normal cell transition occurs and the owner remains eligible, resume the same loop with remaining budget.
9. If the track ends, Process_Movement may select the next track.
10. Process_Drive_Track(retry) may run in the same Drive Process; it repeats pre-budget speed-state work, then masks fresh integer speed and consumes residual only.

#### Track kernel point body

1. Confirm budget is greater than 7.
2. Subtract 7.
3. Read the current cursor's point.
4. Transform it in the verified coordinate/facing frame.
5. Produce point/state effects.
6. Commit any borrow-boundary world effect.
7. Increment the cursor after the point body.
8. Re-evaluate explicit exit/continuation control.
9. Continue while budget is greater than 7.
10. Store residual and apply the strict residual interpolation branch without a facing update.

#### Arrival

Arrival is not accumulated in a generic finished list.

- Empty queue: on the next no-active-track Drive Process at the destination cell, call the full Set_Destination(NULL,1) path, which reaches Stop_Moving/clear-navigation semantics.
- Non-empty queue: preserve the distinct Stop_Moving plus OnArrival/next-destination path.
- A skipped/ineligible object turn without a Drive Process does not clear NavCom; there is no separate admitted-every-third-subtick Drive clock.
- Walk/Hover/Ship completion remains locomotor-specific.

#### Later-object visibility

After the current object's locomotor returns, the next LogicVector entry sees all committed occupancy, lifecycle, RNG, navigation, wall/bridge, and position state. Scratch caches are refreshed before they can return a decision based on pre-effect state.

### Error Handling

Simulation behavior uses sourced branches rather than application-style error recovery.

- An absent live ID is tolerated as required by the scheduler contract.
- After every borrow-boundary effect, re-resolve the owner. Stop immediately if it died, concealed itself, lost the active locomotor, or took a verified early-exit branch.
- Invalid retail table indices are invariant failures. Do not invent a wrap, clamp, center-snap, or fallback track without binary evidence.
- An UNKNOWN native branch remains blocked; it is not silently mapped to stop, continue, retry, or snap.
- Debug assertions may prove membership, cache generation, single processing, cursor range, and no double authority. They must not mutate release behavior.
- No per-point allocation, panic-driven normal control flow, float simulation math, parallel commit, or nondeterministic collection traversal is introduced.

### Migration

#### Stage 1: mechanical extraction

Create the transient pass state, one-object orchestration boundary, and raw Drive kernel interfaces while the existing global production wrapper remains the only movement owner. Newly extracted one-object helpers are called by that wrapper in its existing order; the live object host may only observe them read-only or run them on cloned fixtures.

Acceptance:

- per-tick state_hash is bit-identical to the pre-extraction baseline;
- entity state, occupancy, events, and RNG cursors are identical;
- current focused movement tests remain identical;
- no new persisted/hashed field;
- no production double path.
- no production handled-ID skip set and no category-specific authority split.

#### Stage 2: readiness seams

Prepare the exact segmented Techno/Mission/Foot host plus per-object miner, Infantry, forced-track, tube, Hover, Ship, lifecycle/effect, and Drive owner paths without activating a second production authority. Exercise them in read-only shadows, focused pure tests, or cloned fixtures; do not add a persisted runtime feature flag.

Acceptance:

- every current Phase-1 population member maps to exactly one prepared per-object handler;
- active-special locomotors map to the later owner and are not consumed by ground;
- current postlude mutations are classified as per-object, verified global service, or BLOCKED;
- no category is partially flipped in production.

#### Stage 3: research closure

Resolve all blockers listed in Research Gates. Update the implementation contract and this design if evidence changes an interface premise.

#### Stage 4: atomic authority flip

In one behavior-bearing change:

- call all active ground locomotors from their live object slot;
- activate the corrected normal Drive owner;
- retire the standalone production tick_movement_with_grids call;
- retire gameplay-bearing global movement post-processing or move it to its verified owner, including deferred crush removal, generic arrivals/finalization, formation synchronization, Hover vertical work, gate/factory-exit contacts, and drive-over wall effects unless a native global owner is positively proven;
- process each ground object exactly once;
- increment the current SNAPSHOT_VERSION once;
- rebaseline only executable-evidence-backed fixtures;
- keep a test-only compatibility harness only if it cannot become a second production owner.

If the flip changes a fixture in an unexplained way, revert the flip and retain only the hash-neutral preparation. Do not stack compensating patches.

### Testing Strategy

#### Raw DriveTrack kernel

Required named tests:

- drive_process_movement_fresh_track_cursor_is_zero
- amcv_fresh_track_zero_budget_preserves_center_and_facing
- drive_track_consumes_current_point_before_increment
- drive_budget_equal_seven_does_not_consume
- drive_residual_interp_uses_gt_three_gate
- drive_cell_cross_continues_while_budget_gt_seven
- drive_track_completion_selects_next_same_process_without_second_fresh_budget

#### Drive owner

Required coverage:

- command writes destination/path but no first normal track;
- slope sample precedes movement;
- normal call updates speed once before budget;
- retry repeats pre-budget speed-state work but contributes no second fresh integer speed budget;
- every eligible consecutive object turn offers Drive processing; there is no scheduler-imposed pair of frozen 45 Hz subticks between Drive updates;
- empty-queue clear occurs on the next no-active-track Drive invocation;
- fresh normal, accepted-chain, forced-track, and tube initialization remain distinct;
- renderer consumes the corrected sim position/facing unchanged.

#### Per-object scheduler

Required coverage:

- direct live order, with no mover snapshot or stable-ID sort;
- mission dispatch precedes locomotor for ordinary Unit, Infantry, and miner fixtures;
- guard B occurs after RockingUpdate but before the remaining pre-mission work; guard E occurs only after passive acquire/bomb/SlaveManager/CaptureManager;
- Mission_Move coverage exercises Mission_Dispatch timer/return-delay/RNG semantics rather than substituting `derived_mission`;
- Foot's post-Techno alive check, concrete pre-Process gates, and immediate post-Process alive check each suppress later work at the native point;
- same-pass tail append can be visited;
- compacting removal can skip the shifted successor;
- Drive, Walk, Hover, Ship, forced-track, and active low-bridge-tube cases each run exactly once;
- active special locomotors run zero times under the ground owner;
- the standalone production ground call is absent after flip;
- a prior mover's occupancy commit is visible to the next mover;
- crush/uninit immediately conceals/unregisters while physical store removal stays in the tail pending-delete drain;
- Scenario RNG draw order follows live per-object effect order.

#### Retail executable oracle

Rust-vs-prior-Rust hashes and hand-computed goldens are not parity evidence. The production flip requires captured gamemd.exe fixtures for at least:

- AMCV open-ground turn from rest;
- MTNK straight movement through a cell transition with spendable residual;
- track completion followed by same-process next-track selection;
- empty-queue arrival;
- two ground movers contending for the same cell in known LogicVector order;
- one lifecycle-changing interaction such as crush or scatter.

For each native Drive invocation record and compare:

- owner coordinate and facing;
- active track and point cursor;
- current/target speed fraction;
- fresh speed integer and residual;
- NavCom/head-to/path state;
- occupancy and lifecycle effects;
- Scenario RNG cursor where the branch draws.

A pass without an executable oracle remains UNVERIFIED, never VERIFIED.

#### Determinism and persistence

- Two identical simulations produce identical per-tick state hashes and ordered events.
- A mid-track save/load preserves cursor, residual, speed fractions, destination, path state, and LogicVector order.
- Save/load immediately before and after a cell-transition effect produces the same next invocation.
- No float enters sim movement math.
- No parallel commit path or nondeterministic collection order is introduced.
- All authoritative movement state remains hashed.
- No snapshot version change occurs during hash-neutral preparation; the atomic flip owns one increment.

#### Performance

- One O(live objects) production scheduler pass.
- Lazy per-owner caches with explicit invalidation.
- No pre-collected mover vector in production.
- No per-point heap allocation or effect vector.
- No ECS, trait-object locomotor tree, COM layer, or raw-pointer architecture.
- Pure/read-only preparation may be optimized, but results commit in native order and consume no RNG.

## Research Gates

The architecture is approved. Static scheduling is resolved by [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md): one pass per reached Main_Tick, one locomotor Process opportunity per eligible Foot turn, no separate 15 Hz Drive movement gate, and a late frame-counter increment. Checkpoint A is closed by the corrected Mission_Move report plus [TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md); it authorizes only the inert/test-only plan at `docs/plans/2026-07-20-ordinary-drive-inert-host-harness-plan.md`. Production implementation certification remains blocked on gates 2–11:

1. **CLOSED — exact Techno/Mission/Foot host contract:** the segmented guard-B/guard-E placement, Mission_Dispatch-to-Mission_Move timer path, Foot post-Techno/pre-Process/post-Process gates, Scenario RNG ownership, Unit wrapper order, and Foot-return harness boundary are verified; [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) is corrected. This closes research readiness for the inert harness, not production host activation.
2. **FootClass::GetCurrentSpeed at 0x004DB1A0:** exhaust widths, signedness, rounding, house/veterancy/ability/type/terrain modifiers, and stock AMCV/MTNK fixtures.
3. **RawTrack metadata and accepted-chain start:** re-derive raw loads and object/ILocomotion base frames in 0x004B0AD0, 0x004B0F20, 0x004B2630, and 0x004B4B00.
4. **Per-object miner mission readiness:** prove how the current snapshot/process/writeback Harvest pipeline maps to one live object without losing radio/dock/credit/RNG order.
5. **Infantry mission and Walk readiness:** establish the authoritative mission decision plus full active Walk Process owner before moving Infantry out of the global phase.
6. **Hover readiness:** verify exact active Hover Process XY/vertical order, idle invocation, speed update, and tube precedence.
7. **Ship readiness:** verify the active Ship Process/track/tube order and all current Rust generic-path dependencies before inclusion in the atomic population.
8. **Formation-speed synchronization:** determine whether the current global mutation corresponds to an active native mechanism and where it runs.
9. **Low-bridge tube and forced-track precedence:** verify their exact per-object/leaf branch order without changing their special cursor/budget semantics.
10. **Lifecycle and post-effect ownership:** classify deferred crush removal, generic arrivals/finalization, occupancy/cell effects, wall/gate/factory contacts, and any remaining postlude mutation against Unit/Infantry PerCellProcess, UnInit/pending-delete, or a positively proven global service.
11. **Executable movement oracle:** capture the named native fixtures through position/facing/cursor/residual/NavCom and later-object-visible effects; realized wall cadence/jitter can be measured here without reopening the resolved static call mechanism.

These are research prerequisites, not optional polish. A write-plan or implementation must not silently choose values for them.

## Architectural Decisions

- **One scheduler owner:** reuse object_ai_stage and LogicVector; do not create a second movement scheduler.
- **Exact host spine:** do not flatten Techno guard B/E or replace Mission_Dispatch with `derived_mission`; preserve the segmented native call/guard positions and Foot gates.
- **Rust-native services, gamemd-native order:** use plain modules, owned scratch state, and explicit effects rather than vtables/COM/raw pointers.
- **Atomic population flip:** prepare incrementally but activate the complete current Phase-1 population together.
- **No production skip bridge:** ordinary Drive-first extraction is allowed only under the existing global owner or as shadow/cloned-fixture work; the live host activation and entire global-call retirement are one atomic change.
- **No snapshot mover authority:** live iteration and lazy cache lookup preserve same-pass changes.
- **Immediate effects:** borrow management may yield one effect but cannot semantically defer it past later objects.
- **Canonical Drive state:** keep DriveLocomotionRuntime as the persisted normal-Drive authority.
- **Owner-specific arrival:** no generic finished-entity cleanup for Drive/Walk/Hover/Ship.
- **No render compensation:** correct simulation state at its owner.
- **No false parity certification:** old Rust hashes are regression ratchets; only named retail-derived checks can certify parity.
- **Known temporary architecture debt:** active air/special locomotors remain outside this ground slice. Their per-object migration is a named later requirement, not evidence the full Foot scheduler is complete.

The new ground_pass.rs module follows the existing sim/movement decomposition pattern and keeps techno_ai.rs from absorbing algorithms. It is a justified new boundary because movement_tick.rs currently combines pass scratch state, per-object behavior, and post-effects in one oversized production owner.

## Alternatives Considered

### New general LogicPass coordinator

A new scheduler could own mission, movement, combat, and future object services. It can theoretically preserve the same ledger, but it would duplicate/replace the already-landed LogicVector/object_ai_stage owner, broaden the refactor into unrelated systems, and create a much larger validation surface. Rejected for this design.

### Queue movement jobs and execute globally afterward

The object pass could enqueue IDs in live order and a global executor could process them later. This can preserve a mover ordering list but cannot preserve mission-dispatch-then-locomotor within each object or immediate effects visible to later object AI. It is confirmed DRIFT and rejected.

### Move only Drive Units per object

Moving vehicles while Infantry, miners, Hover, Ship, tube, or forced-track paths remain in the global pass splits contention and RNG/lifecycle order. This leaves an active parity hole and is rejected unless the user explicitly accepts that drift. The approved design does not.

## Non-Goals

- Recreate C++ inheritance, COM, raw pointer vectors, or global singleton mutation.
- Claim full Walk, A*, wall-classification, smoothing, bridge, tube, crush, or pathfinding parity from this authority change.
- Claim literal native one-wall route or full AMCV/MTNK coordinate timeline without executable capture.
- Change accepted-chain or forced-track cursors by applying the fresh normal cursor rule globally.
- Add an AMCV-specific multiplier, cursor special case, render offset, or smoothing patch.
- Replace strict budget greater than 7 or interpolation residual greater than 3 with inclusive tests.
- Fold active Fly, Jumpjet, Teleport, Rocket, or Parachute movement into this ground slice without their own evidence and approved design extension.
- Implement dormant TS Tunnel/Mech/DropPod behavior as a substitute for active YR locomotion.
- Start implementation from this document before the named research gates are closed and a write-plan is explicitly requested.
