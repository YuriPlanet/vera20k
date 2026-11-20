# Terrain passability

A terrain object (tree, rock, tiberium tree) normally blocks its cell.
Phobos lets an individual terrain type — or all ordinary terrain, or all
tiberium trees — be treated as passable.

```ini
[General]
Terrain.IsPassable=no      ; Valueable<bool>, default no (ordinary types)
Tibtree.IsPassable=no      ; Valueable<bool>, default no (SpawnsTiberium=yes types)

[TIBTRE01]                 ; [TerrainTypes] section
IsPassable=yes             ; Nullable<bool>, unset keeps the globals
```

Readers: `Ext/Rules/Body.cpp:479-480` (constructors `Ext/Rules/Body.h:894-895`,
both false) and `Ext/TerrainType/Body.h:33` (the per-type Nullable, read in
`Ext/TerrainType/Body.cpp`).

## Native body

`TerrainClass_SetOccupyBit_PassableTerrain` (`0x71C110`,
`Ext/TerrainType/Hooks.Passable.cpp:7-25`) returns before the occupation-bit
store when the resolved value is true, so the terrain contributes no cell
occupation. The `0x483DDF` arm sits inside `CellClass::RecalcZoneType`
(`0x483C80`): for a passable Terrain it sets `Passability = Passable` and
returns before the type-dword comparison that would classify the cell as zone
2 (Wall) or 5 (Building), so the cell keeps zone 0. `UnitClass::CanEnterCell`
(`0x73FB71`) skips the terrain checks, and `0x7002E9` selects a Move order
instead of an attack order unless force-firing.

## VERA

`PhobosGlobals::terrain_is_passable` / `tibtree_is_passable` read the
`[General]` pair, and `TerrainObjectType::is_passable` holds the per-type
Nullable. The RuleSet constructor projects the resolved value
(`project_phobos`, type key else the global selected by
`SpawnsTiberium=`). `TerrainObjectType::raw_occupation_bits` returns 0 for a
passable type, and the live/map cell projection stores **no**
`terrain_object_occupation` at all (rather than `Some(0)`, which would map to
zone 5): the object keeps its cell-list membership but contributes neither an
occupation mask nor a zone class, matching `0x483DDF`. The map-only terrain
types installed after the RuleSet constructor are projected too.

`TerrainObjectState` carries the resolved value; `Simulation::terrain_object_is_passable`
exposes it read-only. `app::input::context_order::attack_action` (the
`TechnoClass::What_Action` owner shared by cursor and click) applies the
`0x7002E9` arm before `base_action`: a current player's controllable Techno
whose target is a passable Terrain answers `AttackAction::Move` unless forced,
so no class tail runs (`Ext/TerrainType/Hooks.Passable.cpp:25-44`). Force-fire,
another house's actor, an offline actor and a non-passable Terrain all keep the
stock answer.

## Tests

- `src/sim/terrain_object.rs` — the type key, both globals, `SpawnsTiberium`
  selection, an explicit `no` beating the globals, the constructed state's
  zero bits and the projected cell no longer blocking.
- `src/app/input/context_order.rs` —
  `a_passable_terrain_answers_move_for_the_player_unless_forced`: the Move
  answer through production `attack_action`, plus force-fire, AI actor, offline
  actor and an explicit `IsPassable=no` type keeping the stock answer.

## Residuals

- The buildable-upon half of the same Phobos family is
  [its own page](phobos-terrain-can-be-built-on.md).
