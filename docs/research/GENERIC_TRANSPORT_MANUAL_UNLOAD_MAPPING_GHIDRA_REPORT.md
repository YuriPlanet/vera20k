# Generic Transport Manual Unload Mapping - Ghidra Report

Date: 2026-05-23
Target: GENERIC_TRANSPORT_MANUAL_UNLOAD_MAPPING
Mode: `/re-investigate` exhaustive-slice, swarm slot 2
Primary binary: `gamemd.exe`
Primary Rust surface: `src/sim/passenger.rs::tick_unloading`

## 0. Working Notes

Target question: Identify the `gamemd.exe` function/path corresponding to Rust `src/sim/passenger.rs::tick_unloading` manual one-pass unload; verify generic transport unload placement order, destination choice, RNG contract, and whether it shares Infantry Scatter or a separate raw-neighbor path.

Non-goals: Do not investigate CanBeOccupied garrison sell/destruction ejection except as a negative comparison; do not expand into refinery/radio/carryall/paradrop behavior except where directly needed to map generic passenger unload.

Evidence needed to mark COMPLETE: decompile plus assembly/caller evidence for the generic vehicle passenger unload body; parser/INI/default evidence for YR liveness; current Rust comparison; at least one implementation handoff item.

Stop conditions: Ghidra is read-only; do not create or rename functions/labels/comments; if function boundaries are missing, inspect bytes/reachable callers read-only and record uncertainty; write only this report plus the shared swarm claims file.

## 1. Executive Result

The generic vehicle transport "manual one-pass unload" behavior maps to `UnitClass::Mission_Deploy_Building` at `0x0073D630`, specifically the branch where the unit type has `Passengers > 0` and mission state reaches state 3.

It does not map to `UnitClass::Mission_Unload` at `0x00740EF0`. That function searches docking bays using `[General] RepairBay=` (`RulesClass+0x850`) and is a repair/dock mission wrapper, not the passenger disgorge body.

The generic passenger unload path does not queue Infantry Scatter mission `0x0F` and does not use a raw random `% 8` neighbor destination. It pops one passenger from the cargo linked-list head, derives an 8-direction search start from `RateTimer::Current`, scans directions deterministically, places the passenger at/near a validated cell, queues mission `2` to the selected cell, then returns a mission delay with `RandomRanged(0,2)` jitter.

Active in YR: Yes, for standard YR vehicle transports with `Passengers > 0` when mission `0x10`/deploy-building unload is assigned. Evidence: `UnitTypeClass+0x5E0` is parsed from `Passengers` at `0x00714B35..0x00714B50`, stock YR INI contains `Passengers=` on transport types, and `UnitClass::Mission_Deploy_Building` gates directly on `UnitTypeClass+0x5E0 >= 1`.

## 2. Function Map

### 2.1 Positive Mapping: `UnitClass::Mission_Deploy_Building @ 0x0073D630`

Material finding: the top branch checks whether the unit is in the passenger-unload path:

- Decompile evidence: `if (param_1[0xb9] == 0)` then `if (*(int *)(UnitType + 0x5e0) < 1) ... else switch(param_1[0x2f])`.
- Offset evidence: `param_1[0x2f]` is byte offset `+0xBC`, the mission state byte used by this mission.
- Parser evidence: assembly `0x00714B35..0x00714B50` pushes the string pointer for `Passengers`, calls the INI reader, and writes the result to `[EBP+0x5E0]`.
- Active in YR: Yes. Stock YR transport unit types use `Passengers=` (`FV=1`, `BFRT=5`, `LCRF=12`, `SAPC=12`, `YHVR=12`, among others), and this branch is keyed by that parsed field.

### 2.2 Negative Mapping: `UnitClass::Mission_Unload @ 0x00740EF0`

Material finding: `0x00740EF0` is not the generic manual passenger ejection body.

- Decompile evidence: the function calls the unit vtable at `+0x528` with `g_RulesClass_Instance + 0x850, 0, 0`, clears byte `+0x6D2`, then queues Move/Enter on success.
- Assembly evidence: `0x00740EF3..0x00740F08` pushes `Rules+0x850, 0, 0` and calls `[vtable+0x528]`.
- Prior verified reader evidence: `RulesClass+0x850` is `[General] RepairBay=`, and vtable `+0x528` resolves to `FootClass::Find_Docking_Bay @ 0x004DF040`.
- Active in YR: Conditional. It is active as a repair/dock mission path, but not the generic transport passenger unload body being matched to Rust `tick_unloading`.

## 3. Generic Passenger Unload Body

### 3.1 Cargo Order

State 3 pops at most one passenger.

- Assembly evidence: `0x0073D8B7..0x0073D8CD` reads cargo count from `[ESI+0x114]`, reads skip/lower-bound index from `[ESI+0x6E4]`, compares them, and calls `0x004DE710` only when `cargo_count > skip_index`.
- Helper evidence: `FUN_004DE710 @ 0x004DE710` loads `CargoClass` at `[unit+0x114]` and calls `FUN_00473430`.
- Pop primitive evidence: `FUN_00473430 @ 0x00473430` reads the cargo head from `[CargoClass+4]`, writes the next pointer from old head `+0x30` back into the head slot, clears old head `+0x30`, and decrements count.
- Active in YR: Yes. This is inside the `Passengers > 0` branch of `UnitClass::Mission_Deploy_Building`.

Inference: the unload primitive is head-pop. Exact player-facing FIFO/LIFO depends on how boarding inserted each passenger into the cargo linked list. `CargoClass::AddPassenger @ 0x004733A0` inserts through the same list and includes a special-case splice behind a leading passenger flagged by bit `0x4`, so Rust should not assume sorted entity order as a parity rule.

### 3.2 Placement Order and Destination

The placement search is deterministic per call except for timer-derived start direction; it is not raw scenario RNG.

- Start direction evidence: assembly `0x0073D8DC..0x0073D90D` calls `RateTimer::Current @ 0x004C93D0`, reads the returned word, adds `0x7FFF`, shifts by 12, adds 1, then shifts again to derive the base direction.
- Direction scan evidence: assembly `0x0073D917..0x0073D925` computes `(base_direction + local_68) & 7`; loop increment/limit at `0x0073D9FA..0x0073DA02` covers eight directions.
- Neighbor table evidence: assembly `0x0073D934..0x0073D968` indexes `g_DirectionOffsets @ 0x0089F688` to compute candidate cells from the transport cell.
- Validation evidence: assembly `0x0073D981..0x0073D9C2` calls passenger vtable `+0x1AC` twice against candidate cell/height data before accepting a direction.
- Failure evidence: assembly `0x0073DC71..0x0073DC78` re-adds the passenger to cargo through `CargoClass::AddPassenger @ 0x004733A0` if no direction validates.
- Active in YR: Yes. This is the live state-3 passenger placement loop for `Passengers > 0` unit types.

Destination after a valid direction:

- Infantry RTTI path: if passenger RTTI is `0x0F`, code at `0x0073DA66` calls `FUN_004ACA10` for infantry in-cell/subcell selection.
- Non-infantry path: code around `0x0073DADD` calls `FootClass::Find_Nearby_Passable_Cell @ 0x0056DC20` from the candidate seed, then maps the chosen cell to center coordinates.
- Placement evidence: assembly `0x0073DA29..0x0073DB6A` builds center coordinates (`cell_x * 0x100 + 0x80`, `cell_y * 0x100 + 0x80`, z `0`), derives facing from the accepted direction, and calls passenger vtable `+0xD8` to unlimbo/place.
- Active in YR: Yes. This is the success path from the generic passenger unload state.

### 3.3 Post-Placement Order and Mission

Successful placement queues a Move mission, not Scatter.

- Clear/open-topped evidence: if `UnitType+0x5E4` is nonzero, assembly `0x0073DB85..0x0073DB98` calls `TechnoClass::ClearInOpenTransport @ 0x007104A0` on the passenger.
- Back-reference evidence: code at `0x0073DBC9` clears passenger field `+0x11C`.
- Mission evidence: assembly `0x0073DBD1..0x0073DBDB` pushes mission `2` and calls passenger vtable `+0x1E8`.
- Destination handoff evidence: assembly `0x0073DBE1..0x0073DC06` calls passenger vtable `+0x480` with the chosen cell and flag `1`.
- Sound evidence: if type field `+0x568 != -1`, code `0x0073DC1E..0x0073DC67` plays at the transport coordinate via `0x00750E20`.
- Active in YR: Yes. This is the normal success continuation from generic passenger placement.

Negative comparison: this differs from verified CanBeOccupied sell/destruction ejection, which queues Infantry Scatter mission `0x0F`. No `vtable+0x1E8(0x0F)` appears in the generic transport success block inspected here.

## 4. RNG Contract

Material finding: destination selection does not use scenario RNG and does not use `% 8`.

- Placement direction source: `RateTimer::Current @ 0x004C93D0`, not the scenario RNG object.
- Direction order: wrapped eight-direction deterministic scan from the timer-derived base.
- Return-delay RNG evidence: assembly `0x0073E289..0x0073E2B5` gets the mission timer entry, multiplies by `900.0`, converts with `Math::ftol`, then calls `Random::RandomRanged @ 0x0065C7E0` with lower `0` and upper `2` and adds that result to the return delay.
- Active in YR: Yes. The return-delay path is part of the mission function epilogue used by active mission states, including the generic passenger unload mission path.

Rust implication: parity requires no scenario RNG draw for the unload destination. A small scenario RNG draw may still occur for mission scheduling jitter after the mission function invocation.

## 5. Current Rust Comparison

Observed Rust surface: `src/sim/passenger.rs::tick_unloading`.

- Current Rust chooses the first unoccupied adjacent cell from a fixed `NEIGHBORS` order; `gamemd.exe` uses a timer-derived wrapped 8-direction scan plus passability validation from the passenger.
- Current Rust then consumes `sim.rng.next_u32() % 8` to choose a scatter destination around the exit cell; `gamemd.exe` does not use raw `% 8` or scenario RNG for this destination and does not queue Scatter in this path.
- Current Rust issues direct movement after placement; `gamemd.exe` queues mission `2` and separately hands off the chosen destination cell through passenger vtable `+0x480`.
- Current Rust unload order depends on Rust cargo container semantics; `gamemd.exe` pops the cargo linked-list head with no RNG.
- Current Rust should not inherit CanBeOccupied sell/destruction ejection behavior for generic transports.

## 6. Implementation Handoff

1. Verified behavior: generic transport unload scans `(timer_base + i) & 7` over eight directions and validates passability before placement -> Rust delta: replace fixed first-free-neighbor selection for generic manual unload with a parity direction-order/search helper -> affected surface: `src/sim/passenger.rs::tick_unloading` and passability/movement integration -> acceptance scenario: blocked APC with only the third scanned direction passable unloads to that direction without consuming destination RNG -> proposed test name: `generic_transport_unload_uses_timer_seeded_direction_scan` -> risk: medium, because passability helper availability may not yet match `FootClass::Can_Enter_Cell`/nearby passable-cell behavior.

2. Verified behavior: successful generic unload queues Move mission `2` to the selected destination, not Infantry Scatter `0x0F` and not raw `% 8` scatter -> Rust delta: remove the generic post-unload `rng.next_u32() % 8` scatter choice and model a deterministic move-to-selected-cell handoff -> affected surface: passenger unload order/movement intent/RNG determinism -> acceptance scenario: same seed before unload remains unchanged except for documented mission-delay jitter, and passenger receives a move destination derived from the accepted placement direction -> proposed test name: `generic_transport_unload_does_not_consume_destination_rng_or_scatter` -> risk: high, because existing tests may encode the current random-ish scatter.

3. Verified behavior: the cargo pop primitive removes the cargo linked-list head and re-adds on placement failure -> Rust delta: ensure `PassengerCargo` unload order and failure rollback match head-pop/reinsert semantics rather than sorted entity order -> affected surface: `PassengerCargo::unload_first`, unload failure handling, and tests with multiple passengers -> acceptance scenario: two-passenger transport unloads the current cargo head first, and if all exits fail the same passenger remains cargo without loss -> proposed test name: `generic_transport_unload_pops_cargo_head_and_rolls_back_on_no_exit` -> risk: medium, because boarding insertion order must be verified separately before declaring FIFO/LIFO.

## 7. Negative Facts / Do Not Do

- Do not map Rust generic manual transport unload to `UnitClass::Mission_Unload @ 0x00740EF0`; that path uses RepairBay/docking search.
- Do not apply CanBeOccupied garrison sell/destruction edge ejection or Infantry Scatter mission `0x0F` to generic vehicle transport unload.
- Do not consume raw scenario RNG modulo 8 to choose the generic unload destination.
- Do not treat `UnitTypeClass+0x5E0` as Storage in this path; parser evidence identifies it as `Passengers`.
- Do not assume the verified cargo head-pop automatically means FIFO or LIFO at player level until boarding insertion order is fully classified.

## 8. Remaining Uncertainty

- The exact sidebar/hotkey command setter that assigns mission `0x10` to generic transports was not traced in this slice. This does not block the unload body mapping, but it remains outside this report.
- The full semantics of passenger vtable `+0x1AC`, `+0x480`, and infantry helper `FUN_004ACA10` were not decomposed beyond their role in validation/destination handoff.
- Cargo boarding insertion ordering beyond `CargoClass::AddPassenger` head/splice behavior remains a separate parity question.

## 9. Stale-Doc Replacement Wording Found

Path: `docs/research/MISSION_UNLOAD_GHIDRA_REPORT.md`

Replacement wording:
`UnitClass::Mission_Unload @ 0x00740EF0 is a RepairBay/docking mission wrapper that calls FootClass::Find_Docking_Bay via vtable +0x528 using RulesClass+0x850 ([General] RepairBay=). Generic vehicle passenger disgorge is in UnitClass::Mission_Deploy_Building @ 0x0073D630 when UnitTypeClass+0x5E0 (Passengers) is greater than zero.`

Path: `docs/research/UNIT_MISSION_DEPLOY_BUILDING_GHIDRA_REPORT.md`

Replacement wording:
`UnitTypeClass+0x5E0 is Passengers, not Storage. In UnitClass::Mission_Deploy_Building @ 0x0073D630, the Passengers > 0 branch includes generic vehicle transport passenger unload. State 3 pops one passenger from CargoClass and performs generic placement/destination handoff; it is not only a harvester/refinery approach state.`

Path: [docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md)

Replacement wording:
`Successful generic passenger unload from UnitClass::Mission_Deploy_Building @ 0x0073D630 calls TechnoClass::ClearInOpenTransport @ 0x007104A0 for OpenTopped transports at 0x0073DB85..0x0073DB98. Any earlier no-clear statement should be scoped away from the generic passenger disgorge body.`

## 10. Evidence Ledger

| Claim | Active in YR | Evidence |
| --- | --- | --- |
| Generic vehicle passenger unload body is `UnitClass::Mission_Deploy_Building @ 0x0073D630` | Yes | `Passengers > 0` branch in decompile; parser write to `UnitType+0x5E0` at `0x00714B35..0x00714B50`; stock YR transport `Passengers=` values |
| `UnitClass::Mission_Unload @ 0x00740EF0` is not the body | Conditional, not for this behavior | Decompile and assembly `0x00740EF3..0x00740F08` call vtable `+0x528` with `Rules+0x850`; prior reader evidence identifies `Rules+0x850` as RepairBay |
| Cargo order is head-pop | Yes | `0x0073D8B7..0x0073D8CD` calls `0x004DE710`; `0x00473430` pops `[CargoClass+4]`, clears passenger `+0x30`, decrements count |
| Destination search is timer-derived 8-direction scan, not `% 8` RNG | Yes | `0x0073D8DC..0x0073D90D` calls `RateTimer::Current`; `0x0073D917..0x0073DA02` scans `(base+i)&7`; no scenario RNG call in placement loop |
| Success queues Move mission `2`, not Scatter `0x0F` | Yes | `0x0073DBD1..0x0073DBDB` pushes `2` to vtable `+0x1E8`; no `0x0F` queue in inspected success block |

## 11. Status

COMPLETE for the requested unload-body mapping and Rust-facing parity handoff. Non-blocking uncertainty remains around the UI command setter and helper internals, both outside the bounded scope.
