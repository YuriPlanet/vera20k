# Powered.KillSpawns

An armed spawner building that loses power gives up its aircraft: the spawn
manager's target is reset and every child that is attacking or coming home is
destroyed.

```ini
[SOMEBUILDING]                ; BuildingType
Powered.KillSpawns=no         ; Valueable<bool>, default no
```

Reader `Ext/BuildingType/Body.cpp:193`, declaration
`Ext/BuildingType/Body.h:30`, constructor `Body.h:167`.

## Behavior

`BuildingClass::AI` (`0x43FB24`) samples its operational predicate
(`0x004555D0`, vt+`0x350`) at `0x43FB2B` and compares it with the retained
byte `+0x6C8`. On a changed sample it runs the working-sound/dispatcher tail
and stores the new sample at `0x43FBEF`. Phobos
`BuildingClass_AI_PoweredKillSpawns` (`0x43FBEF`, 6 bytes,
`Ext/Building/Hooks.cpp:30-37`) calls `BuildingExt::ApplyPoweredKillSpawns`
(`Ext/Building/Body.cpp:293-311`) there, before the store:

- `Powered.KillSpawns` on and the building now offline;
- with a SpawnManager, `ResetTarget` (the shared
  `ClearAllTargets @ 0x006B7BB0` body) first;
- then every node whose status is `Attacking` or `Returning` takes its own
  current Health as `C4Warhead` damage with defenses ignored:
  `ReceiveDamage(&child->Health, 0, C4Warhead, nullptr, true, false,
  nullptr)`.

The edge is what triggers it: a building that is already offline, or that
stays powered, never runs the kill.

## Residuals

The change-edge call runs ahead of the stored sample, so a power transition and the kill in the same
visit read the `operational` value of the previous sample, exactly as the native AI visit does.

## Where VERA20K owns it

`rules::object_type::ObjectType::phobos_powered_kill_spawns` (Building types
only) and `Simulation::phobos_powered_kill_spawns` in
`sim::spawn_manager` (reset through the existing `ClearAllTargets` owner, then
the `ReceiveDamage` direct-receiver event with the interned
`[CombatDamage] C4Warhead=`). The call site is the changed-edge path of
`Simulation::visit_building_operational` (`sim::world::gap_generator`), ahead
of the stored sample. The offline test reuses the sampled `operational` value
natively re-read by `IsPowerOnline()` in the same AI call.
