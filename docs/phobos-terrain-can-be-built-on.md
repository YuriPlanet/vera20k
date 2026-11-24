# Terrain CanBeBuiltOn

A terrain object (tree, rock, tiberium tree) normally refuses a building on
its cell and blocks the placement flush. Phobos lets an individual terrain
type — or all ordinary terrain, or all tiberium trees — be replaced by the
building placed on it.

```ini
[General]
Terrain.CanBeBuiltOn=no    ; Valueable<bool>, default no (ordinary types)
Tibtree.CanBeBuiltOn=no    ; Valueable<bool>, default no (SpawnsTiberium=yes types)

[TIBTRE01]                 ; [TerrainTypes] section
CanBeBuiltOn=yes           ; Nullable<bool>, unset keeps the globals
```

Readers: `Ext/Rules/Body.cpp:481-482` (constructors `Ext/Rules/Body.h:896-897`,
both false) and `Ext/TerrainType/Body.h:34` (the per-type Nullable, read in
`Ext/TerrainType/Body.cpp`).

## Native body

`Ext/TerrainType/Hooks.Passable.cpp` and `Ext/Unit/Hooks.DeploysInto.cpp`:

- `CellClass_CanThisExistHere_IgnoreSomething` (`0x47C640`,
  `Ext/Unit/Hooks.DeploysInto.cpp:162-288`): in the ordinary
  arm a buildable Terrain inside the cell is admitted and sets
  `builtOnCanBeBuiltOn`, which also skips the occupation-bit test; a
  non-buildable Terrain still refuses. The LaserFence, LaserFencePost and
  Gate arms use the same `isTerrainBuildable` predicate.
- `BuildingTypeClass_FlushForPlacement_BuildableTerrain` (`0x45EF3A`): a
  buildable Terrain no longer blocks the foundation flush.
- `MapClass_PlaceDown_BuildableTerrain` (`0x5684B1`): placing a Building on
  the cell removes the Terrain from the cell and the game. The removal runs
  only after the placement succeeds.
- `OverlayClass_Unlimbo_SkipTerrainCheck` (`0x5FD2B6`): the overlay (wall)
  unlimbo skips its Terrain check and removes a buildable Terrain.
- `MapClass_IsAreaFree` (`0x586780`) with the wrapper at `0x5069DB`: while
  the AI evaluates building sites, a buildable Terrain stops blocking and the
  passability byte also admits `HasFreeSpots` instead of only `Passable`.
- `TacticalClass_DrawLaserFencePlacement_BuildableTerrain` (`0x6D57C1`):
  the laser-fence placement preview keeps drawing over buildable terrain.

## VERA

`PhobosGlobals::terrain_can_be_built_on` / `tibtree_can_be_built_on` read the
`[General]` pair, and `TerrainObjectType::can_be_built_on_key` holds the
per-type Nullable. The RuleSet constructor (`project_phobos`, also run for
map-only types) resolves the key else the global selected by
`SpawnsTiberium=`; `TerrainObjectState` carries the resolved bool.

- `build_site::objects_admit`'s ordinary arm admits a buildable Terrain and
  skips the occupation test (`0x47C640`); its LaserFence and LaserFencePost/
  Gate arms admit one too.
- `build_site::flush_for_placement` continues past a buildable Terrain
  (`0x45EF3A`).
- `world::lifecycle::mark_entity_put` (the PlaceDown owner) retires a
  buildable Terrain in the origin cell after the placement succeeds
  (`0x5684B1`); `clear_mobile_placement_cell` skips it instead of refusing,
  and any other Terrain still refuses the foundation clear.
- `world_spawn::wall_unlimbo::unlimbo_constructed_wall` skips its Terrain
  refusal for a buildable Terrain and removes it (`0x5FD2B6`).
- `cell_rect::check_occupancy_rect` takes an `ai_build_location` flag; the AI
  site search (`SimSiteWorld::clear`) sets it. There the buildable Terrain
  stops blocking and zone 0 or zone 5 passes (`0x586780`): native
  `PassabilityType::HasFreeSpots` is value 5 (`RecalcZoneType` writes
  `[edi+0x4C],5` at `0x483E1E`), which VERA names `zone_class::BUILDING`.

## Tests

- `src/sim/build_site_tests.rs` — type key/both globals/`SpawnsTiberium`
  resolution and an explicit `no` beating the globals; admission, flush and
  PlaceDown removal through production code, with the ordinary terrain
  refusing all three.

## Residuals

- The laser-fence placement preview (`0x6D57C1`) is presentation and is not
  ported.
- The wall Overlay arm removes a buildable Terrain in `unlimbo_constructed_wall`
  only; other Overlay unlimbo callers do not perform the 0x5FD2B6 removal.
