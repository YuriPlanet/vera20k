# Tank-bunker entry

Tank-bunker orders use the shared Unit destination, radio contact and Foot Enter
owners. The destination is the bunker object, so `GetDockCoord` chooses the
approach point. The bunker starts Repair when Unit PerCellProcess sends DOCK_NOW
at arrival; issuing an order from across the map does not start installation.
AI script action 61 retains its separate destination, HELLO, then immediate Enter
sequence. Occupant links are created only when installation finishes.

## Original comparison

```sh
export VERA20K_GAMEMD_EXE=/path/to/retail/gamemd.exe
python -m tools.spatial_oracle.bunker_entry --check
```

`--write` regenerates the reference and metadata from original execution. The
harness reuses the refinery-docking VM and executes unchanged original code;
prepared objects, map occupancy and type scalars are declared inputs. It is not
a complete game startup or a retail rules-reader comparison.

The connected Rust consumer is
[`bunker_entry_oracle_tests.rs`](../../src/sim/world/bunker_entry_oracle_tests.rs).
Its original-code histories cover destination/contact establishment, Enter
cadence, busy and replaced destinations, cancellation and the supplied arrival
boundary. The supplied arrival does not prove the intervening path traversal.
Separate stock rules and ART tests issue real orders and advance ordinary world
frames through installation, cancellation and entry from different directions.
Those are Rust integration regressions, not a whole-match native comparison.

| Owner | Original code |
| --- | --- |
| Unit object destination, busy bunker and final handshake | `741970`, `741D24..741E2A`, `742CAF..742D3D` |
| Building DOCKING and bunker eligibility recheck | `43C7E9`, `43C84C..43C87F` |
| Radio contact establishment / release | `65A970`, `65ACB0` |
| Approach coordinate | `447B20`, bunker direction branch `447C44` |
| Foot Enter and Scenario cadence draw | `4D9290` |
| AI tank-bunker seek | `4DFF40` |
| Arrival DOCK_NOW and locomotor power-off | `73A31F..73A540` |
| Building installation partner and state | `458E50`, occupant else contact zero |

## Gameplay check

With retail Yuri rules and ART, place a Tank Bunker and send a Lasher Tank from
several cells away. Check approaches from each side: the tank reaches the bunker,
turns onto its foundation, faces south, and the walls rise. Cancel an approach
while the tank is still far away, then send another tank to the same bunker.
Also check a Yuri AI team using script action 61 and deploy an installed tank
back out. Record interactive results separately from automated tests.

Removing the former approach marker and duplicate installation candidate changes
the serialized bunker layout. The snapshot version is advanced so older saves
are rejected before their positional payload is decoded.
