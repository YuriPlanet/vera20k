# Grizzly Factory Step Cadence - Ghidra Research Report

**Address(es):** `0x004C9B20` (`FactoryClass::AI`), `0x004C9EA0` (`FactoryClass::SetRate`), `0x00426630` (`CDTimerClass::GetTimeRemaining`)  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** Stock YR Grizzly/`MTNK` factory progress cadence after the already-verified production total `661` and factory rate `661 / 54 = 12`, for normal power, one Allied war factory, no extra factory/power/wall/headstart modifiers, and sufficient credits.  
**Non-Scope:** `BuildTimeMultiplier` formula re-verification, sidebar cameo visual progress, blocked war-factory exit, final vehicle placement/delivery command latency, queue cancellation, multiplayer/AI headstart, and low-power/multiple-factory recalculation.  
**Confidence:** High for factory timer initialization, expiry condition, progress-increment order, and factory-complete frame count. Medium for final player-visible spawned-unit timing because delivery/placement is intentionally non-scope here.  
**Active in YR:** Yes.

## 1. Overview

`FactoryClass::SetRate` converts the stock Grizzly total `661` into a per-step timer duration of `12` frames, writes the timer start to the current global frame, and leaves progress at `0` for a fresh build. `FactoryClass::AI` then advances `Production_Value` by `1` only on frames where `current_frame - start_frame >= duration`, resetting the timer to the current frame after each accepted step.

For a fresh stock `MTNK` with `Production_Value=0`, enough credits, and no conditional headstart, the factory reaches `Production_Value == 54` on the 54th accepted step: `54 * 12 = 648` frames after `SetRate` wrote the start frame. The 54th step also marks the factory suspended/complete and pays the remaining balance; final vehicle delivery/spawn is a later delivery-path concern.

## 2. Class Layout / Key Offsets

| Offset | Owner | Type | Meaning | Evidence | Active in YR |
|---:|---|---|---|---|---|
| `+0x24` | `FactoryClass` | `int32` | `Production_Value`, canonical progress counter, complete at `0x36` | `FactoryClass::AI @ 0x004C9B78..0x004C9B82`, `IsComplete @ 0x004CA130` | Yes |
| `+0x28` | `FactoryClass` | `bool` | `Production_HasChanged`, true only on accepted progress step; false on non-expired tick | `0x004C9B89`, `0x004C9BBA` | Yes |
| `+0x2C` | `FactoryClass` | `CDTimerClass.StartTime` | timer start frame | `SetRate @ 0x004C9F20..0x004C9F34`, `AI @ 0x004C9B8C..0x004C9B97` | Yes |
| `+0x34` | `FactoryClass` | `CDTimerClass.Duration/TimeLeft` | copied from clamped rate; `12` for stock MTNK | `SetRate @ 0x004C9F28/0x004C9F34`; `AI @ 0x004C9B71..0x004C9B76` | Yes |
| `+0x38` | `FactoryClass` | `int32` | same production timer duration/rate as field visible to factory logic | `SetRate @ 0x004C9F28`, `AI @ 0x004C9B71` | Yes |
| `+0x3C` | `FactoryClass` | `int32` | `Production_Step`; normally `1` | constructor/prior struct report; read at `0x004C9B78` | Yes |
| `+0x58` | `FactoryClass` | pointer | currently built object; non-null for active Grizzly build | `StartProduction @ 0x004C9DA4`, `AI @ 0x004C9B33` | Yes |
| `+0x60` | `FactoryClass` | `int32` | remaining unpaid balance | cost division/payment at `0x004C9BB0..0x004C9C34` | Yes |
| `+0x70` | `FactoryClass` | `bool` | suspended/complete/paused flag | `SetRate clears @ 0x004C9EE7`; `AI sets complete @ 0x004C9C0C` | Yes |

## 3. Core Logic

### Fresh production initialization

`FactoryClass::StartProduction @ 0x004C9C70` initializes a new active object but does not start the step timer:

```text
IsDifferent = true
IsSuspended = true
Production_Timer_StartTime = g_CurrentFrameCounter
Production_Timer_Duration = 0
Production_Timer_TimeLeft = 0
Production_Value = 0
Object = type.Create(...)
Balance = type.Cost(...)
```

Evidence: `0x004C9D6E..0x004C9DED`. Active in YR: Yes.

`HouseClass::Begin_Production @ 0x004FA350` then calls `FactoryClass::SetRate` after successful `StartProduction`. A conditional headstart path can later overwrite `Production_Value` in network/current-player cases, but that is explicitly outside the normal no-modifier stock acceptance scenario here. Evidence: `0x004FA350` decompile calls `FactoryClass__SetRate(this_00)` after start/resume; headstart write appears after the call. Active in YR: Yes/Conditional.

### Rate setup for stock MTNK

`FactoryClass::SetRate @ 0x004C9EA0`:

```text
if active object/special item exists, suspended, and not already complete:
    IsSuspended = false
    total = FactoryClass__GetBuildStepTime(Object)   ; settled input: 661
    rate = total / 0x36                              ; signed integer division
    if rate < 1: rate = 1
    else if rate > 0xff: rate = 0xff
    Production_Timer_Duration = rate
    Production_Timer_StartTime = g_CurrentFrameCounter
    Production_Timer_TimeLeft = rate
```

For stock Grizzly: `661 / 54 = 12`, so `Duration=12`, `StartTime=F`, `TimeLeft=12`. Evidence: divide/clamp/write sequence `0x004C9EEF..0x004C9F34`. Active in YR: Yes.

### Timer expiry condition

`CDTimerClass::GetTimeRemaining @ 0x00426630`:

```text
duration = timer[2]
if start != -1:
    elapsed = g_CurrentFrameCounter - start
    if elapsed < duration:
        return duration - elapsed
    return 0
return duration
```

Important edge: expiry is inclusive at `elapsed >= duration`, not `elapsed > duration`. With `StartTime=F` and `Duration=12`, calls on `F..F+11` return non-zero; a call on `F+12` returns `0`. Evidence: `CMP ECX,EAX; JGE zero` at `0x00426642..0x00426649`. Active in YR: Yes.

### AI step order

`FactoryClass::AI @ 0x004C9B20`:

```text
if IsSuspended: return
if no Object and no SpecialItem: return
if Object != null and Production_Value == 54: return
if SpecialItem != -1 and Production_Value == 54: return

if CDTimerClass::GetTimeRemaining(timer) != 0 or Production_Timer_Duration == 0:
    Production_HasChanged = false
    return

Production_Value += Production_Step      ; normally +1
Production_HasChanged = true
timer.StartTime = g_CurrentFrameCounter
timer.TimeLeft = Production_Timer_Duration
IsDifferent = true

costThisStep = if Object == null:
    0
else if 54 - Production_Value == 0:
    Balance
else:
    Balance / (54 - Production_Value)
costThisStep = min(costThisStep, Balance)

if available_credits < costThisStep:
    OnHold = true
    Production_Value -= 1
else:
    Spend_Money(costThisStep)
    OnHold = false
    Balance -= costThisStep

if Production_Value == 54:
    IsSuspended = true
    Production_Timer_Duration = 0
    timer.StartTime = g_CurrentFrameCounter
    timer.TimeLeft = 0
    Spend_Money(Balance)
    Balance = 0
```

Evidence: timer check `0x004C9B63..0x004C9B76`; increment/reset `0x004C9B78..0x004C9B9F`; credit rollback `0x004C9BD5..0x004C9BEC`; completion `0x004C9C06..0x004C9C34`. Active in YR: Yes.

### Stock Grizzly acceptance count

Let `F` be the frame where `SetRate` writes `StartTime=F` and `Duration=12`.

| Event | Frame | State after `FactoryClass::AI` |
|---|---:|---|
| SetRate initializes timer | `F` | `Production_Value=0`, `StartTime=F`, `Duration=12`, `IsSuspended=false` |
| Earliest possible same-frame AI check | `F` | no step; remaining `12` |
| First accepted step | `F + 12` | `Production_Value=1`, timer reset to `F+12` |
| Nth accepted step | `F + 12*N` | `Production_Value=N` |
| 53rd accepted step | `F + 636` | `Production_Value=53`, not complete |
| 54th accepted step | `F + 648` | `Production_Value=54`, `IsSuspended=true`, timer duration cleared |

Factory completion therefore occurs exactly `648` frames after the start frame in this scoped scenario, not `661` frames and not `661` rounded to 54 steps. At the usual `66 ms` queue-frame approximation used in current Rust, `648` frames is about `42,768 ms`; final delivery/spawn can add separate command/UI/exiting latency and is not claimed by this report.

## 4. INI Keys

| Key | Location | Stock value | Effect in this slice | Active in YR |
|---|---|---:|---|---|
| `[MTNK] Cost` | `rulesmd.ini:6621` | `700` | settled input to total `661` | Yes |
| `[MTNK] BuildTimeMultiplier` | `rulesmd.ini:6648` | `1.5` | settled input to total `661` | Yes |
| `[General] BuildSpeed` | `rulesmd.ini:41` | `.7` | settled input to total `661` | Yes |
| `[General] MultipleFactory` | `rulesmd.ini:368` | `0.8` | no effect in one-factory scenario; extra factories would alter total before `/54` | Conditional |
| low-power production keys | `rulesmd.ini:369` and related general rules | stock defaults | no effect at normal power | Conditional |

## 5. Integration Points

| Function | Role | Evidence | Active in YR |
|---|---|---|---|
| `FactoryClass::StartProduction @ 0x004C9C70` | creates active object, resets progress and timer to stopped | `0x004C9D6E..0x004C9DED` | Yes |
| `HouseClass::Begin_Production @ 0x004FA350` | calls `StartProduction` then `SetRate`; may conditionally apply network/current-player headstart | decompile `0x004FA350` | Yes/Conditional |
| `FactoryClass::SetRate @ 0x004C9EA0` | computes `rate = clamp(total / 54, 1, 255)` and starts timer | `0x004C9EEF..0x004C9F34` | Yes |
| `CDTimerClass::GetTimeRemaining @ 0x00426630` | defines inclusive expiry boundary `elapsed >= duration` | `0x0042663A..0x00426649` | Yes |
| `FactoryClass::AI @ 0x004C9B20` | per-frame production step, credit payment, completion state | `0x004C9B20..0x004C9C41` | Yes |
| `LogicClassPerTickUpdateLiveVector @ 0x0055AFB0` | ticks all factories via vtable `+0x5C` before houses | factory loop near end of decompile | Yes |
| `FactoryClass::IsComplete @ 0x004CA130` | completion predicate used by delivery/sidebar paths | decompile `0x004CA130` | Yes |

## 6. Current Rust Implementation Status

Current Rust does not model the RA2 `54` discrete factory steps for the active queue timer. It stores a per-item `total_base_frames` and `remaining_base_frames`, then advances continuous base frames from elapsed milliseconds and rate PPM:

- `src/sim/production/production_queue.rs:216` stores `build_time_base_frames` as `total_base_frames`.
- `src/sim/production/production_queue.rs:412` `tick_production` advances active front items.
- `src/sim/production/production_queue.rs:853` `advance_queue_item` converts `tick_ms` into progressed base frames and completes when progressed frames consume `remaining_base_frames`.
- `src/sim/production/production_tech.rs:302` computes stock MTNK base `661`.

Rust therefore naturally completes a stock MTNK after about `661` base frames at 1x progress, while gamemd's factory step cadence completes the factory at `floor(661 / 54) * 54 = 12 * 54 = 648` frames for this no-modifier scenario. This report does not judge whether current continuous timing is an intentional simplification; for parity, a future RA2-step model should preserve the `54`-step floor and timer-boundary behavior.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Stock total `661` input | verified by prior slot | [GRIZZLY_BUILDTIMEMULTIPLIER_CONSUMER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GRIZZLY_BUILDTIMEMULTIPLIER_CONSUMER_GHIDRA_REPORT.md) | none in this slot |
| `/54` and clamp to `[1,255]` | verified | `0x004C9EEF..0x004C9F34`, `0x004C9FB0` | none |
| Fresh production progress/timer initialization | verified | `0x004C9D6E..0x004C9DED` | none for no-headstart scenario |
| Timer expiry inclusive boundary | verified | `0x00426630` | none |
| AI increment/reset/payment/completion order | verified | `0x004C9B20..0x004C9C41` | none |
| Factory tick integration | verified | `0x0055AFB0` factory loop | exact wall-clock wall time depends on game-speed/frame scheduling outside scope |
| Sidebar cameo visual progress | deferred | non-scope | separate sidebar/cameo cadence trace |
| Vehicle delivery/spawn after complete | deferred | non-scope; prior delivery docs cover adjacent path | separate acceptance for blocked/unblocked war factory exit |
| AI/network headstart | deferred | conditional branch after `SetRate` in `HouseClass::Begin_Production` | separate AI/multiplayer production trace |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-1 - Is this a bounded exhaustive slice? -> Yes; only factory step cadence from settled total/rate to factory-complete state is claimed.` (evidence: user scope and report non-scope)
- `[RESOLVED] OQ-2 - What initializes a fresh factory build's progress and timer? -> `StartProduction` sets `Production_Value=0`, `IsSuspended=true`, and timer duration/time-left `0`.` (evidence: `0x004C9D6E..0x004C9D95`)
- `[RESOLVED] OQ-3 - What starts the timer? -> `SetRate` clears suspension, computes rate, then writes start=current frame and time-left=duration.` (evidence: `0x004C9EE7..0x004C9F34`)
- `[RESOLVED] OQ-4 - What is stock MTNK's rate? -> settled total `661` divided by `54` truncates to `12`, inside clamp.` (evidence: `0x004C9EF6..0x004C9F20`; prior Grizzly BTM report)
- `[RESOLVED] OQ-5 - Does a timer expire when elapsed equals duration or only after it? -> Equal is expired; `elapsed < duration` is the only non-expired branch.` (evidence: `0x00426642..0x00426649`)
- `[RESOLVED] OQ-6 - Does AI step before or after checking timer? -> After; non-zero remaining time early-outs and clears `Production_HasChanged`.` (evidence: `0x004C9B63..0x004C9BBA`)
- `[RESOLVED] OQ-7 - Does AI reset timer before or after increment? -> It increments `Production_Value`, sets changed, then resets timer start/time-left in the same accepted-step branch.` (evidence: `0x004C9B78..0x004C9B97`)
- `[RESOLVED] OQ-8 - What happens on insufficient credits? -> The step is rolled back by `DEC` after setting `OnHold=true`; net progress is zero for that expiry.` (evidence: `0x004C9BE1..0x004C9BEC`)
- `[RESOLVED] OQ-9 - What happens on the last step's cost division? -> If `54 - Production_Value == 0`, cost is remaining balance directly, avoiding division by zero.` (evidence: `0x004C9BA4..0x004C9BC5`)
- `[RESOLVED] OQ-10 - Does completion happen in the same AI pass as the 54th step? -> Yes; after the credit branch, `Production_Value == 54` sets suspended, clears timer duration, spends remaining balance, and zeroes balance.` (evidence: `0x004C9C06..0x004C9C34`)
- `[RESOLVED] OQ-11 - Does `FactoryClass::IsComplete` use `>=54`? -> No, it checks exactly `==0x36` with non-null object or special item.` (evidence: `0x004CA130`)
- `[RESOLVED] OQ-12 - What is exact factory-complete frame count from start? -> `54 * 12 = 648` frames after `SetRate`'s start frame for no-headstart stock MTNK.` (evidence: `0x004C9F20..0x004C9F34`, `0x00426630`, `0x004C9B78..0x004C9C0C`)
- `[RESOLVED] OQ-13 - Is final vehicle delivery included? -> No; factory-complete state is exact, but final spawn/delivery is handled by separate sidebar/Place_Production/exit logic.` (evidence: non-scope plus [STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md))
- `[DEFERRED] OQ-14 - How many frames until the player sees the produced Grizzly exit the war factory?` (category: out-of-scope; reason: this requires delivery command and war-factory exit path, not factory step cadence; next-step-if-pursued: combine this report with blocked/unblocked exit delivery docs)
- `[DEFERRED] OQ-15 - How does multiplayer/current-player headstart alter `Production_Value`?` (category: out-of-scope; reason: conditional branch after `SetRate`, excluded by no-modifier scenario; next-step-if-pursued: trace `HouseClass::Begin_Production` headstart arguments and game-mode gates)
- `[DEFERRED] OQ-16 - Does sidebar cameo visual progress hit its full frame on the same tick as factory completion?` (category: out-of-scope; reason: sidebar visual timer is separate from factory sim timer; next-step-if-pursued: trace `StripClass::AI` and cameo timer setters)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Stock MTNK factory step delay is `12` frames and factory-complete state occurs on the 54th expiry, `648` frames after `SetRate` start. | `0x004C9EA0`, `0x00426630`, `0x004C9B20`; prior total `661` | mismatch if parity requires RA2 step cadence; current Rust continuous countdown completes around `661` base frames | `src/sim/production/production_queue.rs`, `src/sim/production/production_queue_tests.rs` | Model active production as 54 fixed progress steps with clamped per-step delay, or add a parity layer that produces the same complete tick. | Fresh stock MTNK at normal power with one factory is not complete at frame `647` after start and is factory-complete at frame `648`. | Do not complete at `661` frames when emulating gamemd factory cadence; do not round `/54` upward. |
| Timer start frame is not an immediate progress step; first step occurs when `current_frame - start_frame >= duration`. | `SetRate @ 0x004C9F20..0x004C9F34`; `CDTimerClass @ 0x00426642..0x00426649`; `AI @ 0x004C9B63..0x004C9B76` | unchecked/no equivalent frame timer; Rust progresses from elapsed `tick_ms` | `production_queue.rs::tick_production`, any future factory timer state | Preserve exclusive start-frame/inclusive expiry behavior. | After starting stock MTNK at frame `F`, calls through `F+11` do not increment; frame `F+12` increments to progress step `1`. | Do not decrement before checking the start frame in a way that gives a same-frame or `F+11` first step. |
| The 54th accepted step sets complete/suspended and clears timer in the same `FactoryClass::AI` pass; delivery/spawn remains separate. | `0x004C9C06..0x004C9C34`, `IsComplete @ 0x004CA130`, delivery prior doc | Rust currently completes and attempts spawn in `tick_production` immediately | `production_queue.rs`, `production_spawn.rs`, future pending-delivery state | Separate "factory progress complete" from "vehicle successfully delivered" if pursuing higher parity. | At `F+648`, factory reports complete with object retained pending delivery; next queued vehicle does not start until delivery succeeds. | Do not pop/refund/start next queue item merely because progress reached complete if war-factory exit is blocked. |

### Stale Docs / Follow-up Docs

Replace any wording that says stock Grizzly visible production takes `661` frames with:

> Stock Grizzly/`MTNK` total build time before factory step division is `661`, but `FactoryClass::SetRate` converts that total to a step delay by integer-dividing by `54` and clamping. `661 / 54` truncates to `12`; a fresh no-headstart build starts with `Production_Value=0`, `StartTime=current_frame`, and `Duration=12`. `FactoryClass::AI` advances one step only when `current_frame - StartTime >= Duration`, resets the timer after each accepted step, and completes when `Production_Value == 54`. Therefore the factory reaches complete state `648` frames after timer start (`54 * 12`) for stock MTNK at normal power with one factory and enough credits. Final vehicle delivery/spawn timing is separate from this factory-step cadence.

## Negative Facts / Do Not Do

- Do not treat `661` as the factory-complete frame count when emulating gamemd's factory cadence; it is the pre-division total.
- Do not round `661 / 54` to `13`; binary truncates to `12` and clamps only after division.
- Do not advance production on the same frame `SetRate` starts the timer; `GetTimeRemaining` returns non-zero while elapsed is less than duration.
- Do not use `>= 54` for completion unless a future wrapper has already clamped progress; binary `IsComplete` tests `== 0x36`.
- Do not start the next queued vehicle at factory-progress completion if the produced vehicle has not been successfully delivered/exited.

## Remaining Uncertainty

None for the scoped factory timer start, progress-step boundary, and factory-complete frame count. Remaining out-of-scope uncertainty: exact final spawned/visible Grizzly exit tick after factory completion, and conditional multiplayer/current-player headstart behavior.

## Sources

- Ghidra decompile/disassembly: `0x004C9B20`, `0x004C9EA0`, `0x004C9FB0`, `0x00426630`, `0x004C9C70`, `0x004FA350`, `0x004CA130`, `0x004CA1A0`, `0x004CA5A0`, `0x0055AFB0`.
- Prior reports: [docs/research/GRIZZLY_BUILDTIMEMULTIPLIER_CONSUMER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GRIZZLY_BUILDTIMEMULTIPLIER_CONSUMER_GHIDRA_REPORT.md), [docs/research/FACTORY_CLASS_BUILD_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACTORY_CLASS_BUILD_SPEED_GHIDRA_REPORT.md), [docs/research/FACTORYCLASS_AND_CAMEOENTRY_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACTORYCLASS_AND_CAMEOENTRY_STRUCT_LAYOUT.md), [docs/research/STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/STRIP_AI_FACTORY_DELIVERY_GLOBALS_AND_QUEUE_RESTART_GHIDRA_REPORT.md).
- INI files: `ini/rulesmd.ini`, `ini/rules.ini`.
- Rust scan: `src/sim/production/production_tech.rs`, `src/sim/production/production_queue.rs`, `src/sim/production/production_queue_tests.rs`.
