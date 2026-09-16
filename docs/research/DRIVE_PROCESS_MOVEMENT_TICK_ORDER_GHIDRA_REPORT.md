# Drive Process / Movement / Track Tick Order - Ghidra Research Report

**2026-09-13 call-site correction:** original bytes place the active-track
`Process_Movement` call at `4B0647`, followed by retry argument `1` at `4B0665`
and the track retry at `4B0AAA`. The no-active call is `4B0A79`; this report
previously reversed those movement call sites. Direction `8` returns from
movement selection at `4B3298/4B3A4D`; the track retry's tube gate at
`4B1297/4B12AE` runs before the paid-point gate at `4B150D`.

**2026-09-13 callback/state correction:** the paid loop retains its raw array,
raw metadata and chain-direction descriptor across owner callbacks
(`4B1542/154C`, resume `4B158F/1596`; Ship `6A0C08/0C14`, resume `6A0C52`).
Accepted chaining explicitly replaces those caches before its callback
(`4B1C78..1CF9` / `6A12C2..133C`). Coordinate transformation instead reads
the current selector's flags and current head (`4B4780` / `6A3DB0`), and the
residual branch reselects the raw array from current selector/short fields
(`4B1F6D..4B22E2` / `6A15B0..6A1924`). The saved
[`locomotor_track_callback` oracle](../../tools/spatial_oracle/locomotor_track_callback.py)
compares 54 supplied-mutation cases against original instructions, including
original facing loads and complete transform helpers. It does not execute
gameplay callback bodies or prove that each supplied mutation occurs in retail.

Geometric list crossing is separate from `PerCellProcess(2)`: the two explicit
Drive calls are `4B1CFD` (accepted chain) and `4B220F` (paid terminal), with
Ship counterparts `6A1340/6A1852`. During accepted-chain `PerCellProcess`, the
head is full Null and valid is true; the saved candidate head is installed
only after the callback and owner lifecycle checks. Those checks are alive
`+90`, not-in-limbo `+81`, and not-falling `+8D`, not an off-map check. Native
continuation retains the invoked locomotor, without an active-slot equality
guard. Rust's `track_process` tests cover the bounded retained/call-local state
semantics; canonical production stepping and the synchronous world callback
host remain required. The old batch stepper is still active.

**Address(es):** `0x004B0500` (`DriveLocomotionClass::Process`), `0x004B2630` (`Process_Movement`), `0x004B0F20` (`Process_Drive_Track`)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** call/order relationship among the three DriveLocomotion functions for speed/timing-visible state: slope sampling, active-track processing, path/track selection, speed fraction update, residual consumption, arrival stop, and state clears.
**Non-Scope:** exact acceleration/deceleration formulas beyond ordering, full `Can_Enter_Cell` taxonomy, A* internals, tube payload internals, formulas for every terrain/slope speed constant.
**Confidence:** High for the call order and state ordering listed below; Medium for Rust delta severity because Rust has active work in this area and was scanned only at source level.
**Active in YR:** Yes. These functions are the active Drive locomotor tick path. Evidence: `DriveLocomotionClass::Process @ 0x004B0500` calls `Process_Drive_Track @ 0x004B0F20` at `0x004B0576` and `0x004B0AAA`, and calls `Process_Movement @ 0x004B2630` at `0x004B0647` and `0x004B0A79`; normal Drive-locomotor unit data such as `[AMCV] Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}` reaches this path in stock YR.

## Working Notes Required By Slot

Target question: What is the exact tick-order relationship among Drive `Process`, `Process_Movement`, and `Process_Drive_Track` for speed/timing-visible state?

Non-goals: Do not deep-dive speed formulas except to order their reads/writes; do not redo NavCom lifecycle or queued arrival research unless it contradicts this tick-order slice.

Evidence needed to mark COMPLETE: decompile plus address-level call evidence for both `Process_Drive_Track` call sites and both `Process_Movement` call sites from `Process`; decompile evidence for where speed target, current speed fraction, residual, track index, head-to, and arrival/stop calls are read or written.

Stop conditions: stop after every initial open question is resolved or explicitly deferred, and after one zero-add pass over the three functions adds no new in-scope order questions.

## 1. Overview

`DriveLocomotionClass::Process` samples slope first, then chooses one of two tick paths. If an active drive track exists, it processes the current track first with `Process_Drive_Track(0)`, then may run `Process_Movement`, then may immediately run `Process_Drive_Track(1)` in the same tick. If no active drive track exists, it runs arrival/delay/NavCom gates first, then `Process_Movement`, then `Process_Drive_Track(0)` only if movement/path selection succeeded without an early exit.

The second same-tick `Process_Drive_Track(1)` is not a duplicate full movement tick. `Process_Drive_Track` masks out fresh speed when its argument is nonzero and advances only with residual budget already stored at Drive `+0x4C`.

## 2. Class Layout / Key Offsets

| Offset | Owner | Type | Tick-order role | Active in YR / evidence |
|---|---|---|---|---|
| `+0x18` | Drive | dword | current slope index field in decompiler as `piVar2[6]` | Yes; sampled before movement in `0x004B0500` |
| `+0x1C` | Drive | dword | previous slope index field in decompiler as `piVar2[7]` | Yes; updated before movement in `0x004B0500` |
| `+0x20..+0x2C` | Drive | timer/frame fields | slope transition state, timer duration `3` | Yes; `CDTimerClass__Start(3)` before track/movement |
| `+0x30..+0x38` | Drive | coord triplet | destination coord | Yes; checked by `0x004B0500`, `0x004B2630`, `0x004B0F20` |
| `+0x40..+0x48` | Drive | coord triplet | head-to/intermediate coord | Yes; cleared by movement/track branches before/after arrival |
| `+0x4C` | Drive | int | residual drive-track budget | Yes; read/write in `0x004B0F20`, reset by early inactive-track return |
| `+0x50` | Drive | double | Drive-local target speed fraction | Yes; written by `Process_Movement`, consumed by `Process_Drive_Track` |
| `+0x58` | Drive | int | active track index, `-1` means none | Yes; gate in `Process` and `Process_Drive_Track`; written by `Process_Movement` |
| `+0x5C` | Drive | int | active track point index | Yes; consumed/reset by `Process_Drive_Track`; set to `0` by `Process_Movement` |
| `+0x5F` / `+0x63` | Drive | byte | head-to / track-valid flag in decompiler branches | Yes; active-track branch requires this or `track_index != -1`; cleared with head-to |
| `+0x60` | Drive | byte | reversed/short table selector | Yes; track table byte `+1` vs `+0` in `0x004B0F20` |
| `Foot+0x5A4` | Foot | pointer | NavCom target object | Yes; read by `Process` before `Process_Movement` re-aim path |
| `Foot+0x5E0` | Foot | int[24] | path queue/current movement direction | Yes; `Process_Movement` path source, `Process_Drive_Track` tube special-case gate |
| `Techno+0x578` | Techno | double | current speed fraction consumed by `GetCurrentSpeed` | Yes; `Process_Drive_Track` sets via vtable `+0x544` before budget call `+0x538` |

## 3. Core Logic

### 3.1 Top-level `Process @ 0x004B0500`

Order verified from decompile and call-site byte scan:

1. Calls owner vtable `+0x1BC` to get occupied/current cell and reads `CellClass+0x11C` slope index.
2. If slope index differs from Drive current slope field, writes previous slope, writes new slope, starts `CDTimerClass` with literal `3`, and stores transition frame/timer values.
3. If `track_index == -1` or head-to valid byte is clear, enters no-active-track path:
   - arrival/NavCom/mission/delay checks run before movement;
   - may call NavCom vtable `+0x4C` then Drive vtable `+0x44` to re-aim destination before movement;
   - calls `Process_Movement(..., 1, 0)` at `0x004B0A79`;
   - if the out byte is clear and owner is still alive, calls `Process_Drive_Track(0)` at `0x004B0AAA`.
4. Else, enters active-track path:
   - calls `Process_Drive_Track(0)` at `0x004B0576`;
   - if it returns nonzero, or owner is no longer alive, exits before `Process_Movement`;
   - if track was cleared but destination/path conditions still need work, may re-aim NavCom then calls `Process_Movement(..., 1, 0)` at `0x004B0647`;
   - if `Process_Movement` does not set the caller out byte and owner is still alive, calls `Process_Drive_Track(1)` at `0x004B0AAA`.
5. Post-movement ambient/side effects such as tiberium spill animation and final `Is_Moving` return happen after the movement/track chain.

Active in YR: Yes. Evidence: Ghidra decompile of `0x004B0500`; direct `CALL` sites from retail `gamemd.exe`: `0x004B0576 -> 0x004B0F20`, `0x004B0AAA -> 0x004B0F20`, `0x004B0647 -> 0x004B2630`, `0x004B0A79 -> 0x004B2630`; assembly context confirms pushes around these calls.

### 3.2 `Process_Drive_Track @ 0x004B0F20`

Order inside the track processor:

1. Early guard: if no active head-to/track and path state is not the tube sentinel, or deploy-while-moving is disallowed, clear residual budget at `+0x4C` and return `0`.
2. Read `Accelerates` from `TechnoType+0xDBD`.
3. If `Accelerates=false`, call owner vtable `+0x544` immediately with Drive `+0x50` target speed fraction.
4. If `Accelerates=true`, run ramp/brake branch first, then call owner vtable `+0x544`.
5. Only after current speed fraction is updated, call owner vtable `+0x538` (`GetCurrentSpeed`) to get integer speed units for this tick.
6. Compute budget as `(param_2 ? 0 : speed_units) + residual`.
7. If budget is greater than `7`, consume track points in a loop, subtracting `7` before each point body, updating position/facing/collision/arrival state as it goes.
8. Store leftover budget back to Drive `+0x4C`.
9. If leftover is positive and a track remains active, compute residual visual interpolation using leftover times `1/7`; this branch does not call `FacingClass__UpdateFacing`.

Active in YR: Yes. Evidence: decompile of `0x004B0F20`; assembly context `0x004B0F69..0x004B0F81` reads `TechnoType+0xDBD`; `0x004B1261..0x004B1274` shows `SetSpeedFraction` call followed by `GetCurrentSpeed`; prior verified track report records budget formula and `-7` loop.

### 3.3 `Process_Movement @ 0x004B2630`

Order relevant to this slot:

1. If no movement target and path queue first slot is `-1`, clear head-to, optionally call owner vtable `+0x484`, and return without setting a track.
2. If destination triplet is null or owner is locked/tethered, return before path/track selection.
3. If path queue is empty (`Foot+0x5E0 == -1`), movement-delay check runs before `Find_Path`; when delay is not expired, it returns without track work.
4. After path availability, pick next path direction from `Foot+0x5E0`, validate candidate cell, and handle block/retry cases. Several retry branches recursively call `Process_Movement`; byte-scan found direct recursive calls at `0x004B397F`, `0x004B41F7`, `0x004B4219`, `0x004B4480`, and `0x004B4552`.
5. Compute target speed fraction from land/slope/health context, then write Drive `+0x50` if `track_index < 0x40`; if track index is already >= `0x40`, call owner vtable `+0x544` instead.
6. Call owner vtable `+0x534` with the next cell before final track selection.
7. Select the track index from current direction and next direction, reset point index to `0`, clear stale head-to, then install the new head-to coord and flag if needed.

Active in YR: Yes. Evidence: decompile of `0x004B2630`; direct callers from `Process` at `0x004B0647` and `0x004B0A79`; recursive call byte-scan addresses above.

## 4. INI Keys

| Key / source | Default / stock example | Binary role in this slice | Active in YR |
|---|---|---|---|
| `Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}` | Stock Drive units such as `[AMCV]` | Selects Drive locomotor process slot | Yes; normal vehicle movement |
| `Speed=` | e.g. `[AMCV] Speed=4`, `[MTNK] Speed=7` | Indirect input to owner `GetCurrentSpeed` after speed fraction update | Yes; exact reader outside this slot |
| `ROT=` | e.g. stock vehicles carry ROT values | Affects facing timer, not direct budget formula in `Process_Drive_Track` | Yes; `Process_Drive_Track` calls facing update when points are consumed |
| `Accelerates=` | Constructor default true; `[MTNK] Accelerates=false` from prior report | `TechnoType+0xDBD`; gates ramp branch vs direct speed-fraction set before budget | Yes; evidence `0x004B0F69..0x004B0F81` |
| `SlowdownDistance=`, `AccelerationFactor=`, `DeaccelerationFactor=` | Type fields consumed by ramp branch | Read inside `Process_Drive_Track` before `GetCurrentSpeed` | Yes for `Accelerates=true`; formulas out of scope |
| terrain/slope speed fields | `g_SpeedType_LandType_Table`, Rules slope multipliers | `Process_Movement` computes Drive target fraction before track install | Yes; exact constants out of scope |

## 5. Integration Points

| Ordered point | Evidence | Consequence |
|---|---|---|
| Slope sample is first inside Drive `Process` | decompile `0x004B0500` before any movement calls | visual slope transition can start before the same tick's movement/track work |
| Active track is processed before new movement/path work | `0x004B0576 -> 0x004B0F20`, then later `0x004B0647 -> 0x004B2630` | an existing curve may consume movement and clear state before `Process_Movement` chooses the next path step |
| No-active-track path runs arrival/delay/NavCom gates before movement | decompile `0x004B0500`, call `0x004B0A79` after those branches | queued/arrival state is not a post-loop generic cleanup in gamemd |
| Same-tick second track call is possible after movement | `0x004B0AAA -> 0x004B0F20` after `Process_Movement` | newly selected track can start in the same Process call |
| Second track call uses residual only | `Process_Drive_Track` budget uses `(~-(param_2 != 0) & speed) + residual` | prevents double speed budget in same tick |
| Arrival stop can happen from `Process_Drive_Track` track-end branch before top-level no-track arrival gate | decompile `0x004B0F20` track-end branch; arrival report | do not model arrival solely as a generic end-of-`MovementTarget` finalizer |

## 6. Current Rust Implementation Status

Rust currently has useful scaffolding but does not match this exact order:

| Surface | Current shape | Delta |
|---|---|---|
| `src/sim/movement/movement_tick.rs:714` | one generic mover loop gathers movers, processes pending Drive arrivals before movers, then advances `MovementTarget` and optional `drive_track` | missing Drive `Process` owner sequence around each Drive entity |
| `src/sim/movement/movement_tick.rs:955` | computes terrain speed modifier before generic speed ramp and step | resembles `Process_Movement` target-fraction computation but is ordered inside generic movement, not Drive owner dispatch |
| `src/sim/movement/drive_locomotion.rs:37` | stores target fraction; `Accelerates=false` copies target to current; true ramp not implemented | target/current fraction not the authority for Drive track budget |
| `src/sim/movement/movement_tick.rs:993` | generic `MovementTarget.current_speed` acceleration/braking comment says it matches original `Process_Drive_Track` | misleading for Drive parity; gamemd updates speed fraction inside `Process_Drive_Track` before `GetCurrentSpeed` |
| `src/sim/movement/movement_step.rs:272` | `advance_drive_track` runs inside generic movement step with `effective_speed * dt` semantics | does not reproduce same-tick `Process_Movement -> Process_Drive_Track(1)` residual-only chain |
| `src/sim/movement/movement_tick.rs:1504` | finalizer defers/clears Drive arrival after generic movement loop | partial for recent NavCom work; still not the exact `Process` branch structure |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `Process @ 0x004B0500` slope-before-movement order | verified | decompile `0x004B0500` | none for ordering |
| Active-track top-level branch | verified | decompile `0x004B0500`; call `0x004B0576` | full mission side effects out of scope |
| No-active-track top-level branch | verified | decompile `0x004B0500`; call `0x004B0A79` | full mission-id taxonomy out of scope |
| Same-tick second `Process_Drive_Track` call | verified | call `0x004B0AAA`; decompile control flow | none |
| `Process_Movement` call sites from `Process` | verified | byte scan and assembly context `0x004B0647`, `0x004B0A79` | exact argument semantic names remain decompiler-noisy but order is proven |
| `Process_Drive_Track` speed update before budget | verified | decompile; assembly `0x004B1261..0x004B1274` | formulas in ramp branch deferred |
| `Process_Drive_Track` residual-only retry semantics | verified | decompile budget expression; caller param order | none |
| `Process_Movement` path/block recursion | touched-not-exhausted | decompile; direct recursive call addresses | full block-code behavior belongs to path/collision reports |
| Rust exact Drive owner-loop parity | touched-not-exhausted | source scan listed above | future implementation |

## 8. Open Questions - Final State

- `[RESOLVED] OQ1 - Does slope sampling occur before path/track movement? -> Yes, `Process` samples occupied cell slope and starts a 3-frame timer before any movement call.` (evidence: `0x004B0500` decompile)
- `[RESOLVED] OQ2 - When active track exists, what runs first? -> `Process_Drive_Track(0)` runs before `Process_Movement`.` (evidence: `0x004B0576 -> 0x004B0F20`; decompile branch)
- `[RESOLVED] OQ3 - When no active track exists, what runs first? -> arrival/delay/NavCom gates run before `Process_Movement`; track processing comes only after movement succeeds.` (evidence: `0x004B0500`, call `0x004B0A79`)
- `[RESOLVED] OQ4 - Is there a same-tick `Process_Movement -> Process_Drive_Track` chain? -> Yes, active-track `0x004B0647` is followed by retry `0x004B0AAA`; no-active `0x004B0A79` is followed by a fresh call there.` (evidence: original call and argument bytes)
- `[RESOLVED] OQ5 - Does the second track call add fresh speed again? -> No, nonzero `param_2` masks speed to zero and uses only residual.` (evidence: `0x004B0F20` budget expression)
- `[RESOLVED] OQ6 - Is speed fraction updated before budget consumption? -> Yes, `SetSpeedFraction`/ramp branch precedes `GetCurrentSpeed`.` (evidence: `0x004B1261..0x004B1274`)
- `[RESOLVED] OQ7 - Does `Process_Movement` write Drive target speed fraction before selecting the next track? -> Yes, it writes `Drive+0x50` or calls owner `+0x544` before track-index assignment and point-index reset.` (evidence: `0x004B2630` decompile)
- `[RESOLVED] OQ8 - Does residual interpolation update facing? -> No, facing update is in consumed-point body, not the residual branch.` (evidence: `0x004B1AC1`; prior track report)
- `[RESOLVED] OQ9 - Can arrival stop occur before the top-level no-track arrival gate? -> Yes, `Process_Drive_Track` track-end branch can clear track/head-to and call stop/on-arrival logic.` (evidence: `0x004B0F20`; arrival report)
- `[RESOLVED] OQ10 - Are recursive `Process_Movement` retry calls present? -> Yes, five direct calls were found inside `0x004B2630..0x004B4800`.` (evidence: byte scan addresses `0x004B397F`, `0x004B41F7`, `0x004B4219`, `0x004B4480`, `0x004B4552`)
- `[RESOLVED] OQ11 - Is this active in standard YR, not TS legacy? -> Yes for Drive-locomotor units; no TS-only gate controls these top-level calls.` (evidence: vtable/process call chain and stock Drive locomotor INI)
- `[DEFERRED] OQ12 - Exact ramp/brake numeric formulas and constants.` (category: out-of-scope; reason: slot only orders speed writes and budget reads; next-step-if-pursued: dedicated Drive speed fraction formula contract)
- `[DEFERRED] OQ13 - Full `Can_Enter_Cell` code behavior inside `Process_Movement` and `Process_Drive_Track`.` (category: out-of-scope; reason: collision/path legality reports own this; next-step-if-pursued: consolidate runtime `Can_Enter_Cell` Drive callsite contract)
- `[DEFERRED] OQ14 - Exact tube movement payload and state copy semantics.` (category: out-of-scope; reason: tick-order slice only proves tube sentinel gates exist; next-step-if-pursued: low-bridge tube Drive payload slice)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Drive `Process` samples slope before movement/track work. | `0x004B0500` decompile | partial/missing for Drive-owned order | `src/sim/movement/movement_tick.rs`, slope/rocking integration | Slope transition state must be sampled at the Drive process entry point before any track/path advancement for that entity. | Moving vehicle enters a ramp cell: slope timer/state changes before same-tick DriveTrack advancement is observed. Proposed test: `drive_process_samples_slope_before_track_budget`. | Do not update slope only after generic movement finalizes cell crossing. |
| Active-track path runs `Process_Drive_Track(0)`, then possibly `Process_Movement`, then `Process_Drive_Track(1)`. | calls `0x004B0576`, `0x004B0647`, `0x004B0AAA`; retry argument at `0x004B0665` | missing; Rust advances track inside generic movement and finalizes later | `src/sim/movement/movement_tick.rs`, `src/sim/movement/movement_step.rs`, `src/sim/movement/drive_track.rs` | Drive tick must allow same-tick continuation from completed/current track to movement/path selection and a residual-only second track call. | A Drive unit whose current track completes and has a next path direction starts the next track in the same tick without receiving a second fresh speed budget. Proposed test: `drive_track_completion_chains_same_tick_with_retry_budget`. | Do not wait one full Rust tick after track completion; do not call the second track with a full speed budget. |
| `Process_Drive_Track` updates current speed fraction before calling `GetCurrentSpeed`, then consumes `speed + residual` or `residual` for retry. | `0x004B0F69..0x004B1274`; budget expression in `0x004B0F20` | missing/partial; Rust stores Drive fraction but movement still uses `MovementTarget.current_speed * cell_speed_mod` | `src/sim/movement/drive_locomotion.rs`, `src/sim/movement/movement_tick.rs`, `src/sim/movement/drive_track.rs` | Drive runtime speed fraction must be the authority before integer speed budget is computed; residual must persist in Drive runtime and retry call must mask fresh speed. | `Accelerates=false` MTNK uses target fraction on first DriveTrack budget, while same-tick retry consumes only saved residual. Proposed test: `drive_speed_fraction_before_budget_and_retry_masks_speed`. | Do not model `Accelerates=false` by changing generic acceleration factors; do not leave residual only inside `DriveTrackState` if retry semantics need Drive `+0x4C`. |
| Arrival/stop can be reached inside `Process_Drive_Track` and top-level no-track gates; empty/non-empty queue split is not a generic after-loop-only cleanup. | `0x004B0500`, `0x004B0F20`, arrival queue report | partial; recent Rust has delayed NavCom clear but still generic finalizer shape | `src/sim/movement/navcom.rs`, `src/sim/movement/movement_tick.rs` | Arrival clear/queue dispatch must be embedded in Drive process order so the next path/track selection sees gamemd state. | Empty queue arrival clears via `Set_Destination(NULL)` path, queued cell arrival starts a next destination without an extra unrelated movement tick. Proposed test: `drive_arrival_queue_processed_in_process_order`. | Do not infer arrival solely from `MovementTarget` exhaustion. |

## 10. Negative Facts / Do Not Do

- Do not summarize Drive order as simply "`Process` calls `Process_Drive_Track` then `Process_Movement`." Active in YR: Yes. Evidence: no-active-track path calls `Process_Movement` at `0x004B0A79` before the later `Process_Drive_Track` call at `0x004B0AAA`.
- Do not give the same-tick second `Process_Drive_Track` a full speed budget. Active in YR: Yes. Evidence: nonzero `param_2` masks fresh speed to zero in `0x004B0F20`.
- Do not compute Drive track budget before speed fraction update. Active in YR: Yes. Evidence: `SetSpeedFraction` call at `0x004B1269` precedes `GetCurrentSpeed` call at `0x004B1274`.
- Do not put residual interpolation in the same path as facing update. Active in YR: Yes. Evidence: facing update call `0x004B1AC1` is in consumed-point body; residual branch follows stored leftover and lacks that call.
- Do not make Drive arrival a generic finalizer independent of the Drive owner process order. Active in YR: Yes. Evidence: `0x004B0500` and `0x004B0F20` contain arrival/stop/clear branches before post-process side effects.

## 11. Stale Docs / Follow-up Docs

Potential stale or incomplete wording:

- `docs/research/GRIZZLY_ACCELERATES_FALSE_SEMANTICS_GHIDRA_REPORT.md:125` says "`Process @ 0x004B0500` ... calls `Process_Drive_Track` then `Process_Movement`." Replacement wording: "`Process @ 0x004B0500` has two active paths. With an active track/head-to it calls `Process_Drive_Track(0)` first, may call `Process_Movement`, then may call `Process_Drive_Track(1)` in the same tick. With no active track it runs arrival/delay/NavCom gates, calls `Process_Movement` first, and only then may call `Process_Drive_Track(0)`."
- `docs/research/miner/DRIVELOCOMOTION_PROCESS_DRIVE_TRACK_CHRONO_MINER_004B0F20_GHIDRA_REPORT.md:47` is correct but should be expanded when reused outside the track-budget slice: "`param_2=1` is the same-tick retry/chained call from `Process` after `Process_Movement`; it consumes residual only and exists in both active-track continuation and no-active-track movement-start paths."
- Source comment `src/sim/movement/movement_tick.rs:993` says generic speed ramping matches original `Process_Drive_Track` speed computation. Replacement wording for a future Rust patch: "Generic movement speed ramp; DriveLocomotion parity requires Drive-owned speed-fraction and residual-budget handling."

## Sources

- Ghidra read-only decompile: `DriveLocomotionClass::Process @ 0x004B0500`
- Ghidra read-only decompile: `DriveLocomotionClass::Process_Drive_Track @ 0x004B0F20`
- Ghidra read-only decompile: `DriveLocomotionClass::Process_Movement @ 0x004B2630`
- Ghidra assembly context: `0x004B0576`, `0x004B0647`, `0x004B0A79`, `0x004B0AAA`, `0x004B0F69..0x004B1274`, `0x004B1AC1`
- Retail `gamemd.exe` byte-level direct-call scan: `0x004B0576 -> 0x004B0F20`, `0x004B0AAA -> 0x004B0F20`, `0x004B0647 -> 0x004B2630`, `0x004B0A79 -> 0x004B2630`, recursive `Process_Movement` calls listed above
- Prior docs: [docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_ARRIVAL_QUEUE_NULL_DESTINATION_GHIDRA_REPORT.md), [docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVELOCOMOTION_HEAD_TO_COORD_CLEAR_NAVIGATION_STATE_GHIDRA_REPORT.md), [docs/research/miner/DRIVELOCOMOTION_PROCESS_DRIVE_TRACK_CHRONO_MINER_004B0F20_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DRIVELOCOMOTION_PROCESS_DRIVE_TRACK_CHRONO_MINER_004B0F20_GHIDRA_REPORT.md), [docs/research/GRIZZLY_ACCELERATES_FALSE_SEMANTICS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GRIZZLY_ACCELERATES_FALSE_SEMANTICS_GHIDRA_REPORT.md)
- Rust source scan: `src/sim/movement/movement_tick.rs`, `src/sim/movement/movement_step.rs`, `src/sim/movement/drive_locomotion.rs`, `src/sim/movement/navcom.rs`


### Foot-owned replay lifetime (2026-09-13)

`tools/spatial_oracle/foot_path_queue.py` executes original fresh/chain/tube
queue-write blocks and complete FootStop4DF0D0 / Drive END4AF930 helpers.
Its 28 supplied-state witnesses cover short and full24 queues, empty preservation,
negative coordinate division and signed16 reference wrapping. Fresh Drive4B45CB /
4B45F6 and Ship6A3BF7 /6A3C22 consume two/one directions and write owner+558;
chain4B1DF7 /6A143A and tube4B1362 consume one without changing that reference.
FootStop clears only owner+5A0/+5A4; END transfers the piggyback pointer without
writing the owner queue. These are bounded memory-operation proofs, not callback
admission, constructor, release, or whole-Process proofs. Rust compares them in
`locomotor_owner_tests::foot_queue_operations_match_original_memory_witnesses`.

Rust now retains this replay in `NavigationState.path_replay`, outside the
retired Drive/Ship payload. Failed miner-command rollback separately captures it;
current snapshot version150 records the changed field layout. Explicit Stop and
MCV unload exhaustion remain separate from FootStop/instance retirement.

Still OPEN for the same bridge TrackProcess host: native Stop4AFE00 /69F510
clamp target fraction and clear destination without resetting applied speed.
The rest reset belongs to Drive4B0828..4B0880 /Ship69FEF0..FF47 and requires full
Null destination/head, live empty Foot queue and positive applied fraction.
Same-cell/coordinate/queued arrival, active ProcessTrack nonzero, and the
ProcessMovement early-return byte can skip that tail. The current Rust NavCom
adapter still resets applied speed early; adding an unconditional tail or merely
an empty-queue guard there would not fix native call order. Exact tail admissions
and retained invocation ownership remain required before bridge mechanism closure.
