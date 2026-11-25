# Terrain deformation

Explosions can dent the ground, as in Tiberian Sun. It is off by default. On the skirmish screen,
press **Game Options** and tick **Terrain Deformation**; every later game plays it, and a nuke then
leaves a crater too.
A LAN or online room plays its host's setting, which the host can change in the room's Game
Options, and a save keeps the rules it was started with. Quick Match plays the original game,
without it.

The box switches on `TerrainDeformation=` for each new game. A mod or a map can set these keys as
well, and a map can switch deformation off for its own game:

```ini
[CombatDamage]
TerrainDeformation=yes   ; default no
DeformLowestLevel=-4     ; the lowest Level a dent digs to, default -4

[General]
CraterLevel=1            ; how many dents a landing meteor makes, default 4; retail sets 1
```

gamemd still reads each warhead's `Deform=` and `DeformThreshhold=` but never uses them. Once this
is on, the retail warheads that set them (HE, V3WH, DMISLWH, CMISLWH, ARTYHE, SCHOPWH, ORCAHE and
IonCannonWH) dent the ground. The nuclear missile's `NUKE` warhead sets neither, so the box also
gives it `Deform=100%` and VERA's `DeformDepth=3` and `DeformRadius=10`, each unless the rules
already set it. With the rules key alone, give them yourself:

```ini
[NUKE]
Deform=100%
DeformDepth=3    ; levels the middle of a crater sinks, default 1
DeformRadius=10  ; cells around the hit cell that sink, default 0
```

Switched off, no extra random draw is spent, no extra tiles load, nothing is added to the saved
game or its checks, and zones read cell Levels as gamemd does.

## How a dent forms

With the default keys, a dent is Tiberian Sun: Firestorm's (Firestorm 2.0, `GAME.EXE`).

**The roll** (Firestorm 2.0 0x46006B, in its explosion damage after the explosive-overlay branch).
An explosion whose damage is above `DeformThreshhold=` spends one Scenario `Random_Pick(0, 99)` draw
and dents its cell when the draw is below `damage * Deform`, with `Deform` as a fraction (an ion
cannon blast with `Deform=100%` always does). The height of the blast does not matter: one high in
the air dents the ground under it too. Only after the draw, a blast at or above the deck of a high
bridge leaves the ground under the bridge alone.

Firestorm also rolled in its wide-area damage (Firestorm 2.0 0x460440), once for the middle cell and
again in each cell's explosion around it. It used that for a dying object with `Explodes=`, for a
dying harvester's tiberium and for a nuke whose blast animation could not be made. Yuri's Revenge
keeps that function (`0x0048A700`) without the roll, and nothing calls it. The same three blasts
explode once instead: a dying `Explodes=` object fires its death weapon, a harvester's tiberium
does nothing (Ares adds one blast), and the nuke's fallback is one ordinary explosion. Each of these
explosions rolls once, as above.

**Meteors** (Firestorm 2.0 0x4151B1 and 0x65E58D). A landing meteor, a cartoon one or a voxel one,
that is not on a bridge deck dents its landing cell when `CraterLevel=` is not 0; retail sets 1.
Above 1 it also dents the four cells diagonal to it, above 2 all eight cells around it, and above 3
its landing cell a second time.

**The dent** (Firestorm 2.0 0x51E950). When the cell and all eight cells around it lie on the map,
one Scenario `Random_Pick(0, 3)` draw picks a corner of the cell, and that corner sinks one level.
Five frames later the other three corners sink too (0x51EA50). There is one such waiting step: a
later dent replaces it, so of a meteor's dents only the last one's second step comes.

**Sinking corners** (0x5F9AB0). The corners within 14 cells of the dented cell are read. A corner
sinks a level, and each corner around it that would be more than a level higher sinks with it, so no
slope is steeper than one level per cell. A corner stays where it is if sinking would take it below
`DeformLowestLevel=` or leave it above Level 12. If a corner that must move cannot, nothing changes
at all. Yuri's Revenge ground can reach Level 14, and VERA keeps Firestorm's top: a corner at
Level 14 stays where it is.
Then every cell with a corner that moved, or tried to, is rewritten: its Level, its slope and the
theater's clear tile or its ramp, its smudge gone, and each vehicle and soldier on it set back on the
ground, those the blast just killed too.

**Which cells may change** (0x5F83D0). A cell on the map with a `Morphable=` tile (clear ground and
its ramps), no overlay (walls, ore, bridges), no building, tree or other object but vehicles and
soldiers, nothing on a bridge over it, and no bridge over it or over the cell north or west of it.

The rest of the dent is VERA's:

- Firestorm's floor is Level 0, where most YR ground lies, so VERA's is `DeformLowestLevel=`,
  default -4; a map that sets 0 gets Firestorm's. On ground at the floor a dent still takes, as in
  Firestorm: the picked corner stays, but the cells around it are rewritten as above, the waiting
  step fills, and five frames later the cells around the other three corners are rewritten too.
- VERA keeps every cell a bridge has marked, not only the ones under a bridge, because rewriting a
  cell would clear the bridge's marks on it.
- After the dent, the sand, pavement and other LAT ground around the rewritten cells, and the ramps
  beside them, are smoothed as cliff collapse does it (`MapClass::CollapseDestroyableCliff` @
  `0x00581140`), so their art joins up. Firestorm leaves the tiles as the dent wrote them.
- Firestorm builds its corner grid over the whole map (0x5F8500) and fills the corners within 14
  cells of the dented cell; VERA's grid ends there, 14 corners west and north and 15 east and south,
  and a lowering whose pull reaches that edge fails whole. Only ground that climbs a level per cell
  for 14 cells from the dent pulls that far.

A wider or deeper crater is VERA's. A warhead with `DeformRadius=` or `DeformDepth=` starts with the
dent above, both steps at once, and keeps sinking with the same corner rules: every cell within
`DeformRadius=` cells of the hit sinks a level (at most 11, the widest blast gamemd's cell-spread
table holds), and cells nearer the middle further, down to `DeformDepth=` levels in the middle (at
most 16): level *k* reaches `(radius + ½) * (depth - k + 1) / depth` cells, so the crater has
steps. Each cell's sinking happens whole or not at all, so the crater bends around ground it cannot
dig, and the crater looks at 16 cells around the hit. Art exists only for whole-level ramps, so a
ring of ramps climbs back to the ground around the crater: the nuke's crater is 23 cells across on
open ground, as wide as its blast. Repeated hits dig deeper and wider, down to `DeformLowestLevel=`.

A crater widens the dent's rule for which cells may change twice. What the blast kills no longer
holds the ground up: the crater digs under the buildings, units and trees that die in it, while
living buildings and trees keep their cells and living vehicles and soldiers ride the ground down. A
building that dies there keeps its height while its death plays out. And flat land digs even when
its tile is not `Morphable=`: Clear, Road and Rough ground (roads, pavement, crops, sand, rough
grass) with no water, beach, cliff, ice, tunnel or rail beside it, since shore and cliff pieces mix
such cells with flat ones and a ramp stamped there would cut the piece's art.

Dents and the waiting second step are part of the saved game and the simulation hash. With the
switch on, the atlas preloads the clear, ramp, smoothed-ramp and LAT tiles a dent can draw.

Ground below Level 0 is stored as a signed byte, as gamemd's cell Level is, and heights, lighting,
pathfinding and drawing read it signed. gamemd's cursor scan (`0x006D6590`) stops a row below the
cursor, because its ground never draws lower than flat; once a map has ground below Level 0 VERA's
carries on up by the deepest level. The zone flood fills (`MapClass__FloodFillBaseZoneScanline` @
`0x0056CB90`, `MapClass__FloodFillZoneLevelScanline` @ `0x005824A0`) read their copy unsigned, so
Level -1 would be a cliff 255 levels high and cut a crater off from the ground around it at the next
zone rebuild. With the switch on they read it signed too; for every Level a retail map holds, both
readings agree.
