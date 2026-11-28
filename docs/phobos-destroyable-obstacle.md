# Destroyable pathfinding obstacles

`IsDestroyableObstacle=yes` makes a building a pathfinding obstacle that
behaves like a wall: its foundation cells answer the Destroyable cell class, so
movement rows that do not admit that class cannot route into it, and the zones
return to the terrain class once the building is destroyed or sold.

```ini
[OBELISK]           ; any BuildingType
IsDestroyableObstacle=yes
```

## Rules key

`[BuildingType] IsDestroyableObstacle=` (`Valueable<bool>`, default `no`,
`Ext/BuildingType/Body.cpp:221`, declaration `Body.h:66`). The documentation
prose spells it `IsDestroyableBlockage`; the reader uses `IsDestroyableObstacle`.

## Native evidence

- `CellClass::RecalcZoneType`'s object walk (`0x483D8E`) is hooked to jump to
  `0x483CD4`, the branch that stores cell class `2` (Destroyable), instead of
  writing the ordinary building class (`Ext/Building/Hooks.cpp:568`).
- `BuildingClass::Unlimbo` (`0x440D01`) and `Limbo` (`0x445D87`) recalculate
  that building's foundation cells and their zones/sub-zones, so the class
  appears on placement and disappears on destruction or sale
  (`Ext/Building/Hooks.cpp:533-543`).
- Class `2` is the passability matrix's Destroyable/wall column. Rows Destroyer,
  AmphibiousDestroyer, Subterranean, InfantryDestroyer, Fly and CrusherAll
  admit it; Normal, Crusher, Amphibious, AmphibiousCrusher, Water and WaterBeach
  do not (`MOVEMENT_ZONE_PASSABILITY` in
  `src/sim/pathfinding/passability.rs`).

## Where VERA20K owns it

- `rules::object_type::ObjectType::phobos_destroyable_obstacle` (BuildingType
  only).
- `sim::world::navigation::destroyable_obstacle_cells`: the base foundation of
  every marked such building.
- `map::resolved_terrain::ResolvedTerrainGrid::reconcile_destroyable_obstacle_cells`:
  writes class `2` while the building is marked and restores `RecalcZoneType`'s
  terrain answer when it leaves the set.
- `Simulation::rebuild_dynamic_navigation` reconciles the set before the zone
  flood; `mark_entity_put` / `unmark_entity_remove` republish it synchronously
  for a flagged structure (`0x440D01`, `0x445D87`), and the frame-end structure
  publication catches any other path that changes the set.

## Residuals

- The native hooks recalculate only the affected foundation cells and their
  zones; VERA reconciles the same class byte but rebuilds the whole zone map
  when the obstacle set changes. Placement is not a hot path, so the result
  matches while the execution strategy differs.
- A mover whose MovementZone admits class `2` still meets the building in its
  own entry walk, which refuses the occupant as it does for any building. The
  path can end adjacent, so the unit stops and fights the obstacle as the
  Phobos behaviour describes; it never drives onto the footprint.
- A flag on a non-BuildingType section is ignored: the reader is the
  BuildingType extension's.
