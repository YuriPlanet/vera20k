# Stock rules retail leaves off

gamemd reads some rules keys that the retail INIs never switch on. A mod or a map that sets
them gets gamemd's behaviour in VERA too, in every build. The extension parts that hook the
same code are listed with their extension's chain doc.

## Assaulters

```ini
[SomeInfantry]
Assaulter=yes   ; default no; retail sets it on no infantry (Boris, Tanya, the SEAL say no)
```

An infantryman that is not an `Occupier=` but an `Assaulter=` may enter a garrisoned building
whose house does not count his own an ally, while anyone is inside
(`BuildingClass::CanBeOccupiedBy @ 0x00457CE0`, `0x00457DAD..0x00457DD3`). The player gets the
same Enter cursor and Capture order as for an occupier. When he reaches the building
(`InfantryClass::AddGarrisonOccupant @ 0x00522910`, `0x00522A09..0x00522A45`):

- every occupant is killed, last in first, through `BuildingClass::KillOccupants @ 0x004585C0`:
  the `AssaultAnim=` of the assaulter's primary weapon plays at that occupant's firing port
  (`MuzzleFlash<n>=` in the art, ZAdjust -200), and the occupant leaves the game without a
  death animation;
- gamemd books each kill the wrong way round: the assaulter's Record_The_Kill runs with the
  occupant as the killer (`0x004586D6`). The occupant's house scores the kill and the
  assaulter's house a loss, and the occupant earns the experience;
- the building keeps its owner; a civilian building goes back to its civilian house on its
  next AI, as when its occupants leave;
- the assaulter does not go in: he drops his destination and scatters from the building.

Code: `sim::passenger::can_dock_occupier_garrison`, `add_garrison_occupant` and
`passenger::garrison_eject::kill_building_occupants`. With Ares, `Assaulter.Level` limits
which buildings an assaulter may take and the kill is booked to him
([ares-transports.md](ares-transports.md), [ares-bugfixes.md](ares-bugfixes.md)).

## Detonating a bomb on a click

```ini
[CombatDamage]
CanDetonateTimeBomb=no   ; default no; retail no
CanDetonateDeathBomb=no  ; default no; retail no
```

Read by `RulesClass::ReadCombatDamage` (`0x0066C545`, `0x0066C564`). With one switched on,
the player selects a unit carrying a bomb of the player's house alone and clicks it: the cursor
is the Demolish frames (action 0x37) and the bomb goes off at once
(`TechnoClass::What_Action_OnObject` `0x006FFF8D..0x00700003`). The unit's owner is not asked,
so a selected enemy works too. A death bomb only exists with Ares, so stock builds read only
`CanDetonateTimeBomb`. An Engineer over its own bombed self defuses instead.

The click sends the DETONATE event (opcode 0x0A, the carrier's object token, the bomb's house
in the header; `FootClass::ClickedAction_Object` `0x004D7616`). Its Execute (`0x004C7823`)
drops the carrier's planning tokens and sets the bomb off (`BombClass::Detonate @ 0x00438720`,
silent for a carrier in limbo).

Not represented: an Aircraft or Building carrying the bomb, and a healer Unit, which answers
Select over itself first. Code: `sim::bomb` (`bomb_detonate_action`, `execute_detonate_event`),
`app::input::context_order::self_detonates`.

## Visceroids

```ini
[General]
SmallVisceroid=VISC_SML  ; retail names both types but defines neither
LargeVisceroid=VISC_LRG

[VehicleTypes]
900=VISC_SML

[VISC_SML]
SmallVisceroid=yes   ; UnitType key, default no
; or LargeVisceroid=yes
```

A Unit whose type sets `SmallVisceroid=` or `LargeVisceroid=` runs `UnitClass::UpdateVisceroid
@ 0x00737180` in its AI, after firing and turning (`0x0073666B..0x00736685`):

- its idle frames cycle: below Stage value 90, a value `% 30 == 29` takes a new
  `RandomRanged(0, 2) * 30`; at 90 or more, a value `% 5 == 4` restarts at one and the update
  ends there (Scenario draws);
- while it moves, nothing more;
- a small one sends the first small visceroid Unit it finds on the ground of a neighbouring cell
  (N first) over to itself, unless that one already has a destination or a target;
- with no destination or target it wanders a cell at a time, keeping its direction
  (`+0x6D4`) on a `RandomRanged(0, 2)` other than 0.

Can_Enter_Cell lets a small visceroid into a small visceroid Unit's cell (`0x0073F5B5`), so the
one sent over drives onto the other. gamemd has no merge: the two then share a cell and stay
small. With Ares they merge into `LargeVisceroid=` ([ares-tiberium.md](ares-tiberium.md)). Code:
`sim::visceroid`, called from the Unit AI in `world::object_turn`, and `world::object_entry`.
