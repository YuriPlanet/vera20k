# Jumpjet flight: native facts behind the cruise port

Binary: YR 1.001 `gamemd.exe`, SHA-256 `1cdd1180…4298c`. Read 2026-09-15/16 from
disassembly. Two corpora make this **parity demonstrated** within their stated
scopes: `tools/spatial_oracle/jumpjet_flight.json` for the cruise body (Update
then State 3) and `tools/spatial_oracle/jumpjet_states.json` for the whole
`Process` gate and state machine (States 0–4). See their `.meta.json` files for
the declared assumptions and substitutions. Everything else here is **native
behaviour established** from bodies and callers, or marked otherwise. Rust owners: `src/sim/movement/jumpjet_flight.rs` (kernel) and
`src/sim/world/jumpjet_cruise.rs` (production host).

## Process model

- `JumpjetLocomotionClass::Process @ 0x0054AEC0` calls
  `Update_Coordinates_And_Altitude @ 0x0054D0F0` only while the interface's
  `Is_Moving` (+0x10, `0x0054AE50`, the moving byte at receiver `+0x4C`) or
  `Is_Moving_Now` (+0x80, `0x0054D0D0`) answers true. `Is_Moving_Now` is simply
  *state is neither 0 nor 2*, so an idle landed owner (state 0, not moving) and
  an idle holding owner (state 2, not moving) are advanced by **nothing**: they
  neither bob nor drift. It then dispatches the state at receiver `+0x50`
  through the jump table at `0x0054B19C`: 0 ground `0x0054B980`, 1 ascend
  `0x0054BA30`, 2 hold `0x0054BD30`, 3 translate `0x0054BFF0`, 4 descend
  `0x0054C550`, 5 touchdown/crash `0x0054CA90`; 6 dispatches nothing.
- Between the Update and the dispatch sits the crash latch: with owner `+0x425`
  set, a state other than 5 or 6 and `GetHeight > 0`, the owner's cell is
  compared with the destination cell, and a piggyback owner or a match forces
  `+0x80 = -5` and state 5. `0x0053A130` is the constant-false leaf, so its arm
  is dead. The tail runs two visibility probes (`0x00586360`, `0x005865E0`) and
  re-submits the object (`0x004A9720`) when the layer query `0x0054B8D0`
  reports a changed answer.
- Receiver-relative fields: `+0x1C..+0x3C` type block, `+0x40` destination XYZ,
  `+0x4C` moving byte, `+0x50` state, `+0x54` the locomotor's own FacingClass,
  `+0x70` current speed (double), `+0x78` target speed (double), `+0x80` target
  height (int), `+0x88` bob phase (double).

## Link and facing

- `Link_To_Object @ 0x0054AD30` (COM, `RET 8`) copies `TechnoTypeClass+0xD70..
  +0xD90`: turn rate, speed (int), climb, crash, height floored at two cell
  levels (`0x0054AD9B`), accel, wobbles, deviation, no-wobbles (`+0xD8C`). It then
  rebuilds `+0x54` with `FUN_004C91E0(type JumpjetTurnRate)` and snaps it to
  `0x4000`. The constructor's `[JumpjetControls] TurnRate` (Rules `+0x40C`)
  facing is therefore replaced; the body turns at the **type's**
  `JumpjetTurnRate=`. Stock misspells that key in every jumpjet section, so all
  stock jumpjets turn at the constructor's 4.
- State 0 (`0x0054B980`) snaps the locomotor facing to the body facing
  (`+0x388`) at takeoff; every Update copies the locomotor facing back to the
  body (`0x0054D67B..D692`), except Infantry with `JumpJetTurn=`
  (`InfantryTypeClass+0xECB`, key at `0x008258FC`) holding a target in state 2,
  which faces the target instead.

## Update (0x0054D0F0)

1. Speed ramp: if target > current, current += accel, capped at speed; then if
   target < current, current -= 1.5 × accel, floored at 0. The second test reads
   the updated value, so a target below the cap overshoots and is pulled back in
   the same frame. `SetSpeedFraction(current / speed)`.
2. Bob: states 2 and 3 without no-wobbles add `2pi / (15 / wobbles)`; otherwise
   the phase is zeroed. Target height `T = ftol(sin(bob) × deviation + +0x80)`.
3. Ground `G` = floor height at the owner; on a high-bridge cell, `G += 416`
   once `Z >= G + 4 levels − crash`.
4. Reference `R`: states 0/4 use `G`; otherwise `0x0054D820` unless the owner is
   in the destination cell without `BalloonHover=`. `0x0054D820` samples the
   current cell's top (`CellClass 0x00485080`: centre ground, plus the first
   building's `Dimension2` height, art `Height=` at `+0xEF4` times
   `g_HeightFactor` = 104 from the startup chain `0x0045AFA0..0x0045B070`
   (`tools/spatial_oracle/height_factor.json`), or 85 for any other techno) and,
   while moving,
   the cell one step ahead along the facing (direction deltas at `0x0089F6D8`,
   filled by the CRT initializer `0x0049F3A0`); it returns the ahead value when
   higher, else the average. Both of its bridge tests add the deck to the
   *current* sample.
5. Height: `A = Z − R`. Below `T`: climb by `JumpjetClimb=` or snap to `T`
   (with a grounded reset through vtable `+0xF4` at height 0). Above `T`: descend
   by climb or snap, never below `G`.
6. Outside the destination cell, `A < T/2` or `A < T/4` zeroes the speed.
7. XY step along `Current(+0x54)`: `Y −= sin × trunc(speed)`,
   `X += cos × trunc(speed)`, committed through `SetLocation`.

## State3_Translate (0x0054BFF0)

- Desired facing `ftol((atan2(curY−destY, destX−curX) − pi/2) × −65536/2pi)`,
  set gradually. Turn error `((diff16 >> 7) + 1) >> 1 & 0xFF` over the unsigned
  destination-minus-current difference: a quarter turn left reads 192, right 64.
- Distance `ftol(Sqrt_Approx(dx² + dy²))`. Arrival below 20 leptons: zero both
  speeds, snap XY to the destination, then state 4 (+0x80 = 0) for an ordinary
  owner; a `BalloonHover=` owner or one with a target claims the cell's air slot
  and holds (state 2) or scatters to a random neighbour when the slot is taken;
  a Unit with `IsSimpleDeployer=` (`UnitTypeClass+0xE13`) and `DeployToLand=`
  (`TechnoTypeClass+0x6AD`) holds at full height. The arrival notify `0x00705D60`
  returns early unless `TechnoClass+0x514` (a planning-code manager) is set.
- Zones with `S = JumpjetSpeed`: `d < S` → S/8, height/2; `d < 2S` → S/4,
  height/2, S/10 (floor 1.0) when the error exceeds the turn rate;
  `d < 50S/turn rate` → S/2, height × 0.75, S/5 (floor 1.0) when the error exceeds
  5 × turn rate; else S at full height. Height writes are skipped with a target.
- Tail: `BalloonHover=`, a Water or Beach destination (LandType 2/6), or a Unit
  with `DeployToLand=` keeps full height.

## States 0, 1, 2 and 4

Read from disassembly 2026-09-16; **parity demonstrated** for the scope of
`tools/spatial_oracle/jumpjet_states.json` (ten rows through the real `Process`).

- **State 0 ground `0x0054B980`.** Returns at once unless `Is_Moving`. Then
  `SetSpeedFraction(1.0)`, the locomotor facing `+0x54` snaps to the *body*
  facing `+0x388` (so takeoff keeps the body heading and the link-time `0x4000`
  never leaks), both speed doubles clear, `+0x80 = JumpjetHeight`, and the state
  becomes 1. The air-bucket add at `0x0054BA20` is gated on the owner's cell
  matching the global cell at `0x00ABC588`.
- **State 1 ascend `0x0054BA30`.** Height comes from `GetHeight` (+0x1C8), less
  the bridge deck while over a `0x100` cell. Below `+0x80`: the release gate is
  `+0x80 / 2` for a `BalloonHover=` Unit and `+0x80 / 4` otherwise (both
  truncating); above it, the target speed becomes `JumpjetSpeed=`, the facing is
  set toward the destination, and an owner already in the destination cell
  without `BalloonHover=` takes `+0x80 = GetHeight` and state 3. At or above
  `+0x80`: the cell's air slot is claimed (state 2), or, when another object
  holds it, `RandomRanged(0,7)` picks a neighbour, `Set_Destination` re-aims and
  the state becomes 3.
- **State 2 hold `0x0054BD30`.** Returns unless `Is_Moving`. A destination whose
  XY differs from the owner's re-opens the cruise: facing toward it, release the
  slot the owner holds, state 3. Otherwise a TarCom keeps it in place; a Unit
  with `IsSimpleDeployer=` and `DeployToLand=` (and `+0x134` clear) holds at
  `JumpjetHeight=`; a `BalloonHover=` type holds; anything else releases its slot
  and drops to state 4. Each hold or drop calls the arrival notify `0x00705D60`
  — **not** `0x006385C0`, which the decompiler's `thunk_` name suggested.
- **State 4 descend `0x0054C550`.** A piggyback owner skips admission entirely.
  A Water or Beach destination (LandType 2/6) refuses the landing: claim the
  slot and hold at full height, or scatter. Otherwise `Can_Enter_Cell` (+0x1AC),
  the destination sub-cell (`0x004810A0`, only ever 0, 2, 3 or 4) and
  `IsSubCellFree` (`0x00481130`) decide; a Unit settling on sub-cell 0 is always
  admitted. A refusal hands back to `Stop_Moving`; otherwise `+0x90` latches and
  `+0xF0` runs once. Then `+0x80 = 0` and, at height 0, touchdown:
  `SetSpeedFraction(0)`, `SetLocation(destination)`, NullCoord destination,
  moving byte clear, `Set_Destination(0, 1)`, fog, slot release, bucket remove,
  **state 0**, crate pickup, and the `+0x6AE`/`+0x427`/`+0x425`/`+0x90` latches.
- **`Move_To 0x0054B1C0`** stores the request, sets the moving byte and, when it
  lifts a descent, sets state 4 → 1 **and restores `+0x80 = JumpjetHeight`**,
  undoing State 4's `+0x80 = 0`. The corpus records this independently.

## Direction tables and the cached cell

Two distinct globals, both BSS and both zero in the image, filled by separate
CRT initializers listed in the table at `0x00812B90`:

- `0x0049F3A0` fills the **8-byte-stride lepton deltas** at `0x0089F6D8`
  (`0, -256`, `256, -256`, …) that the reference height `0x0054D820` reads.
- `0x0049F2F0` fills the **4-byte-stride packed adjacent-cell offsets** at
  `0x0089F688` (`0,-1`, `1,-1`, `1,0`, `1,1`, `0,1`, `-1,1`, `-1,0`, `-1,-1`,
  built from `DX = 0`, `CX = -1`, `AX = 1`) that the neighbour step
  `MapCoord_StepByDir_GetCell @ 0x00481810` reads at `[EDX*4 + 0x89F688]`.

They are *not* the same table. A harness that runs only `0x0049F3A0` leaves the
adjacent-cell table zeroed, and every scatter then steps by `(0,0)` onto the
owner's own cell — which is exactly how the first draft of the states corpus
recorded a scatter that never happened.

The owner's cached cell `+0x560` is written **only** through owner vtable
`+0x2F8` (`0x0041C160`: `MOV [ECX+0x560], EAX; RET 4`), called from the three
air-bucket helpers. `Mark` (vtable `+0x124`,
`TechnoClass__MarkCellLists @ 0x004D3780`) enters and exits cell lists and never
touches it. Three readers matter here: the State 1 (`0x0054BAA9`) and State 3
(`0x0054C036`) cell-change probes, and State 2's slot release at `0x0054BF7A`.

That last one is load-bearing. When State 2 re-opens the cruise and the current
cell's slot is empty, it releases the *cached* cell's slot if that still holds
the owner (`0x0054BF75..0x0054BFA8`), and only then releases the current cell —
unconditionally, even if another object holds it (`0x0054BFAD..0x0054BFBC`).
State 1 sets the target speed before promoting to the hold, so the owner drifts
out of the cell it claimed; without the cached-cell release the claim would
orphan. VERA has no cached cell and instead drops every slot the owner still
holds, which is what the cached cell names in practice.

The states corpus cannot witness this branch: its declared substitution leaves
`+0x560` at its supplied value (the only writers are the no-op air-bucket
helpers), so the cached cell never holds the owner and the original skips the
release. The corpus therefore records a claim on one cell and, a frame later, a
release of a *different* cell — the orphan, visible in the data. The VERA
behaviour is covered by a Rust regression test
(`re_opening_a_cruise_drops_a_drifted_slot`) rather than by a parity row.

## Numeric model

`WinMain` calls `_controlfp(0x300, 0x300)` (`0x006BBFB7..BFC1`) and
`0x007C5EE4` captures the word at `0x00822D80`; `Math::ftol @ 0x007C5F00`
reloads it without restoring. The process runs x87 at 53-bit precision rounding
toward zero, which the oracle confirms: truncating replay matches every bob sum,
round-to-nearest diverges after 13–14 frames. Sine and cosine read the table at
`0x0084F084` with the binary32 scale at `0x008223B0` (`0x4522F983`), the
arctangent reads the 4097-entry table at `0x008610B4` (step `0x3CC7FE84`,
FNV-1a `4056c36f7f1eab9c`), and the distance uses the `Sqrt_Approx` LUT.

## Production hand-off

VERA's air adapter no longer drives a Jumpjet at all: `world::jumpjet_cruise`
runs `Process` for every one of them, and `AirMovePhase` and the locomotor
altitude are *derived* from the native state byte (0 Landed, 1 Ascending,
2 Hovering, 3 Cruising, 4 Descending). That is what stops an idle non-balloon
Jumpjet cycling takeoff and landing every 102 frames, which the adapter did
because its idle branch turned `Landed` back into `Ascending` while `should_land`
turned `Hovering` into `Descending`.

An order arriving on `movement_target` stands in for `Move_To`: it stores the
destination, sets the moving byte and lifts a descent back to state 1 with
`+0x80 = JumpjetHeight`. A cruise whose order drops leaves state 3 for a stopped
hold with both speeds zeroed — native `Stop_Moving` would instead re-target a
nearby cell through FNPC and keep flying, which stays a recorded residual.

## Not yet ported

- State 5 crash (`0x0054CA90`) and the `+0x425` crash latch in `Process` that
  reaches it, including the infantry crush and the damage it deals on impact.
- `Stop_Moving 0x0054B4D0`'s FNPC re-target, and `Move_To`'s FNPC relocation of
  an ordered cell that is not passable.
- `Can_Enter_Cell`'s graded answer: VERA's passability predicate is binary, so
  the native 0 / 2 / above-2 distinction collapses to clear or refused.
- The owner missions that skip State 4's landing admission (`+0xB4` or the
  queued mission answering 7), `RulesClass+0x48`'s deploy facing, owner `+0x134`,
  `JumpJetTurn=` (`InfantryTypeClass+0xECB`) and the planning-token arrival
  notify `0x00705D60` (inert without `TechnoClass+0x514`).
- The air-bucket helpers `0x004134A0`/`0x004135D0`/`0x004138C0` and, with them,
  the cached-cell writes through `+0x2F8`; State 0's bucket add is additionally
  gated on the global cell at `0x00ABC588`, which VERA does not model. Because
  the cached cell is therefore never maintained, State 2's cached-cell release
  is modelled as "drop every slot this owner holds" instead.
- `Process`'s owner `+0x90` gate at `0x0054AEF5..0x0054AF00`: with that byte
  clear, Update still runs but the crash latch, the **whole state dispatch**,
  both visibility probes and `Submit_Object` are skipped. The writers
  (`ObjectClass__UnInit 0x005F6625`, the destructors, and
  `TechnoClass__ReceiveDamage`) read it as an object alive/valid flag, not an
  airborne-layer flag. VERA only advances live objects, so the practical risk is
  low, but the gate itself is unported. This is a different field from the
  locomotor's own `+0x90` landing latch that State 4 writes.
- The owner's `+0xF0` landing callback, which State 4 runs once alongside
  latching `+0x90`.
- Bridges, building tops and cell objects in the reference height remain
  Rust-tested only — no corpus row places one.
