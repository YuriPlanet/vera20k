# Transport Door Timing / Radio 0x11 Deploy Tracker - Ghidra Research Report

Date: 2026-05-22
Target: `TRANSPORT_DOOR_TIMING_RADIO_0X11_DEPLOY_TRACKER`
Binary: `gamemd.exe`
Status: PARTIAL
Active in YR: Conditional

## 0. Investigation Contract

### Target Question

Starting from [RADIO_MSG_0X11_SENDERS_AND_MEANING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_MSG_0X11_SENDERS_AND_MEANING_GHIDRA_REPORT.md), trace the `UnitClass+0x350` deploy/door tracker around the `0x11` passenger-entry poll. Determine the timer duration, how the poll gates/restarts closure, and whether the path is live for stock YR transports.

### Non-goals

- Do not re-prove the full `0x11` sender/receiver meaning except as an anchor.
- Do not investigate refinery, factory, carryall, or unrelated radio messages.
- Do not implement Rust changes.
- Do not edit in-repo docs or INI files.

### Evidence Needed To Mark COMPLETE

- Prior `0x11` sender decompile plus assembly range.
- Tracker helper disassembly/decompile for `0x004A51D0`, `0x004A5240`, completion, and transition.
- Conversion constant and rounding behavior for `DeployTime`.
- INI/default evidence for stock transports with `Passengers=` and `DeployTime=`.
- UnitClass visual/render consumer evidence for the tracker, or a proved negative.

### Stop Conditions

- Stop once helper state transitions, duration formula, and `0x11` branch effect are established.
- Stop once stock-YR activity is classified from INI/defaults.
- If live Ghidra is unavailable for UnitClass render dataflow, mark visual-frame claims partial rather than guessing.

## 1. Executive Summary

The `0x11` poll does not board passengers by itself. It is a keep-open/status gate around the transport's `TechnoClass+0x350` deploy animation tracker. `UnitClass::AI` sends `0x11` only when `Passengers > 0`, the tracker is not idle, and current mission is not `0x10`; if the first radio contact does not answer `1`, the unit calls `0x004A5240` with `DeployTime`.

The timer duration is `trunc(DeployTime * 900.0)` sim ticks. The conversion is not rounded-to-nearest: `_ftol @ 0x007C5F00` loads FPU control word `0x0E7F`, which is round-toward-zero. Therefore stock `DeployTime=.022` is `trunc(19.8) = 19` ticks.

Active in YR: Conditional. The path is live for stock units with `Passengers > 0`; the nonzero timed effect is live for stock transport units that also set `DeployTime=.022` such as `[FV]`, `[LCRF]`, `[HTK]`, `[SAPC]`, `[YHVR]`, `[BUS]`, `[DDBX]`, and `[PROPA]`. Passenger-capable stock units without a `DeployTime` override, such as `[BFRT]`, `[SHAD]`, and `[HIND]`, inherit the TechnoType default `DeployTime=0`, so the same path has zero-duration timing unless another path gives them a nonzero type value.

This report is PARTIAL because no live Ghidra MCP was exposed in this subagent slot. Local binary disassembly was used for helper/timer assembly, but the exact UnitClass visual renderer for doors/ramps was not exhaustively dataflow-traced. Direct helper xrefs found no UnitClass progress-frame consumer comparable to building gate rendering; do not implement a visible ramp animation from this report alone.

## 2. Class Layout / Key Offsets

| Offset | Owner | Meaning | Evidence | Active in YR |
|---:|---|---|---|---|
| `+0x350` | Techno/Unit instance | Embedded deploy animation tracker | Existing decompiled layout [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md); helper assembly `0x004A50F0..0x004A5385` | Yes, generic Techno field |
| `+0x350` | tracker `+0x00` | `double` scaled duration = `DeployTime * 900.0` | `0x004A5254..0x004A5264`; constant bytes at `0x007E27F8` = `900.0` | Conditional |
| `+0x358` | tracker `+0x08` | start frame copied from global frame counter `0x00A8ED84` | `0x004A5269..0x004A5276`; constructor `0x004A50F2..0x004A50F8` | Yes |
| `+0x360` | tracker `+0x10` | duration ticks from `_ftol` | `0x004A527F`; `_ftol @ 0x007C5F00` | Yes |
| `+0x364` | tracker `+0x14` | total duration ticks, used as progress denominator | `0x004A5282`; `GetProgress @ 0x004A52F0` | Yes |
| `+0x368` | tracker `+0x18` | active/timer-running byte | state queries `0x004A5110..0x004A51E3` | Yes |
| `+0x369` | tracker `+0x19` | phase/direction byte | state queries `0x004A5110..0x004A51E3` | Yes |
| `UnitType+0x5E0` | Unit type | `Passengers=` | Prior parser evidence `0x00714B43..0x00714B50`, string `0x0081BBD4` | Yes |
| `TechnoType+0x3C8` | Type | `DeployTime=` double low dword | Prior parser evidence `0x00714B77..0x00714B99`, string `0x00843904` | Conditional |
| `TechnoType+0x3C8` | Type default | constructor default `0` | `TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md` table, row `0x3C8` | Yes |

## 3. Core Logic

### 3.1 Tracker State Queries

Assembly range: `0x004A5110..0x004A51E3`.

State query truth table from local disassembly:

| Helper | Returns true when | Relevant assembly |
|---|---|---|
| `0x004A5110` | `+0x18 != 0 && +0x19 != 0` | tests both bytes nonzero at `0x004A5110..0x004A5123` |
| `0x004A5130` | `+0x18 != 0 && +0x19 == 0` | `0x004A5130..0x004A5143` |
| `0x004A51B0` | `+0x18 == 0 && +0x19 == 1` | `0x004A51B0..0x004A51C2` |
| `0x004A51D0` | `+0x18 == 0 && +0x19 == 0` | `0x004A51D0..0x004A51E3` |

For the transport handoff, the important names are:

- `both clear` = idle/closed for the UnitClass passenger-door use.
- `phase-only` (`+0x18=0,+0x19=1`) = held deployed/open state.
- `active + phase 0` = returning/closing timer.
- `active + phase 1` = deploying/opening timer.

The exact bit-to-visible mapping is known for this UnitClass behavior only by state context, not by a verified UnitClass door-frame renderer.

### 3.2 Start/Restart Closing/Return Timer

Function: `0x004A5240`
Assembly range: `0x004A5240..0x004A5289`

Behavior:

1. If `+0x18 == 0` and `+0x19 == 0`, return without changing anything.
2. Otherwise load the double argument from the stack, multiply it by `900.0`, convert with `_ftol`, and set:
   - `+0x18 = 1`
   - `+0x19 = 0`
   - `+0x358 = current frame`
   - `+0x360 = trunc(duration * 900.0)`
   - `+0x364 = trunc(duration * 900.0)`

Important tiny detail: if the tracker is already active, this function still restarts the timer from the current frame. It does not ignore repeated calls.

### 3.3 Completion And State Transition

Functions:

- `0x004A5150` completion check, assembly `0x004A5150..0x004A51AB`
- `0x004A5360` state transition, assembly `0x004A5360..0x004A5385`
- caller in `TechnoClass::AI_Update`, assembly `0x006FA5BE..0x006FA5D1`

`TechnoClass::AI_Update` does:

```text
ecx = this + 0x350
if tracker_check_completion(ecx) != 0:
    tracker_transition(ecx)
```

Transition behavior:

- Active + phase `1` completes to `+0x18=0,+0x19=1`.
- Active + phase `0` completes to `+0x18=0,+0x19=0`.
- If not active, transition returns without change.

Zero duration completes immediately when `0x004A5150` sees total duration `0`, so missing `DeployTime` means a zero-tick/next-AI-update tracker transition, not the 19-tick stock `.022` duration.

### 3.4 Radio 0x11 Poll Effect

Prior verified sender: `UnitClass::AI @ 0x0073668F..0x007366E6`.

Relevant local disassembly:

```text
0073668F: mov  ecx, [esi+0x6c4]       ; type
00736695: mov  eax, [ecx+0x5e0]       ; Passengers
0073669D: jle  0x7366eb               ; skip if <= 0
0073669F: lea  edi, [esi+0x350]
007366A7: call 0x4a51d0               ; both-clear idle?
007366AE: jne  0x7366eb               ; skip if idle/closed
007366B4: call [vtable+0x184]         ; current mission
007366BA: cmp  eax, 0x10
007366BD: je   0x7366eb               ; skip if mission 0x10
007366C1: push 0x11
007366C5: call [vtable+0x274]         ; Transmit_Radio_ToFirst
007366CB: cmp  eax, 1
007366CE: je   0x7366eb               ; receiver says still entering: do not close/restart
007366D0: mov  ecx, [esi+0x6c4]
007366D6: mov  edx, [ecx+0x3cc]
007366DC: push edx
007366DD: mov  eax, [ecx+0x3c8]
007366E5: push eax
007366E6: call 0x4a5240               ; start/restart return/closing timer
```

Effect:

- While the tracker is idle/both-clear, the poll is not sent.
- While the tracker is open/deployed or animating, the transport polls the first radio contact.
- A `1` response (passenger current/queued `Mission_Enter`) suppresses `0x004A5240`, so a held-open tracker remains held open.
- Any non-`1` response starts or restarts the return/closing timer with `DeployTime`.
- Because `0x004A5240` restarts when already active, repeated non-`1` responses can refresh the timer. Exact end-to-end close completion depends on when the unit stops receiving/issuing the non-`1` poll, which was not runtime-verified in this slot.

## 4. INI Keys

| INI key | Default/source | Stock examples | Effect in this slice | Active in YR |
|---|---|---|---|---|
| `Passengers=` | absent -> `0`; parser store `UnitType+0x5E0` | `[FV]=1`, `[LCRF]=12`, `[HTK]=5`, `[SAPC]=12`, `[YHVR]=12`, `[BFRT]=5`, `[SHAD]=5`, `[HIND]=10` | gates whether `UnitClass::AI` can enter the `0x11` poll path | Yes |
| `DeployTime=` | TechnoType default `0` at `+0x3C8`; parser string `0x00843904` | most transport vehicles `.022`; `[BFRT]`, `[SHAD]`, `[HIND]` absent | duration = `trunc(DeployTime * 900.0)` ticks | Conditional |
| `PipScale=Passengers` | UI/pip display | many passenger units | not a gate for the `0x11` tracker branch | Yes, but not relevant to timing |

Stock `DeployTime=.022` examples in `rulesmd.ini`: `[FV]`, `[LCRF]`, `[HTK]`, `[SAPC]`, `[YHVR]`, `[BUS]`, `[DDBX]`, `[PROPA]`. These resolve to 19 ticks.

Stock `Passengers > 0` but no `DeployTime=` examples in `rulesmd.ini`: `[BFRT]`, `[SHAD]`, `[HIND]`, `[YAPOWR]`. For this tracker duration, absence means default `0`.

## 5. Integration Points

Verified/touched integration:

- `UnitClass::AI @ 0x0073668F..0x007366E6` sends `0x11` and calls `0x004A5240` on non-`1`.
- `FootClass::Receive_Radio @ 0x004D9219..0x004D9253` returns `1` only when current or queued mission is `7`.
- `TechnoClass::AI_Update @ 0x006FA5BE..0x006FA5D1` advances/completes the tracker.
- `UnitClass::Mission_Guard @ 0x00740A90..0x00740B03` also calls `0x004A5240` if the tracker is not idle before delegating to base guard logic.
- `UnitClass::Mission_Hunt_Override @ 0x00740B10..0x00740B50` does the same.
- `UnitClass` vtable target `0x00744180..0x007441AF` returns false while the tracker is in the `0x004A5110`, `0x004A5130`, or `0x004A51B0` states, true only when neither active nor held-deployed. The caller/semantic name of this vtable slot was not traced here.

Negative/touched integration:

- Direct call census found `0x004A52F0` progress consumers at building-render addresses only (`0x0043D584`, `0x0043DE51`, `0x0044E746`), not a UnitClass transport-render progress consumer. This is a useful negative clue, not an exhaustive proof that no indirect visual exists.

## 6. Current Rust Implementation Status

Rust passenger boarding is direct state mutation, not a radio/status-poll-controlled door tracker:

- `src/sim/passenger.rs` defines `PassengerRole::{Transport, Boarding, Inside}` and `BoardingPhase::{Approach, Entering}`.
- `tick_boarding` directly boards once Chebyshev distance is within `BOARD_DISTANCE`, pushes the passenger into cargo, and hides the passenger as `PassengerRole::Inside`.
- `tick_unloading` ejects one passenger per tick to the first free adjacent cell.
- `src/rules/object_type.rs` parses `Passengers` and `SizeLimit`, but no `DeployTime` object field was found in the Rust scan.
- `GameEntity.radio_contacts` exists, but passenger boarding does not use the generic radio contact vector.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Prior `0x11` sender/receiver meaning | verified | [RADIO_MSG_0X11_SENDERS_AND_MEANING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_MSG_0X11_SENDERS_AND_MEANING_GHIDRA_REPORT.md); assembly `0x0073668F..0x007366E6`, `0x004D9219..0x004D9253` | none for anchor |
| Tracker query helpers | verified | local disassembly `0x004A5110..0x004A51E3`; existing decompiled layout doc | none for byte truth table |
| `0x004A5240` timer start/restart | verified | local disassembly `0x004A5240..0x004A5289` | fresh Ghidra decompile would improve confidence wording |
| Completion/transition | verified | local disassembly `0x004A5150..0x004A51AB`, `0x004A5360..0x004A5385`, caller `0x006FA5BE..0x006FA5D1` | none for state transition |
| `DeployTime * 900` and truncation | verified | constant `0x007E27F8 = 900.0`; `_ftol @ 0x007C5F00`, FPU CW `0x00822D80 = 0x0E7F` | none |
| Stock INI activity | verified | `rulesmd.ini` scan; `TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md` default row | none for listed stock sections |
| UnitClass visual door/ramp frame consumer | touched-not-exhausted | direct call census: `0x004A52F0` direct consumers are building render paths | live Ghidra dataflow/xref pass or runtime capture |
| End-to-end close completion after repeated non-`1` polls | touched-not-exhausted | helper restart semantics verified, caller context touched | runtime/logging or deeper mission/contact lifecycle trace |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-1 - What is the target slice? -> UnitClass passenger-door/deploy tracker around already-verified radio 0x11, not full radio protocol.` (evidence: user scope; prior report)
- `[RESOLVED] OQ-2 - What does 0x004A51D0 test? -> Returns true only when tracker bytes +0x18 and +0x19 are both zero.` (evidence: `0x004A51D0..0x004A51E3`)
- `[RESOLVED] OQ-3 - What does 0x004A5240 write? -> If not both-clear, sets active=1, phase=0, start frame=current frame, duration=total=ftol(arg*900).` (evidence: `0x004A5240..0x004A5289`)
- `[RESOLVED] OQ-4 - Is .022 rounded to 20 ticks? -> No. `_ftol` uses FPU control word 0x0E7F, so `.022*900=19.8` truncates to 19.` (evidence: `0x007C5F00`, `0x00822D80`, `0x007E27F8`)
- `[RESOLVED] OQ-5 - Does a ROGER 0x11 response close the tracker? -> No. It skips the `0x004A5240` call.` (evidence: `0x007366CB..0x007366E6`)
- `[RESOLVED] OQ-6 - Does non-ROGER start or restart the timer? -> Yes. `UnitClass::AI` calls `0x004A5240`, and that helper restarts even when already active.` (evidence: `0x007366D0..0x007366E6`, `0x004A5246..0x004A5282`)
- `[RESOLVED] OQ-7 - Is the path stock-active? -> Conditional yes: gated by stock `Passengers>0`; nonzero duration requires nonzero `DeployTime` such as `.022`.` (evidence: `rulesmd.ini`, parser/default docs)
- `[RESOLVED] OQ-8 - Does Rust currently parse or model this timer? -> No parsed `DeployTime` object field or transport door tracker found; boarding is direct `PassengerRole`.` (evidence: `src/rules/object_type.rs`, `src/sim/passenger.rs`)
- `[DEFERRED] OQ-9 - Which exact UnitClass renderer draws transport door/ramp frames from this tracker?` (category: `requires-different-system-context`; reason: no live Ghidra MCP available; direct `0x004A52F0` call census did not find UnitClass render consumers; next-step-if-pursued: run Ghidra xrefs/dataflow from `UnitClass` draw/vtable entries and capture stock transport boarding frames)
- `[DEFERRED] OQ-10 - Under repeated non-1 polls, exactly when does closing complete in a live stock boarding sequence?` (category: `needs-runtime-debugger`; reason: helper restarts are verified but contact/mission lifecycle timing was not runtime-observed; next-step-if-pursued: instrument stock IFV/HTK boarding with frame log for tracker bytes and radio return)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `DeployTime` duration for this tracker is `trunc(DeployTime * 900.0)` ticks; stock `.022` is 19 ticks. | `0x004A5254..0x004A5264`, `0x007E27F8=900.0`, `_ftol @ 0x007C5F00` with CW `0x0E7F` | missing | `src/rules/object_type.rs`, passenger/transport sim state | Parse/store `DeployTime` and use a deterministic integer tick duration of 19 for `.022`. | `transport_deploy_time_dot022_truncates_to_19_ticks` | Do not use seconds, milliseconds, 60 FPS, or round-to-nearest. |
| A transport with `Passengers>0` and non-idle tracker polls first contact with `0x11`; ROGER suppresses return/close restart. | `UnitClass::AI @ 0x0073668F..0x007366E6`; `FootClass::Receive_Radio @ 0x004D9219..0x004D9253` | missing | `src/sim/passenger.rs`, possible `sim::radio`/contact surface | Keep door/deploy tracker held while the first contact is in current/queued enter-equivalent state. | `transport_entry_poll_keeps_door_tracker_open_while_passenger_entering` | Do not implement `0x11` as a movement/order command or cargo insertion. |
| Non-ROGER calls `0x004A5240`, which starts/restarts the return/close timer from the current frame. | `0x007366CB..0x007366E6`, `0x004A5240..0x004A5289` | missing | passenger boarding/unloading timing state | Start or refresh the closing/return timer only after the entering passenger no longer answers yes. | `transport_door_close_timer_restarts_after_enter_poll_stops_roger` | Avoid closing immediately on the same tick the passenger reaches adjacency if its enter state is still active. |
| Passenger boarding in Rust is direct cargo mutation; no door timer or `DeployTime` gate exists. | `src/sim/passenger.rs` scan | mismatch/missing | `src/sim/passenger.rs` | Add timing/visibility state before/around direct cargo mutation only after visual/runtime questions are settled. | `boarding_passenger_enters_only_after_transport_door_tracker_window` | Do not add speculative visible ramp frames without a verified render consumer. |

Concrete Rust test-name proposal:

- `transport_entry_poll_keeps_door_tracker_open_while_passenger_entering`

## 10. Negative Facts / Do Not Do

- Do not round `DeployTime=.022` to 20 ticks or treat it as about 1 tick; the verified tracker conversion gives 19 ticks.
- Do not make `PipScale=Passengers` or `Category=Transport` the sender gate. The binary gate is `Passengers > 0` plus non-idle tracker.
- Do not treat `0x11` as the boarding operation. It is a status poll; cargo insertion/lifecycle is elsewhere.
- Do not assume every stock passenger-capable unit has nonzero door timing. `[BFRT]`, `[SHAD]`, and `[HIND]` have `Passengers` but no `DeployTime` override in `rulesmd.ini`.
- Do not implement a UnitClass visual ramp/door animation solely from the generic tracker. This report did not prove a UnitClass frame renderer for it.

## 11. Remaining Uncertainty

- Exact UnitClass visual output is unresolved. Direct helper xrefs did not show a UnitClass `GetProgress` frame consumer, but a full Ghidra dataflow pass over UnitClass rendering was not possible in this slot.
- Exact close completion in a live sequence with repeated non-`1` polls should be runtime-logged. The helper restart behavior is verified, but the mission/contact lifecycle that stops repeated refreshes was not fully traced.
- `BFRT`, `SHAD`, and `HIND` stock behavior needs runtime confirmation: the path is `Passengers`-eligible, but their default `DeployTime=0` should make this tracker visually/timing-wise negligible unless another type override is in play.

## 12. Stale Docs / Follow-up Wording

Recommended replacement for unit docs that describe `DeployTime=.022` as about 1 tick or millisecond-scale:

> For the `TechnoClass+0x350` deploy/door tracker used by the transport `0x11` entry poll, `DeployTime` is converted as `trunc(DeployTime * 900.0)` sim ticks. Therefore stock `DeployTime=.022` is 19 ticks. Do not describe this specific tracker as a 1-tick or 22-ms delay.

Recommended caveat for docs saying `DeployTime` directly drives visible passenger load/unload animation:

> `DeployTime` definitely drives the `+0x350` tracker duration around passenger entry/door timing. The exact UnitClass door/ramp render consumer remains unverified; avoid claiming specific visible ramp frames until a UnitClass render dataflow or runtime capture confirms them.

## Sources

- Prior report: [docs/research/RADIO_MSG_0X11_SENDERS_AND_MEANING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_MSG_0X11_SENDERS_AND_MEANING_GHIDRA_REPORT.md)
- Existing layout report: [docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md)
- Existing type report: `docs/research/TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md`
- Local binary disassembly from `gamemd.exe`: `0x004A50F0..0x004A5385`, `0x006FA5BE..0x006FA5D1`, `0x0073668F..0x007366E6`, `0x004D9219..0x004D9253`, `0x00740A90..0x00740B50`, `0x00744180..0x007441AF`
- INI files checked: `ini/rulesmd.ini`, `ini/rules.ini`
- Rust files scanned: `src/sim/passenger.rs`, `src/rules/object_type.rs`, `src/sim/game_entity.rs`
