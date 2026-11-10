# SHP building turrets

A building turret is a voxel model when its type sets `TurretAnimIsVoxel=yes`.
Without that key, `TurretAnim=` names an ordinary animation with 32 facing
frames, shown in the building's animation slot 9. This is original `gamemd`
behavior; no retail building uses it, so only mods and maps reach it.

```ini
[SOMEBASE]                    ; rules: a BuildingType
Turret=yes
TurretAnim=SOMETUR            ; a name from [Animations]
TurretAnimDamaged=SOMETURD    ; optional; the normal name when left out
TurretAnimX=0                 ; pixel offsets from the building's draw origin
TurretAnimY=0
TurretAnimZAdjust=-15
TurretAnimYSort=0
```

All of these are read from the building's rules section. `TurretAnim` in the art
file is not read. A name that is not listed in `[Animations]`, or whose SHP no
archive holds, leaves the building without a turret picture.

## Frame order

The animation does not play. Every frame the building writes the frame for its
heading into it and stops its frame step. With `bucket` the 32-step heading
(0 = north, 8 = east, 16 = south, 24 = west):

```
frame = (28 - bucket) & 31
```

So frame 0 points north-west (straight up on screen) and the ring runs
counter-clockwise: 28 is north, 20 east, 12 south, 4 west. Shadow frames, if the
SHP has them, follow the 32 facing frames.

## Native sites

- `BuildingTypeClass::ReadINI` `0x00464450..0x00464647` reads the keys from the
  rules INI.
- Grand_Opening `0x00446580..0x004466D5` builds the slot-9 animation the first
  time a building opens, for a building that has a turret and is not
  `TurretAnimIsVoxel`.
- The frame comes from the table at `0x007F4890`, written at `0x004466BE`
  (Grand_Opening), `0x0045127F` (every frame) and `0x0044FE10` (map load).

In VERA20K the row is `rules::art_data::building_turret_anim_config`, the
opening is `Simulation::open_turret_anim` and the frame write is
`Simulation::aim_building_turret_anim`.

## Not done

- `TurretAnimGarrisoned=` is not read; a garrisoned building keeps its normal
  turret animation.
- The map-load write is not a separate step; the every-frame write sets the same
  frame on the first game frame.
