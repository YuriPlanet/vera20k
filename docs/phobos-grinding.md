# Grinding gates and extras

Phobos lets a `Grinding=` building type restrict who it grinds and replaces
the sound and refund-driven weapon behavior (`Ext/BuildingType/Body.cpp:200-207`,
`Ext/Building/Body.cpp:228-290`, `Ext/Building/Hooks.Grinding.cpp`).

```ini
[GRINDER]                        ; BuildingType
Grinding.AllowAllies=false       ; bool, default false
Grinding.AllowOwner=true         ; bool, default true
Grinding.AllowTypes=             ; TechnoType list, default empty (all)
Grinding.DisallowTypes=          ; TechnoType list, default empty (none)
Grinding.Sound=                  ; sound, default [AudioVisual] EnterGrinderSound
Grinding.PlayDieSound=true       ; bool, default true
Grinding.Weapon=                 ; weapon, default none
Grinding.Weapon.RequiredCredits=0 ; integer, default 0
```

## Behavior

`BuildingExt::CanGrindTechno` admits an Infantry or Unit only when all of these
hold: the building type is `Grinding=`, the type's absorb flags accept that
class, a same-house victim passes `AllowOwner`, an allied victim passes
`AllowAllies`, and any non-empty `AllowTypes` names the victim type while
`DisallowTypes` does not. The predicate gates the Script60 seek (hook
`0x4DFABD`), the CAN_LOAD answer (`0x43C30A`), the mission-Eaten destination
check (`0x4D4CD3`) and the cursor rows.

CAN_LOAD for a `Grinding=` building answers before the shared admission: the
sender's ground/water class must match unless its zone is any amphibious one;
a building that is constructing, selling, in BState 0, offline, or a
balloon-hover sender refuses; otherwise the type gates answer ROGER or
NEGATORY. The Infantyman cursor (`0x51F0AF`) shows Grind through the type
gates for an allied grinder on any base action when `AllowAllies=yes`
(otherwise only on Select). The Unit cursor (`0x740134`) adds the same gates
after CAN_LOAD and answers NoEnter when a grinder refuses; holding the
force-fire key with an armed Unit skips that row. An Engineer over a full
health grinder (`0x51E63A`) answers the same way.

On arrival, `Grinding.PlayDieSound=no` suppresses the victim's VoiceDie and
DieSound. Every refund credited to the grinder's owner (the victim, nested
cargo and its parasite) accumulates on the building. Once the type's
`Grinding.Weapon=` is off its `ROF` cooldown and the accumulated refund
reaches `Grinding.Weapon.RequiredCredits=`, the weapon detonates on the
building itself (`WeaponTypeExt::DetonateAt`) and the accumulator clears. A
type `Grinding.Sound=` plays at the victim's coordinates and replaces the
stock `[AudioVisual] EnterGrinderSound=` for that visit.

## Where VERA20K owns it

`rules::phobos_grinding` reads the type keys; `native_processing` allocates
the weapon a `Grinding.Weapon=` names on the same rules pass, as Phobos'
`Read<true>` does. `Simulation::can_grind_techno` and
`Simulation::do_grinding_extras` in `sim::grinding` serve the hook sites in
`sim::radio::receive`, `sim::building_entry`, `sim::mission::eaten` and
`sim::world::world_orders`. The per-building accumulator and last-fired frame
live on `GameEntity` and join the state hash only when nonzero.

## Residuals

- Phobos' `Grinding_Weapon.Read<true>` uses `FindOrAllocate`: an unknown name
  creates an empty default weapon. VERA resolves the name against the rules
  set instead, so an unknown name resolves to no weapon and never fires.
- `DoGrindingExtras`' `AccumulatedIncome += refund` is wired
  (`src/sim/grinding.rs:142`): the refund the grinder actually credited feeds
  the building's `DisplayIncome` accumulator
  (`src/sim/building_display_income.rs`), and the label is printed by the
  existing `DisplayIncome=` owner. Regression test
  `grinding_refund_feeds_the_display_income_accumulator`
  (`src/sim/grinding_tests.rs:388`). No residual here.
