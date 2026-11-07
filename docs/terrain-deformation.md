# Terrain deformation

Explosions can dent the ground, as in Tiberian Sun. It is off by default and changes nothing until
the rules switch it on, for example in your [personal rules](personal-rules.md)
(`VERARULES.INI`):

```ini
[CombatDamage]
TerrainDeformation=yes   ; default no
DeformLowestLevel=-4     ; the lowest Level a crater digs to, default -4
```

gamemd still reads each warhead's `Deform=` and `DeformThreshhold=` but never uses them. Once this
is on, the retail warheads that set them (HE, V3WH, DMISLWH, CMISLWH, ARTYHE, SCHOPWH, ORCAHE and
IonCannonWH) dent the ground. The nuclear missile's `NUKE` warhead sets neither, so a nuke leaves a
crater only when you give it one:

```ini
[NUKE]
Deform=100%
```

A map or mod can set the same keys. In a network game every player needs the same personal rules,
which the rules compatibility check compares, and a save keeps the rules it was started with.
Switched off, no extra random draw is spent, no extra tiles load and zones read cell Levels as
gamemd does.

## How a dent forms

The roll is Tiberian Sun's: an explosion within one level (104 leptons) of the ground under it,
whose damage is above `DeformThreshhold=`, spends one Scenario `Random_Pick(0, 99)` draw after the
explosive-overlay branch and dents its cell when the draw is below `damage * Deform`, with `Deform`
as a fraction (an ion cannon blast with `Deform=100%` always does).

gamemd has no deformation body, so the dent uses its own terrain code. The cell's corners sink one
level through the random map generator's corner-height engine (`RandomMapGenerator::CreateHills`
@ `0x005A35F0`), which pulls neighbouring corners along so no slope is steeper than one level per
cell and rolls the whole dent back when a corner it needs cannot move. It runs over a window of 16
cells around the hit, and its rule for which cells may change is gamemd's (`0x006B2520`): no
overlay (walls, ore, bridges), no object (units, infantry, buildings, trees) and a `Morphable=`
tile. So units never sink or float; a crater only forms on open ground and bends around anything
standing next to it. The reshaped cells take the theater's clear tile or its ramps, and the LAT and
ramp smoothing of the cells around them is redone as cliff collapse does it
(`MapClass::CollapseDestroyableCliff` @ `0x00581140`). Repeated hits dig deeper and wider, one level
per hit, down to `DeformLowestLevel=`; each extra level of depth widens the slope by a cell on every
side, since art exists only for whole-level ramps.

Craters are part of the saved terrain and the simulation hash. A flat cell that tilts loses its
smudge. With the switch on, the atlas preloads the clear, ramp, smoothed-ramp and LAT tiles a dent
can draw.

Ground below Level 0 is stored as a signed byte, as gamemd's cell Level is, and heights, lighting,
pathfinding and drawing already read it signed. The zone flood fills
(`MapClass__FloodFillBaseZoneScanline` @ `0x0056CB90`, `MapClass__FloodFillZoneLevelScanline` @
`0x005824A0`) read their copy unsigned, so Level -1 would be a cliff 255 levels high and cut a
crater off from the ground around it at the next zone rebuild. With the switch on they read it
signed too; for every Level a retail map holds, both readings agree.
