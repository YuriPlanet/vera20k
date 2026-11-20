# Prevent crew, passenger and occupant escape

Three `[WarheadType]` booleans (all default `no`, read by
`Ext/WarheadType/Body.cpp:448-450`, constructors `Ext/WarheadType/Body.h:571-573`)
make a killing hit deny the victim's survivors:

- `PreventCrewEscape=yes` — the dying techno's crew is not spawned;
- `PreventPassengerEscape=yes` — its passengers are killed;
- `PreventOccupantEscape=yes` — a building's occupants are killed instead of
  released.

```ini
[WARHEAD]
PreventCrewEscape=yes
PreventPassengerEscape=yes
PreventOccupantEscape=yes
```

## Native evidence

`Ext/Techno/Hooks.ReceiveDamage.cpp:259-297`.

Hook `0x702672` (inside `TechnoClass::ReceiveDamage`'s death arm, after the
kill and revenge weapons):

```cpp
if (pWHExt->PreventCrewEscape)
    TechnoExt::Fetch(pThis)->PreventCrewEscape = true;

if (pWHExt->PreventPassengerEscape)
    pThis->KillPassengers(pSource);
```

`GetCrewCount` answers 0 while that latch is set
(`Ext/Techno/Hooks.cpp:2470-2491`), so the later crew block and every
survivor-count query see an uncrewed object.

Hook `0x442635` (inside `BuildingClass::ReceiveDamage`, before
`DestructionEffects`):

```cpp
if (WarheadTypeExt::Fetch(pWarhead)->PreventOccupantEscape)
{
    pThis->KillOccupants(pSource);
    return SkipClearOccupants;
}
```

It replaces the stock `SellBuilding(0,0)` release at `0x44263B`, so occupants
are killed even though the building would normally let them out.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::{prevent_crew_escape,
  prevent_passenger_escape, prevent_occupant_escape}`.
- `sim::game_entity::GameEntity::phobos_prevent_crew_escape`: the TechnoExt
  latch.
- `sim::combat::world_receiver`'s death path (`0x702672` block): sets the latch
  and calls `Simulation::kill_passengers`; its building arm calls
  `garrison_eject::kill_building_occupants` before the
  `BeforeBuildingDestruction` stage, whose stock release then finds none.
- Crew counts read the latch: `Simulation::spawn_vehicle_crew`,
  `Simulation::building_survivor_count` and the Ares
  `release_ares_vehicle_survivors` (`sim/crew_survival`).

## Residuals

- `KillOccupants` attribution: the shared owner takes no attacker, so an
  occupant's kill is not credited to the warhead's source (native passes
  `pSource`). The stock release path has the same limitation.
- The latch is only set on a killing blow, matching the hook site; non-fatal
  hits do not set it.
