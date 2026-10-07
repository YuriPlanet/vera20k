# Rocket oracles

## Rocket locomotor

`flight.py` runs the original `RocketLocomotionClass` (ILocomotion vtable
`007F0B1C`) through whole missile flights: the constructor `00661EC0`, Move_To
`006632E0`, then one Process `006622C0` per frame until Detonate `00663030`
UnInits the owner or the row's frame budget ends. The impact predictor
`006620F0`, Is_Moving / Is_Moving_Now, the sine, cosine and arctangent tables,
`Sqrt_Approx`, `atan2`, `ftol`, the owner's FacingClass Set/Current, the
`In_Bounds` diamond and the owner's GetCoords / cell / GetHeight bodies execute.
The owner's Mark, SetLocation, display, AircraftTracker, sound, anim and damage
calls are recorded seams, listed in the module docstring and `flight.meta.json`.

Use the repository's `tools/requirements-test.txt` environment (Unicorn 2.1.4),
set `RA2_DIR` or `VERA20K_GAMEMD_EXE`, then from the repository root:

```sh
python -m tools.rocket_oracle.flight --check
```

`--write` replaces `flight.json` and its `.meta.json` after review. The Rust
parity test replays every row through the production kernel
(`sim::movement::rocket_movement`) with a host that records the same seams:

```sh
cargo test -p vera20k --lib sim::movement::rocket_movement::tests::
```

## Coverage

Sixteen rows on retail-shaped `[General]` blocks:

| Rows | Exercises |
| --- | --- |
| `v3_*` | V3: no pause, tilt, climb along PitchFinal, LazyCurve cruise, steering, predictor impact at the destination height; elite payload; a second Move_To ignored; killed on the rail (explodes on the first moving frame) and in cruise; not-alive owner skips the tracker update; In_Bounds refusals; CruiseStartDistance 0 falls back to the level cruise and dive; structural-bridge deck impact |
| `dmisl_*` | Dreadnought: pause, level cruise, dive (state 5) and its three turn arms, a short shot that loops, a dive onto higher ground (height clause) |
| `cmisl_boomer` | Boomer: V3TAKOFF puffs and the display resubmit in the pause, the raise (state 6), the climb's resubmit |
| `dmisl_with_null_types_takes_the_boomer_arms` | the Boomer test compares type pointers: two unset types send a DMisl block down the Boomer arms |

Not covered: the seam bodies themselves (FootClass Mark/SetLocation, the
AircraftTracker buckets, AnimClass, SelectAnim, Apply_area_damage), a bridge
the owner stands on (`+0x8C`), save/load (`Load @ 00663410`) and the missile's
own mission (`AircraftClass::Mission_Move`).

## Kamikaze tracker

`kamikaze.py` runs the original tracker at `00ABC5F8`, which takes a missile
out of its launcher's control: its static initializer `0054E260` and Clear
`0054E6F0` at frame 0, then each row's script of Push `0054E3B0` (with the
callers' `{Frame, 2}` restart where the call site writes it), Update
`0054E4D0`, Remove `0054E590` and Clear. FacingClass Current,
GetOccupiedCell, Adjacent_Cell and GetCellAt execute over a cell array of
cells (0..127, 0..127); the child's Crash, Assign_Target and Queue_Mission
and the target's GetCoords are recorded or supplied seams.

```sh
python -m tools.rocket_oracle.kamikaze --check
cargo test -p vera20k --lib sim::kamikaze::tests::
```

Seven rows: the launch cadence (2 frames, then 30) with two missiles and their
removal, target coordinates rounding toward zero, the facing step for every
eighth boundary and a facing mid-turn, the Crash of a child without
`MissileSpawn=`, Remove scanning from the back (a duplicate, an absent child,
a target pointer), Clear's 1-frame restart, and a Kill_All_Spawns push that
waits for the running cadence.

Not covered: Remove's cell arm, Save/Load (`0054E750`/`0054E7B0`) and the
dummy cell GetCellAt answers off the cell array.
