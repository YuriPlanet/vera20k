# Trailer.SpawnDelay

A VoxelAnimType that names a `TrailerAnim=` drops that Anim at its Location
while it flies. Phobos replaces the stock random every-other-tick spawn with a
restartable timer.

```ini
[DBRIS1LG]              ; any VoxelAnimType section
TrailerAnim=SMOKE       ; stock key, the spawned AnimType
Trailer.SpawnDelay=2    ; Phobos: game frames between spawns, default 2
```

Reader: `Ext/VoxelAnimType/Body.cpp:18` (`Valueable<int>`, constructor 2 at
`Ext/VoxelAnimType/Body.h:34,43`); the key lives in the type's rules section,
beside the stock `TrailerAnim`.

## Native body

- The stock block at `0x74A70E..0x74A7AA` spawns the trailer on
  `Randomizer::Global % 2 == 0` (`0x74A720..0x74A733`; the MSVC signed-rem
  idiom) and then continues into the Bounce update at `0x74A7AB`.
- Phobos `VoxelAnimClass_AI_Trailer @ 0x74A70E`
  (`Ext/VoxelAnim/Hooks.cpp:47-67`) skips that block. A type with
  `TrailerAnim` and an expired `TrailerSpawnTimer` restarts the timer with
  `Trailer.SpawnDelay.Get()`, constructs `AnimClass(TrailerAnim, Location, 1,
  1)` (delay 1, loop 1, default draw flags `0x600`), and calls
  `AnimExt::SetAnimOwnerHouseKind(OwnerHouse, nullptr, false, true)` so the
  trailer carries the piece's owner house (a null owner leaves it unowned).

## VERA

`VoxelAnimType::trailer_spawn_delay` reads the key (Phobos builds only).
`VoxelAnimObject` keeps Phobos' serialized `TrailerSpawnTimer`; it starts
expired, so the first flight visit spawns, and `trailer_spawn_due` restarts it
after each spawn. `visit_voxel_anim` runs the block on the Flight step before
the Bounce update, and `Simulation::spawn_trailer_anim` constructs the child
with the constructor's delay 1 / flags `0x600` and installs the owner house.

## Tests

`src/sim/world/voxel_trailer_tests.rs`:

- `trailer_spawn_delay_reads_with_a_two_frame_default`
- `the_trailer_spawns_every_delay_frames` (delay 3 → frames 0, 3, 6)
- `the_unset_key_spawns_every_two_frames`
- `a_zero_delay_spawns_on_every_visit`
- `an_unowned_piece_leaves_the_trailer_unowned`
- `the_trailer_inherits_the_piece_owner_house`

The timer gate and the owner installation were each mutated red and restored.

## Residuals

- The stock randomized block is not ported in non-Phobos builds: no retail
  `[VoxelAnims]` entry authors `TrailerAnim` (the five artmd `TrailerAnim=`
  sections — `DBRIS1LG`, `DBRIS5LG`, `DBRIS8LG`, `METLARGE`, `METSMALL` — are
  AnimTypes, not VoxelAnimTypes), so the branch is dormant on retail data; the
  Phobos build replaces it, as the hook does.
- The child's animation inherits VERA's constructor defaults for the arguments
  the hook does not pass (loop 1, flags `0x600`), matching the native ctor
  defaults the stock block also used.
- A `MakeInfantry=`/`CreateUnit=` trailer type re-selects its own owner kind in
  native; this port keeps the invoker house, the same residual
  `spawn_owned_anim` records.
- The timer is folded into the state hash only while it differs from the
  expired default, so pieces that never spawned keep their previous hashes.
