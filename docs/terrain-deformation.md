# Terrain deformation

Explosions can dent the ground, as in Tiberian Sun. It is off by default. Tick **Options → Game
Options → Terrain Deformation** to switch it on for the next game; a nuke then leaves a crater too.
In an online game every player needs the same setting, which the rules compatibility check
compares, and a save keeps the rules it was started with. Quick Match plays the original game, so
it asks you to switch the box off first.

The box switches on this rules key for each new game. A mod, a map or your
[personal rules](personal-rules.md) (`VERARULES.INI`) can set it as well, and a map can switch it
off for its own game:

```ini
[CombatDamage]
TerrainDeformation=yes   ; default no
DeformLowestLevel=-4     ; the lowest Level a crater digs to, default -4
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

Switched off, no extra random draw is spent, no extra tiles load and zones read cell Levels as
gamemd does.

## How a dent forms

The roll is Tiberian Sun's: an explosion within one level (104 leptons) of the ground under it,
whose damage is above `DeformThreshhold=`, spends one Scenario `Random_Pick(0, 99)` draw after the
explosive-overlay branch and dents its cell when the draw is below `damage * Deform`, with `Deform`
as a fraction (an ion cannon blast with `Deform=100%` always does).

gamemd has no deformation body, so the dent uses its own terrain code: the random map generator's
corner-height engine (`RandomMapGenerator::CreateHills` @ `0x005A35F0`), which sinks a cell's
corners a level at a time, pulls neighbouring corners along so no slope is steeper than one level
per cell, and rolls the step back when a corner it needs cannot move. A dent sinks every cell within
`DeformRadius=` cells of the hit by one level (default 0, the hit cell alone as in Tiberian Sun; at
most 11, the widest blast gamemd's cell-spread table holds), and the cells nearer the middle
further, down to `DeformDepth=` levels in the middle (default 1; at most 16): level *k* reaches
`(radius + ½) * (depth - k + 1) / depth` cells, so the crater is a terraced bowl. Each cell's step
rolls back alone, so the crater bends around ground it cannot dig. The engine runs over a window of
16 cells around the hit, and its rule for which cells may change starts from gamemd's
(`0x006B2520`): no overlay (walls, ore, bridges), no object (units, infantry, buildings, trees) and
a `Morphable=` tile. VERA narrows it once: gamemd's rule runs before a random map has bridges, so a
cell a bridge has flagged stays too, because most cells of a high bridge hold no overlay. And VERA
widens it twice. What the blast kills no longer holds the ground up, so a crater digs under the
units, buildings and trees that die in it, while living ones keep their cells and never sink or
float; an infantry death or a fire that plays out on a dug cell stays where the ground was. And flat
land digs even when its tile is not `Morphable=`: Clear, Road and Rough ground (roads, pavement,
crops, sand, rough grass) with no water, beach, cliff, ice, tunnel or rail beside it, since shore
and cliff pieces mix such cells with flat ones and a ramp stamped there would cut the piece's art.
The reshaped cells take the theater's clear tile or its ramps, and the LAT and ramp smoothing of the
cells around them is redone as cliff collapse does it (`MapClass::CollapseDestroyableCliff` @
`0x00581140`). Art exists only for whole-level ramps, so a ring of ramps climbs back to the ground
around the crater, and no slope in it is steeper than a level per cell: the nuke's crater is 23
cells across on open ground, as wide as its blast. Repeated hits dig deeper and wider, down to
`DeformLowestLevel=`.

Craters are part of the saved terrain and the simulation hash. A flat cell that tilts loses its
smudge. With the switch on, the atlas preloads the clear, ramp, smoothed-ramp and LAT tiles a dent
can draw.

Ground below Level 0 is stored as a signed byte, as gamemd's cell Level is, and heights, lighting,
pathfinding and drawing read it signed. gamemd's cursor scan (`0x006D6590`) stops a row below the
cursor, because its ground never draws lower than flat; once a map has ground below Level 0 VERA's
carries on up by the deepest level. The zone flood fills (`MapClass__FloodFillBaseZoneScanline` @
`0x0056CB90`, `MapClass__FloodFillZoneLevelScanline` @ `0x005824A0`) read their copy unsigned, so
Level -1 would be a cliff 255 levels high and cut a crater off from the ground around it at the next
zone rebuild. With the switch on they read it signed too; for every Level a retail map holds, both
readings agree.
